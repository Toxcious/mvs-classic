//! The PreMatch level's voices: what
//! the pre-match screen asks of its level and game state as far as it is sound. The preview actors are their
//! Comp_Actor_VO alone; the announcer is the game state's.
//!
//! The announcer (UI_PreMatch @16172-16531 at the screen's start, @29035-29394 when the match is starting): the game
//! state's BPI_PlayVO(alias, "", AlwaysPlayVO, not PlayOnlyLocally) = pfgPlayVO on its own Comp_Actor_VO. The owner
//! is no pawn: of the gates only the component's IsCurrentlySpeaking is left, one untagged row per alias.
//! Unverified: the PreMatch level's game state class is not in the dumps; read as one that implements BPI_VO as
//! PandaGameState does (the only implementation in the pseudo).
//!
//! A preview actor (BP_CharacterPreview BPI_PlayVo @35247 -> its Comp_Actor_VO.pfgPlayVO): InitVOData from the applied
//! skin's VODataTable. The owner is no pawn: nothing while the component speaks; without AlwaysPlayVO nothing either
//! while the game state counts a VO speaker (@1988-2188: the announcer's line holds back a click line, not the
//! select line). With no extra tag the tags are none (`select_vo` is given the extra tags alone). Only the local
//! player's preview is made to speak (PreMatch.onConfirmP0, VO_Clickable), so only its character's data is read, when
//! the preview's actor has settled on it (`MenuEvent::PreviewSettled`: a tile crossed asks for nothing), or at a
//! press on it, and let go when it gets another or the screen ends.
//! Unverified: the tags of a select line are taken as none: every `chaselect` row of the three ported tables is
//! untagged with priority 0, so the other previews' tags and the map's choose nothing; a table with a tagged row
//! needs them. VoiceOverEnabled is taken as on.
//! Deviation: a line's end is its length from the audio data, not the engine's callback; the lines are posted
//! without a position (the build: the actor's AkComponent). The data is read on the loader from the fighter's change
//! on, beside its preview's view, and taken at the settle (waited for there when it is not in yet, as a press before
//! it does): a line is not late by its data's load.
//! The announcer's pack is this machine's equipped one (LoadStoredAnnouncerPackAssetData, `mvs_data::stored_announcer_pack`),
//! read when the screen opens: nothing equips a pack while it shows.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mvs_audio::Audio;
use mvs_audio::tape::post::{Call, Line};
use mvs_data::{AnnouncerPack, GameData};
use mvs_render::menuscene::PreviewSpec;
use mvs_sim::fighter::match_side::{select_vo, skin_vo_data};
use mvs_ui::menu::scene::SceneCmd;
use mvs_ui::menu::screens::pre_match::setup::{AnnouncerLine, PreMatchCall};

/// The game object of the game's own posts.
const OWNER: i32 = -1;
/// BPI_GetPreviewActor(PlayerIndex 0).
const LOCAL_PREVIEW: usize = 0;
/// The scope the screen holds its lines under.
const SCOPE: &str = "prematch";
/// The announcer's two lines of the screen.
const ANNOUNCER_PREFIX: &str = "announcer_characterselect_";

#[derive(Debug, Default)]
struct Actor {
    key: Option<String>,
    skin: String,
    recent: i32,
    until: f64,
}

/// The screen's voices.
#[derive(Debug)]
pub struct PreMatchVoice {
    /// The stored announcer pack, once read.
    announcer: Option<AnnouncerPack>,
    /// The data read so far (the opening read, then a settle's): a character is read once.
    cache: Vec<Arc<GameData>>,
    /// The stage key the data loads under.
    stage: String,
    /// The local preview's character, its skin, and its data once read.
    held: Option<String>,
    held_skin: String,
    held_data: Option<Arc<GameData>>,
    /// The held character's data reading on the loader (from its change of fighter).
    /// Its cancel flag: a read still queued when the preview moves on does not start (a sweep across the grid
    /// leaves no reads ahead of the last hover's).
    reading: Option<(String, mvs_load::Ticket<Option<GameData>>, Arc<AtomicBool>)>,
    actors: HashMap<usize, Actor>,
    announcer_until: f64,
    speakers: Vec<f64>,
    over: bool,
    rng: u64,
    /// The lines last held (so the scope is sent when they change).
    held_lines: Vec<String>,
    /// The tick of the sound calls.
    pub tick: u32,
}

fn call(audio: &mut Option<Audio>, tick: u32, c: Call) {
    crate::audio::send(audio, &Line { tick, call: c });
}

impl PreMatchVoice {
    /// preMatchVoice(): the announcer's lines held as files (the stored pack, at `announcer_path`, read at once).
    ///
    /// The data is read once here, for the characters the setup names (a settle's own read is 60 ms in the exe: a
    /// hitch of 4 frames), and kept; a character it lacks is read at its settle, once.
    pub fn new(
        store: &mvs_store::Store,
        audio: &mut Option<Audio>,
        tick: u32,
        level: &str,
        keys: &[&str],
        announcer_path: &str,
    ) -> PreMatchVoice {
        // the match's stage (the screen's level, else the store's first): the table needs a stage to load, not its data
        let stage = mvs_data::stage_levels(store)
            .ok()
            .and_then(|l| l.iter().find(|(lv, _)| lv == level).or(l.first()).map(|(_, k)| k.clone()))
            .unwrap_or_default();
        let data = match GameData::load(store, keys, &stage) {
            Ok(d) => Some(Arc::new(d)),
            Err(e) => {
                eprintln!("mvs: pre-match voice: no data: {e:?}");
                None
            }
        };
        let announcer = match mvs_data::stored_announcer_pack(store, announcer_path) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("mvs: pre-match voice: no announcer pack: {e:?}");
                None
            }
        };
        let seed =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        let mut v = PreMatchVoice {
            cache: data.into_iter().collect(),
            announcer,
            stage,
            held: None,
            held_skin: String::new(),
            held_data: None,
            reading: None,
            actors: HashMap::new(),
            announcer_until: -1.0,
            speakers: Vec::new(),
            over: false,
            rng: seed | 1,
            held_lines: Vec::new(),
            tick,
        };
        v.hold(audio);
        v
    }

    fn random(&mut self, n: f64) -> f64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let r = (self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64;
        (r * n).floor()
    }

    /// The announcer's rows of the screen's two lines: (alias, event).
    fn announcer_rows(&self) -> Vec<(String, String)> {
        let Some(p) = &self.announcer else { return Vec::new() };
        p.rows
            .iter()
            .filter(|r| r.0.to_lowercase().starts_with(ANNOUNCER_PREFIX) && r.1.is_empty())
            .map(|r| (r.0.to_lowercase(), r.3.clone()))
            .collect()
    }

    /// The rows a preview can be asked for: the select line with no tag, the click line with its one tag.
    fn lines(&self) -> Vec<String> {
        let (Some(key), Some(d)) = (&self.held, &self.held_data) else { return Vec::new() };
        let Some(c) = d.chars.get(key) else { return Vec::new() };
        let skin = skin_vo_data(c, c, Some(&self.held_skin).filter(|s| !s.is_empty()).map(String::as_str));
        let rows = skin.rows.unwrap_or(&[]);
        let can = |alias: &str, tags: &[&str]| -> Vec<String> {
            rows.iter()
                .filter(|(al, tg, ..)| {
                    al.eq_ignore_ascii_case(alias)
                        && tg.split('|').all(|t| t.is_empty() || tags.iter().any(|s| s.eq_ignore_ascii_case(t)))
                })
                .filter_map(|r| r.3.clone().filter(|e| !e.is_empty()))
                .collect()
        };
        let mut out = can("chaselect", &[]);
        out.extend(can("clickable", &["affinity_level_01"]));
        out
    }

    /// The scope the screen holds: the announcer's two lines and the local preview's two, lazy (they start on a press
    /// or a state change, not on a frame); empty at the end.
    fn hold(&mut self, audio: &mut Option<Audio>) {
        let mut lazy: Vec<String> = Vec::new();
        if !self.over {
            lazy.extend(self.announcer_rows().into_iter().map(|r| r.1));
            lazy.extend(self.lines());
        }
        let lazy: Vec<String> = lazy.into_iter().map(|e| e.to_lowercase()).collect();
        if lazy == self.held_lines {
            return;
        }
        self.held_lines = lazy.clone();
        call(audio, self.tick, Call::Scope { name: SCOPE.to_owned(), keep: Vec::new(), lazy });
    }

    fn length(audio: &Option<Audio>, ev: &str) -> f64 {
        audio.as_ref().and_then(|a| a.init().events.get(&ev.to_lowercase()).and_then(|e| e.dur[1])).unwrap_or(0.0)
    }

    /// Posts `ev` (the game's own) and counts a speaker until its end: an event with "eff" is no speech.
    fn post(&mut self, audio: &mut Option<Audio>, now: f64, ev: &str) -> Option<f64> {
        call(
            audio,
            self.tick,
            Call::Post { ev: ev.to_owned(), owner: Some(OWNER), key: None, stop_key: None, at: None },
        );
        if ev.contains("eff") {
            return None;
        }
        let until = now + Self::length(audio, ev);
        self.speakers.retain(|u| *u > now);
        self.speakers.push(until);
        Some(until)
    }

    /// The preview commands the screen left for the level (BPI_ChangeFighter / BPI_ClearFighter), in order.
    pub fn scene_cmds(&mut self, store: &mvs_store::Store, audio: &mut Option<Audio>, cmds: &[SceneCmd]) {
        for c in cmds {
            let SceneCmd::Preview { slot, spec } = c else { continue };
            self.change_fighter(store, audio, *slot, spec.as_ref());
        }
    }

    fn change_fighter(
        &mut self,
        store: &mvs_store::Store,
        audio: &mut Option<Audio>,
        preview: usize,
        spec: Option<&PreviewSpec>,
    ) {
        let a = self.actors.entry(preview).or_default();
        a.key = spec.map(|s| s.key.clone());
        a.skin = spec.map(|s| s.skin.clone()).unwrap_or_default();
        if preview != LOCAL_PREVIEW {
            return;
        }
        let (key, skin) = (a.key.clone(), a.skin.clone());
        if key == self.held {
            // (the same fighter in another skin: that skin's rows, once the data is in)
            if skin != self.held_skin {
                self.held_skin = skin;
                self.hold(audio);
            }
            return;
        }
        self.held = key;
        self.held_skin = skin;
        self.held_data = None;
        self.cancel_reading();
        if let Some(key) = self.held.clone().filter(|k| !self.cache.iter().any(|d| d.chars.contains_key(k))) {
            let (store, stage, k) = (store.clone(), self.stage.clone(), key.clone());
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            let read = mvs_load::spawn(move || {
                if flag.load(Ordering::Relaxed) {
                    return None;
                }
                match GameData::load(&store, &[&k], &stage) {
                    Ok(d) => Some(d),
                    Err(e) => {
                        eprintln!("mvs: pre-match voice: no data for {k}: {e:?}");
                        None
                    }
                }
            });
            self.reading = Some((key, read, cancel));
        }
        self.hold(audio);
    }

    /// Drops the read on the loader: one not started yet never starts.
    fn cancel_reading(&mut self) {
        if let Some((_, _, cancel)) = self.reading.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// previewSettled: the local preview has settled on `key`: its data is read, and its lines held.
    pub fn settled(&mut self, store: &mvs_store::Store, audio: &mut Option<Audio>, slot: usize, key: &str) {
        if slot != LOCAL_PREVIEW || self.over || self.held.as_deref() != Some(key) || self.held_data.is_some() {
            return;
        }
        self.held_data = self.read(store, key);
        self.hold(audio);
    }

    fn read(&mut self, store: &mvs_store::Store, key: &str) -> Option<Arc<GameData>> {
        if let Some(d) = self.cache.iter().find(|d| d.chars.contains_key(key)) {
            return Some(d.clone());
        }
        if self.reading.as_ref().is_some_and(|(k, ..)| k == key) {
            let (_, read, _) = self.reading.take().expect("matched above");
            let d = Arc::new(read.wait()?);
            self.cache.push(d.clone());
            return Some(d);
        }
        match GameData::load(store, &[key], &self.stage) {
            Ok(d) => {
                let d = Arc::new(d);
                self.cache.push(d.clone());
                Some(d)
            }
            Err(e) => {
                eprintln!("mvs: pre-match voice: no data for {key}: {e:?}");
                None
            }
        }
    }

    /// What the widget left for the sound side: a preview's line, or the announcer's.
    pub fn calls(&mut self, store: &mvs_store::Store, audio: &mut Option<Audio>, now: f64, calls: Vec<PreMatchCall>) {
        for c in calls {
            match c {
                PreMatchCall::PlayVo { preview, alias, tags, always } => {
                    self.play_vo(store, audio, now, preview, &alias, &tags, always);
                }
                PreMatchCall::Announcer(line) => self.announce(audio, now, line),
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // hooks.playVo's own parameters, with the program's parts
    fn play_vo(
        &mut self,
        store: &mvs_store::Store,
        audio: &mut Option<Audio>,
        now: f64,
        preview: usize,
        alias: &str,
        tags: &str,
        always: bool,
    ) {
        if self.over {
            return;
        }
        let Some(a) = self.actors.get(&preview) else { return };
        let Some(key) = a.key.clone() else { return };
        // (a press before the settle takes the data as the settle would; any other preview's is read for the line)
        let data = if self.held.as_deref() == Some(&key) {
            if self.held_data.is_none() {
                self.held_data = self.read(store, &key);
                self.hold(audio);
            }
            self.held_data.clone()
        } else {
            self.read(store, &key)
        };
        let Some(data) = data else { return };
        let (until, skin, recent) = {
            let a = &self.actors[&preview];
            (a.until, a.skin.clone(), a.recent)
        };
        if now < until || (!always && self.speakers.iter().any(|u| *u > now)) {
            return;
        }
        let Some(c) = data.chars.get(&key) else { return };
        let skin = skin_vo_data(c, c, Some(skin.as_str()).filter(|s| !s.is_empty()));
        let rows = skin.rows.unwrap_or(&[]);
        let tags: Vec<&str> = tags.split('|').filter(|t| !t.is_empty()).collect();
        let pick = select_vo(rows, alias, &tags, mvs_sim::math::narrow(f64::from(recent)), |n| {
            mvs_sim::math::narrow(self.random(f64::from(n)))
        });
        let Some(ev) =
            usize::try_from(pick).ok().and_then(|i| rows.get(i)).and_then(|r| r.3.clone()).filter(|e| !e.is_empty())
        else {
            return;
        };
        let a = self.actors.entry(preview).or_default();
        a.recent = pick;
        if let Some(u) = self.post(audio, now, &ev) {
            self.actors.entry(preview).or_default().until = u;
        }
    }

    fn announce(&mut self, audio: &mut Option<Audio>, now: f64, line: AnnouncerLine) {
        let alias = format!(
            "{ANNOUNCER_PREFIX}{}",
            match line {
                AnnouncerLine::Intro => "intro",
                AnnouncerLine::Outro => "outro",
            }
        );
        let Some(ev) = self.announcer_rows().into_iter().find(|r| r.0 == alias).map(|r| r.1).filter(|e| !e.is_empty())
        else {
            return;
        };
        if now < self.announcer_until {
            return;
        }
        if let Some(u) = self.post(audio, now, &ev) {
            self.announcer_until = u;
        }
    }

    /// The screen's end: the lines let go.
    pub fn end(&mut self, audio: &mut Option<Audio>) {
        self.over = true;
        self.held_data = None;
        self.hold(audio);
    }
}
