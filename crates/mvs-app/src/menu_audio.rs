//! The menus' sound on the sound engine: the `MenuSink` the
//! menu views post through, the menu music flow across the title and the main menu visits, and the Collection's
//! announcer equip line.

use mvs_audio::Audio;
use mvs_audio::tape::post::{Call, Line};
use mvs_ui::menu::MenuSink;
use mvs_ui::menu::prefs::{DisplayInfo, WindowInfo};

use crate::apply;

/// The game object the menus' posts go on (`{ owner: -1 }`: the game's own).
const UI_OBJECT: i32 = -1;

/// A call on the sound engine (dropped without one), on the program's post tape (`--audio-tape`) either way.
fn call(audio: &mut Option<Audio>, tick: u32, c: Call) {
    crate::audio::send(audio, &Line { tick, call: c });
}

/// The menus' sink: posts, parameters, switches and states by name, stamped with the menu's frame count.
pub struct MenuAudio<'a> {
    pub audio: &'a mut Option<Audio>,
    pub tick: u32,
    /// The window's mode, framed size and display (the Graphics screen's start state).
    pub window: Option<WindowInfo>,
    /// Where an announcer pack's equip is left for the line (None: a sink of no screen with the Collection).
    pub equip: Option<&'a mut AnnouncerEquip>,
}

/// The announcer's line when a pack is equipped in the Collection (UI_CollectionChooser @29359-29906, after
/// SetAnnouncerPack: LoadStoredAnnouncerPackAssetData, the game state's BPI_InitVOData(pack.DTVO), then
/// BPI_PlayVO("announcer_characterselect_intro", "", AlwaysPlayVO, not PlayOnlyLocally): pfgPlayVO on the game state's
/// component, whose only gate is its IsCurrentlySpeaking (InitVOData leaves it as it was), so a line while the last
/// one speaks is dropped. VOLocal posts it on the game state, 2D.
/// Its event is held (the `announcerEquip` scope) from its post to a second after its end.
// Deviation: the line's end is its length from the audio data, not the engine's callback.
#[derive(Debug, Default)]
pub struct AnnouncerEquip {
    /// The pack equipped this frame, its line not yet asked for.
    asked: Option<String>,
    /// The menus' clock until which the last line speaks, s.
    until: f64,
    /// The menus' clock at which the scope is let go.
    release: Option<f64>,
}

/// The scope the equip line is held under.
const EQUIP_SCOPE: &str = "announcerEquip";
/// The alias the equip line plays.
const EQUIP_ALIAS: &str = "announcer_characterselect_intro";

impl AnnouncerEquip {
    /// The frame's end at the menus' clock `now`: the equipped pack's line posted (`store` holds the packs), the
    /// last line's scope let go when due.
    pub fn frame(&mut self, store: Option<&mvs_store::Store>, audio: &mut Option<Audio>, tick: u32, now: f64) {
        let Some(store) = store else { return };
        let length =
            |ev: &str| audio.as_ref().and_then(|a| a.init().events.get(ev).and_then(|e| e.dur[1])).unwrap_or(0.0);
        for c in self.take_calls(store, length, now) {
            call(audio, tick, c);
        }
    }

    /// The equipped pack's line asked for (`MenuSink::announcer_equipped`).
    pub fn ask(&mut self, pack_path: &str) {
        self.asked = Some(pack_path.to_owned());
    }

    /// `frame`'s calls: the line of the pack asked for, if any (`length` an event's length, s), and the scope's let
    /// go when due.
    pub fn take_calls(&mut self, store: &mvs_store::Store, length: impl Fn(&str) -> f64, now: f64) -> Vec<Call> {
        let mut out = Vec::new();
        if let Some(path) = self.asked.take() {
            match mvs_data::stored_announcer_pack(store, &path) {
                Ok(pack) => out.extend(self.line(&pack, length, now)),
                Err(e) => eprintln!("mvs: the announcer pack {path}: {e}"),
            }
        }
        if self.release.is_some_and(|r| now >= r) {
            self.release = None;
            out.push(Call::Scope { name: EQUIP_SCOPE.to_owned(), keep: Vec::new(), lazy: Vec::new() });
        }
        out
    }

    /// The pack's equip line, none while the last one speaks.
    fn line(&mut self, pack: &mvs_data::AnnouncerPack, length: impl Fn(&str) -> f64, now: f64) -> Vec<Call> {
        // (one untagged row per alias in every pack of the build)
        let row = pack.rows.iter().find(|r| r.0.eq_ignore_ascii_case(EQUIP_ALIAS) && r.1.is_empty());
        let Some(row) = row else { return Vec::new() };
        if now < self.until {
            return Vec::new();
        }
        let ev = row.3.to_lowercase();
        if !ev.contains("eff") {
            self.until = now + length(&ev);
        }
        self.release = Some(self.until.max(now) + 1.0);
        vec![
            Call::Scope { name: EQUIP_SCOPE.to_owned(), keep: vec![ev.clone()], lazy: Vec::new() },
            Call::Post { ev, owner: Some(UI_OBJECT), key: None, stop_key: None, at: None },
        ]
    }
}

impl MenuAudio<'_> {
    /// A post on the menus' object, at a UE location when given (PF_Post_Event_At_Location).
    pub fn post_at(&mut self, event: &str, at: Option<[f64; 3]>) {
        call(
            self.audio,
            self.tick,
            Call::Post { ev: event.to_owned(), owner: Some(UI_OBJECT), key: None, stop_key: None, at },
        );
    }
}

impl MenuSink for MenuAudio<'_> {
    fn post(&mut self, event: &str) {
        call(
            self.audio,
            self.tick,
            Call::Post { ev: event.to_owned(), owner: Some(UI_OBJECT), key: None, stop_key: None, at: None },
        );
    }

    fn has_event(&self, event: &str) -> bool {
        // (no sound engine: every event is one, so the menus' flow is the same with the sound off)
        self.audio.as_ref().is_none_or(|a| a.has(event))
    }

    fn set_rtpc(&mut self, name: &str, value: f64, interp_ms: f64) {
        call(self.audio, self.tick, Call::SetRtpc { name: name.to_owned(), value, owner: UI_OBJECT, interp_ms });
    }

    fn set_switch(&mut self, group: &str, value: &str) {
        call(
            self.audio,
            self.tick,
            Call::SetSwitch { group: group.to_owned(), value: value.to_owned(), owner: UI_OBJECT },
        );
    }

    fn set_state(&mut self, group: &str, value: &str) {
        call(self.audio, self.tick, Call::SetState { group: group.to_owned(), value: value.to_owned() });
    }

    fn announcer_equipped(&mut self, pack_path: &str) {
        if let Some(e) = self.equip.as_deref_mut() {
            e.ask(pack_path);
        }
    }

    fn clipboard(&mut self, text: &str) {
        // (the menus' window has the keyboard when a click copies)
        if let Err(e) = crate::launcher::setup_os::copy_text_foreground(text) {
            eprintln!("mvs: the clipboard: {e}");
        }
    }

    fn open_folder(&mut self, path: &str) {
        if let Err(e) = crate::launcher::setup_os::open_folder(std::path::Path::new(path)) {
            eprintln!("mvs: open the folder {path}: {e}");
        }
    }

    fn displays(&self) -> Vec<DisplayInfo> {
        apply::display_infos()
    }

    fn window(&self) -> WindowInfo {
        // (no window given: the trait's defaults)
        self.window.clone().unwrap_or(WindowInfo {
            mode: mvs_settings::WindowMode::Windowed,
            size: [1280, 720],
            display: 0,
        })
    }
}

/// An event the menu scope holds: a `ui_` one, or one the menus' code posts by name (`posted::POSTED`'s area `menu`),
/// the music's `mus_` events aside (the music holds its own media).
fn menu_held(e: &str) -> bool {
    use crate::audio::posted::POSTED;
    e.starts_with("ui_")
        || (!e.starts_with("mus_")
            && POSTED.binary_search_by(|(n, _)| (*n).cmp(e)).is_ok_and(|i| POSTED[i].1.contains(&"menu")))
}

/// The menu music's state across the menus' visits.
#[derive(Debug, Default)]
pub struct MenuMusic {
    playing: bool,
    visits: u32,
    /// The switch values a level asked for whose media the menus hold (`level_switch`).
    held: Vec<String>,
    /// The frames the menus have run (the sink's tick).
    pub tick: u32,
}

impl MenuMusic {
    /// The title screen's sound before it shows: the music's pieces the title and the main menu play and the menus'
    /// events held. Unverified: the three playlists of the menus are held from the start, and the menu scope is every
    /// `ui_` event and every other one the menus' code posts by name but the music's (the announcer's
    /// vo_announcer_toast of the podium's toast button, UI_ButtonToast @230, and the rematch lines), read when first
    /// posted (the menu bundle's named ones and those the menus post).
    pub fn preload(&mut self, audio: &mut Option<Audio>) {
        let Some(a) = audio else { return };
        let tick = self.tick;
        let lazy: Vec<String> = a.init().events.keys().filter(|e| menu_held(e)).cloned().collect();
        let scope = Call::Scope { name: "menu".to_owned(), keep: Vec::new(), lazy };
        let music = Call::PreloadMusic {
            ev: "mus_play".to_owned(),
            values: ["mus_menu_main_welcome", "mus_menu_main_intro", "mus_menu_main_intro_ambient"]
                .map(str::to_owned)
                .to_vec(),
        };
        for c in [scope, music, Call::Resume] {
            call(audio, tick, c);
        }
    }

    /// PandaGameInstance StartMusic (once, IsMusicPlaying): global switch mus-z_mus_none, map state none, global event
    /// mus_play.
    pub fn start_music(&mut self, audio: &mut Option<Audio>) {
        let tick = self.tick;
        for c in self.start_calls() {
            call(audio, tick, c);
        }
    }

    /// StartMusic's calls, none once the music plays (they go where the caller's calls go: a match's begin, its tape).
    pub fn start_calls(&mut self) -> Vec<Call> {
        if self.playing {
            return Vec::new();
        }
        self.playing = true;
        let switch = |group: &str, value: &str| Call::SetSwitch {
            group: group.to_owned(),
            value: value.to_owned(),
            owner: UI_OBJECT,
        };
        vec![
            switch("mus", "z_mus_none"),
            switch("mus_map_state", "z_mus_map_state_none"),
            Call::Post { ev: "mus_play".to_owned(), owner: Some(UI_OBJECT), key: None, stop_key: None, at: None },
        ]
    }

    /// The title screen's music: BPI_StartMusic, then PF_Post_Global_Switch(mus-mus_menu_main_welcome) and, from the
    /// WelcomeScreen level Blueprint (@6068), SetState(state_gameflow-state_gameflow_mainmenu).
    pub fn title(&mut self, audio: &mut Option<Audio>) {
        self.start_music(audio);
        let mut s = MenuAudio { audio, tick: self.tick, window: None, equip: None };
        s.set_switch("mus", "mus_menu_main_welcome");
        s.set_state("state_gameflow", "state_gameflow_mainmenu");
    }

    /// The main menu's music. UI_MainMenu OnResourcesLoaded (@7561): on the first visit (IsFirstMainMenuVisit:
    /// BPI_GetMainMenuCount <= 1) the switch mus-mus_menu_main_intro (@7619); on every visit BPI_SetMusicMapState(6)
    /// in the same frame (@7821) and, a Delay(0.2) later (@7670 -> @945), BPI_PlayMusOST (@1044). The gameflow state
    /// is New_MainMenu's level Blueprint (@2714). The build never goes back to mus_menu_main_welcome, and the intro
    /// plays once: their media is let go.
    pub fn menu(&mut self, audio: &mut Option<Audio>) {
        self.start_music(audio);
        let mut s = MenuAudio { audio, tick: self.tick, window: None, equip: None };
        s.set_state("state_gameflow", "state_gameflow_mainmenu");
        self.visits += 1;
        if self.visits == 1 {
            let mut s = MenuAudio { audio, tick: self.tick, window: None, equip: None };
            s.set_switch("mus", "mus_menu_main_intro");
            call(
                audio,
                self.tick,
                Call::ReleaseMusic { ev: "mus_play".to_owned(), values: vec!["mus_menu_main_welcome".to_owned()] },
            );
        }
        MenuAudio { audio, tick: self.tick, window: None, equip: None }
            .set_switch("mus_map_state", "z_mus_map_state_none");
    }

    /// A level's PF_Post_Global_Switch of the music group `mus` (the PreMatch level's SetupCharSelect @18136:
    /// mus_menu_chaselect): the value's media held first, once (`preload` holds the title's and the main menu's),
    /// then the switch.
    // Deviation: the value's media are held (PreloadMusic, once a run, kept) when the level asks for the switch, not
    // by the build's bank loads.
    pub fn level_switch(&mut self, audio: &mut Option<Audio>, value: &str) {
        if !self.held.iter().any(|v| v == value) {
            self.held.push(value.to_owned());
            let values = vec![value.to_owned()];
            call(audio, self.tick, Call::PreloadMusic { ev: "mus_play".to_owned(), values });
        }
        MenuAudio { audio, tick: self.tick, window: None, equip: None }.set_switch("mus", value);
    }

    /// PandaGameInstance BPI_PlayMusOST (@33601): OstSwitch is mus_ost-mus_ambient (the class default; BPI_SetMusOST is
    /// called with a button's Music_Ost only, which no widget of the dump sets), so the switch
    /// mus-mus_menu_main_intro_ambient (@33746).
    pub fn play_mus_ost(&mut self, audio: &mut Option<Audio>) {
        MenuAudio { audio, tick: self.tick, window: None, equip: None }
            .set_switch("mus", "mus_menu_main_intro_ambient");
        if self.visits > 0 {
            call(
                audio,
                self.tick,
                Call::ReleaseMusic { ev: "mus_play".to_owned(), values: vec!["mus_menu_main_intro".to_owned()] },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_music_posts_mus_play_once() {
        let mut m = MenuMusic::default();
        let calls = m.start_calls();
        assert!(matches!(&calls[..], [
            Call::SetSwitch { value: a, .. },
            Call::SetSwitch { value: b, .. },
            Call::Post { ev, owner: Some(-1), .. },
        ] if a == "z_mus_none" && b == "z_mus_map_state_none" && ev == "mus_play"));
        // (the title's, then a match's first input: the music plays already)
        assert!(m.start_calls().is_empty());
    }

    #[test]
    fn the_menu_scope_holds_the_announcer_lines_the_menus_post() {
        assert!(menu_held("vo_announcer_toast"));
        assert!(menu_held("vo_announcer_rematch_accept"));
        assert!(menu_held("ui_menu_notification"));
        assert!(!menu_held("mus_play"));
        assert!(!menu_held("vo_announcer_ko"));
    }
}
