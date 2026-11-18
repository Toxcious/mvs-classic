//! The Lab's training config as its menu reads and writes it: Struct_TrainingConfig
//! on TrainingModePlayerState, sent with RequestConfigUpdate and echoed back by
//! TrainingGameState.BroadcastAuthorityConfig. The match owns the session; the menu's widgets see only `LabSession`.

/// Struct_TrainingPlayerConfig. An empty account id is an empty slot; a `None` character is a Random pick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabPlayerConfig {
    pub account_id: String,
    pub player_name: String,
    pub character: Option<String>,
    pub skin: String,
    pub starting_damage: f64,
    /// EBotBehavior; 0 = a human, [`LAB_EXPERT`] the port's Expert bot.
    pub bot_difficulty: u8,
    pub team_index: i32,
    /// Perks: PerkData asset names; the panel's row sends its four slots in order, an empty one None
    /// (UI_LAB_PlayerPanel BuildPlayerTrainingConfig @102 / @610).
    pub perks: Vec<Option<String>>,
}

/// (the port's own) The Lab's Expert entry (the fight menu's choice after Hard): an EBotBehavior value past the
/// build's last (28, Patrolling), so the build's values keep their entries. Its bot is the Hard entry's with the
/// Expert layer (`BtBot::match_bot` at the Expert level).
pub const LAB_EXPERT: u8 = 29;

/// Struct_TrainingConfig.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabConfig {
    pub players: Vec<LabPlayerConfig>,
    pub leader_account_id: String,
    /// SelectedMap: a MapData's asset name.
    pub selected_map: String,
    pub instant_cooldowns: bool,
    pub same_attack_penalty: bool,
}

/// The Lab's toggles a restart carries (LabSession's hitstunCombo, juggleCombo, hazards).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LabToggles {
    /// TrainingGameState bDisplayHitstunComboDamage.
    pub hitstun_combo: bool,
    /// bDisplayJuggleComboDamage.
    pub juggle_combo: bool,
    /// The match config's AllowHazards.
    pub hazards: bool,
}

/// The running Lab as its menu sees it; the app's Lab match implements it.
pub trait LabSession {
    /// TrainGameMode.AuthorityTrainingConfig.
    fn config(&self) -> &LabConfig;
    /// BPI_GetAccountGUID of the local player.
    fn me(&self) -> &str;
    fn toggles(&mut self) -> &mut LabToggles;
    /// PS.RequestConfigUpdate(config): the authority takes it and broadcasts it back (`take_config`). Dropped while
    /// the broadcast is being handled (PS.bRespondingToConfigUpdate, TrainingModePlayerState @5113).
    fn request_config_update(&mut self, config: LabConfig);
    /// PS.RequestAuthorityConfig: the authority's config comes back through `take_config`.
    fn request_authority_config(&mut self);
    /// PS.RequestKickPlayer -> TrainGameMode.KickPlayer: the slot emptied and the config broadcast.
    fn request_kick_player(&mut self, account_id: &str);
    /// PlayerController.BPI_Pause.
    fn pause(&mut self, on: bool);
    /// TrainingGameState.OnTrainingConfigUpdateFromServer: the config broadcast since the last call.
    fn take_config(&mut self) -> Option<LabConfig>;
    /// MatchmakingHandler.bIsMatchmaking: the online queue searches while the Lab runs (Wait in the Lab).
    fn is_matchmaking(&self) -> bool;
    /// TrainGameMode Back To Matchmaking TGM@4005 (UI_LAB_Panel OnQuitConfirm LP@13490).
    fn back_to_matchmaking(&mut self);
}

/// An empty slot.
pub fn empty_lab_player() -> LabPlayerConfig {
    LabPlayerConfig::default()
}

/// TrainingGameState ArePlayerConfigsEqual(a, b) (function @0-2007): every field, then the perks over a's length,
/// a missing entry of b read as None (Array_Get out of range): [] equals four empty slots.
pub fn lab_players_equal(a: &LabPlayerConfig, b: &LabPlayerConfig) -> bool {
    a.team_index == b.team_index
        && a.bot_difficulty == b.bot_difficulty
        && a.starting_damage == b.starting_damage
        && a.skin == b.skin
        && a.character == b.character
        && a.player_name == b.player_name
        && a.account_id == b.account_id
        && a.perks.iter().enumerate().all(|(i, p)| p.as_deref() == b.perks.get(i).and_then(Option::as_deref))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perks_compare_over_the_first_list() {
        let a = LabPlayerConfig { perks: vec![None; 4], ..empty_lab_player() };
        assert!(lab_players_equal(&a, &empty_lab_player()));
        assert!(lab_players_equal(&empty_lab_player(), &a));
        let b = LabPlayerConfig { perks: vec![Some("P".into())], ..empty_lab_player() };
        assert!(!lab_players_equal(&b, &a));
        // (a's list is the one walked: b's extra perk is not seen)
        assert!(lab_players_equal(&empty_lab_player(), &b));
    }
}
