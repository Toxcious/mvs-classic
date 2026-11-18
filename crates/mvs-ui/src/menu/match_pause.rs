//! The match pause menu's ties to the running match: what UI_PauseMenu and
//! UI_InputBindingModal call on the match (PlayerController 0, the game instance, IsGamePaused) and what
//! UI_Game.MatchSetPause calls on the menu. The app's match implements the session for an offline non-Lab match
//! that has a pause menu and hands it to the menu's frame (`MenuRun::pause`); None otherwise.

use super::controller::Cx;

/// The running match as its pause menu sees it.
pub trait MatchPauseSession {
    /// PlayerController 0's BPI_Pause(on) (PC @164407): ResetInputs, then UI_Game.MatchSetPause(on, -1, false).
    fn pause(&mut self, on: bool);
    /// PlayerController 0's BPI_ResetInputs (PC @1622).
    fn reset_inputs(&mut self);
    /// UGameplayStatics IsGamePaused.
    fn is_game_paused(&self) -> bool;
    /// GameInstance BPI_GetIsLocalPlay.
    fn is_local_play(&self) -> bool;
    /// GetPlayerController(i) valid and its pawn IsLocallyControlled (UpdateAppearance @1363-1659).
    fn is_local_player(&self, i: i32) -> bool;
    /// What UI_Functions Color for Team reads outside Local Play (@891-3224): the active match config's team
    /// style (ETeamStyle) and player 0's team index (its MatchPlayerData).
    fn team_style(&self) -> i32;
    fn local_team(&self) -> i32;
    /// Leave Match: BPI_RecreateParty, Transition(New_MainMenu, type 0, "Lobby"), BPI_PlayMusOST.
    fn leave_match(&mut self);
    /// The game mode's BPI_GetGameModeType (4: TutorialGameMode).
    fn game_mode_type(&self) -> i32;
    /// GameState BPI_IsTutorialStepSkippable, and BPI_SkipTutorialStep / BPI_SkipToTutorialEndStep on its
    /// tutorial actors.
    fn is_tutorial_step_skippable(&self) -> bool;
    fn skip_tutorial_step(&mut self);
    fn skip_to_tutorial_end_step(&mut self);
    /// IsPlayInEditor.
    fn is_play_in_editor(&self) -> bool;
}

/// UI_PauseMenu's own events that UI_Game calls: OnPaused at each MatchSetPause, Init at each pause. Its
/// controller returns itself from `Controller::as_pause_menu`.
pub trait PauseMenuWidget {
    fn on_paused(&mut self, cx: &mut Cx<'_>, paused: bool, player_pausing: i32, is_controller_disconnect: bool);
    fn init(&mut self, cx: &mut Cx<'_>);
}
