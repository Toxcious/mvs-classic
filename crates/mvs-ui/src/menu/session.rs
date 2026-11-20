//! What the menus keep from one screen to the next (the homescreen queue, Local Play flag and started match,
//! `Lobby.kept`): one value the program holds
//! across its menu views (`Shell` hands it to the view it makes and takes it back at the view's end).

use super::arcade::ArcadeState;
use super::lobby::LocalLobby;
use super::local_play_match::LocalPlayStart;
use super::toasts::Toasts;
use super::widgets::end_of_game::EogState;
use super::widgets::toast_button::PodiumToasts;

/// EHomescreenQueue's Local Play entry (BPI_LocalPlayBegin @35771, ShowMainMenu @38864).
pub const HOMESCREEN_LOCAL_PLAY: i32 = 14;

/// The menus' state across screens.
#[derive(Debug, Default)]
pub struct Session {
    homescreen_queue: Vec<i32>,
    local_play: bool,
    /// The match LocalPlayFacilitator started, for the program when the level it opened is up.
    pub started: Option<Box<LocalPlayStart>>,
    /// The custom lobby kept through its match (`Lobby.kept`), shown again at the match's end.
    pub lobby: Option<Box<LocalLobby>>,
    /// QuitGame: asked for by the home screen's quit confirmation; the program quits at the frame's end.
    pub quit: bool,
    /// The results screen's data and state.
    pub eog: Box<EogState>,
    /// The toast service (one per session) and the podium's toast buttons.
    pub toasts: Toasts,
    pub podium_toasts: PodiumToasts,
    /// The arcade run the screens read.
    pub arcade: Box<ArcadeState>,
    /// (the port's own) The Waves mode's pick and its last run's result (`waves.rs`).
    pub waves: Box<super::waves::WavesState>,
    /// The stats file: the profile, the badges' counts, the arcade and
    /// tutorial records and the toasts.
    pub stats: mvs_settings::stats::StatsFile,
    /// The main menu's online state, with the queue the main menu's Confirm asked for
    /// and the note (boxed: the session is moved by value through the program's frames).
    pub main_menu: Box<super::widgets::main_menu::MainMenuState>,
    /// Online play as the program's online client gave it for this frame. The one copy the
    /// menus read: the client sets the whole view before each frame, its `events` replaced with the frame's (an empty
    /// list when nothing happened); every reader reads the events and none takes them, so each sees each event once
    /// per frame.
    pub online: super::online::OnlineView,
    /// (the port's own) The main menu's backgrounds the home screen's switch cycles through, in order (mvs-render
    /// `Renderer::menu_background_options`: "" the plane's own, then each Event Lookup Table slug); empty: no switch.
    pub menu_backgrounds: Vec<String>,
    /// The game instance's TutorialActorClass (BPI_SetTutorialActorClass, tutorial.rs `launch_tutorial`): the class
    /// the tutorial level runs, kept until the next launch.
    pub tutorial_actor: Option<String>,
    /// The game instance's AdvTutShortcut (BPI_SetAdvTutShortcut / BPI_IsAdvTutShortcut): set by the tutorial end
    /// screen's View More Tutorials, read and cleared by the home screen at its construct
    /// (HandleIfAdvTutorialShortcut), which then opens the advanced tutorials' tiles.
    pub adv_tut_shortcut: bool,
}

impl Session {
    /// The menus' state of a run whose stats are `stats` (the data folder's file).
    pub fn new(stats: mvs_settings::stats::StatsFile) -> Session {
        Session { stats, ..Session::default() }
    }

    /// UI_Functions AddToHomescreenQueue(Item, Critical) @0-607: an item already queued is not added again; a
    /// critical one goes first, any other last.
    pub fn add_to_homescreen_queue(&mut self, item: i32, critical: bool) {
        if self.homescreen_queue.contains(&item) {
            return;
        }
        if critical {
            self.homescreen_queue.insert(0, item);
        } else {
            self.homescreen_queue.push(item);
        }
    }

    /// ShowMainMenu @38841-39034: Array_Contains, then Array_RemoveItem and BPI_SetHomescreenQueue.
    pub fn take_from_homescreen_queue(&mut self, item: i32) -> bool {
        let Some(at) = self.homescreen_queue.iter().position(|&i| i == item) else { return false };
        self.homescreen_queue.remove(at);
        true
    }

    /// BPI_SetIsLocalPlay (PGI @35713): true SetMatchFlowID("LocalPlay", lock true), false ResetMatchFlow;
    /// BPI_GetIsLocalPlay = MatchFlowID == "LocalPlay" (@9170). ShowMainMenu's ResetMatchFlow (@36324) clears it too.
    pub fn set_is_local_play(&mut self, on: bool) {
        self.local_play = on;
    }

    pub fn is_local_play(&self) -> bool {
        self.local_play
    }
}
