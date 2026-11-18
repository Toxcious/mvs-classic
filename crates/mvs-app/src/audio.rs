//! The match's sound:
//! the scopes a match holds, the sim's sound events posted on their fighters' game objects, the
//! emitters and the listener placed each frame, the HUD's posts, the music's flow, the stage's ambience and the
//! scenery's sounds ([`SceneSound`]).
//!
//! The glue makes the sound engine's calls by name ([`Line`]: the post tape's record, so its check compares them
//! with the oracle's tape) and the loop hands them to [`mvs_audio::Audio::call`]; it needs no sound engine to run.
//!
//! Game objects: fighters by their id, minions from 100 (their own ids), the game state -1, the pooled components of
//! the hit sounds 1000 + n, the ships 2000 + the owner, the scenery's objects 3000 + n, the components the sim spawns
//! for one sound 4000 + n.

mod jake;
pub mod posted;
pub(crate) mod scopes;

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mvs_audio::RtpcMirror;
use mvs_audio::tape::post::{Call, FlowArg, Line, ReverbArg};
use mvs_render::Renderer;
use mvs_render::scenery::SoundSink;
use mvs_sim::events::{DisguiseFxKind, EndPhase, PopKind, SoundKey};
use mvs_sim::fighter::common::latent_frames;
use mvs_sim::fighter::match_side::{applied_skin_audio, swap_skin_event};
use mvs_sim::world::blast::ring_out_asset_name;
use mvs_sim::{Event, World};
use mvs_view::camera::Pov;

pub use scopes::{Index, Lists, base_events};

use crate::lab_setup::LabMusic;
use crate::{Error, Result};

type V3 = [f64; 3];

/// The stage's ambience: its voices' key, which a Lab reset's stop-all leaves.
const STAGE_AMBIENCE: &str = "stageAmbience";

/// The Audio screen's music volume (apply.rs `VOLUME_RTPCS`): the music bus's parent's volume, 0 silent, 1 at 0 dB.
const MUSIC_SLIDER: &str = "menu_slider_mus";

/// The pause menu's sound mix let go: SetState state_pause_off, Resume All, UnMute All.
const PAUSE_MIX_OFF: &str = "mix_pause_off";

/// (the port's own) A Lab map load's music fade out under the cover, and back in at the next level's start, ms.
const COVER_MUSIC_OUT_MS: f64 = 500.0;
const COVER_MUSIC_IN_MS: f64 = 1000.0;
/// The pooled components' game objects (PandaGameState: 11 made at BeginPlay): `AK_POOL` + index.
const AK_POOL: i32 = 1000;
const AK_POOL_SIZE: usize = 11;
/// The ships' game objects (one per owner).
const AK_SHIP: i32 = 2000;
/// The scenery's own game objects, numbered by the key they were first placed under.
const AK_SCENERY: i32 = 3000;
/// The components a sim event spawns for one post (SpawnAkComponentAtLocation, AutoDestroy): `AK_SPAWNED` + n, n
/// going round `AK_SPAWNED_COUNT`.
// Unverified: a spawned component is destroyed when its post ends; here its game object is used again 1000 spawns
// later, long after any of these sounds ends.
const AK_SPAWNED: i32 = 4000;
const AK_SPAWNED_COUNT: usize = 1000;
const SFX_STOP_ALL_CHARACTER: &str = "mix_stop_all_character_sfx";
/// PHYSMAT: the surface of a hit's victim (0 and 6 are flesh, 7 up sets nothing).
const PHYSMAT: [&str; 7] = ["flesh", "rock", "wood_thin", "wood_thick", "metal_solid", "metal_grate", "flesh"];
const STEP: f64 = 1.0 / 60.0;

/// A call the scenery made on its sound sink, kept until the frame's calls.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneCall {
    PostAt { event: String, at: [f64; 3], key: Option<String> },
    PostOn { event: String, key: String, at: [f64; 3] },
    Place { key: String, at: [f64; 3] },
}

/// The scenery sink's queue, shared by the sink and the glue (a glue made on a loader thread is Send).
pub type SceneQueue = Arc<Mutex<Vec<SceneCall>>>;

fn queue(q: &SceneQueue) -> MutexGuard<'_, Vec<SceneCall>> {
    q.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The scenery's sound sink: its calls queued for [`MatchAudio::frame`], its game parameters read from the
/// mixer's mirror (None without a sound engine).
pub struct SceneSound {
    pub calls: SceneQueue,
    pub rtpc: Option<Arc<RtpcMirror>>,
}

impl SoundSink for SceneSound {
    fn post_at(&mut self, event: &str, at: [f64; 3], key: Option<&str>) {
        queue(&self.calls).push(SceneCall::PostAt { event: event.into(), at, key: key.map(Into::into) });
    }
    fn post_on(&mut self, event: &str, key: &str, at: [f64; 3]) {
        queue(&self.calls).push(SceneCall::PostOn { event: event.into(), key: key.into(), at });
    }
    fn place(&mut self, key: &str, at: [f64; 3]) {
        queue(&self.calls).push(SceneCall::Place { key: key.into(), at });
    }
    fn rtpc(&self, name: &str) -> Option<f64> {
        self.rtpc.as_ref()?.get_set(mvs_data::sound::short_id(name))
    }
}

/// What the glue reads of the drawn pawns (the last pose) and of the effects' picks: the pawn records.
pub trait Views {
    /// FindClosestBone_K2 on the victim's mesh: the bone location nearest to `hit` (UE place); `hit` itself for a
    /// victim with no mesh.
    fn nearest_bone(&self, victim: i32, hit: V3) -> V3;
    /// The ring-out entry the effects picked for the KO the event names: its ally and enemy sound events (an empty
    /// one is the default pair's).
    fn ko_sound(&mut self, world: &World, asset: &str) -> Option<(String, String)>;
    /// `mutedByIntro`: a sound event of a pawn's default intro that its skin's own intro
    /// replaces (not played). None without drawn pawns.
    fn muted_by_intro(&self, _world: &World, _e: &Event) -> bool {
        false
    }
    /// The effects' own sounds since the last frame (an event at a UE place, posted
    /// on the game state), taken. None without drawn effects.
    fn take_sounds(&mut self) -> Vec<(String, V3)> {
        Vec::new()
    }
    /// GetSocketRotation: the world rows (UE axes, row vectors) of a socket or bone of pawn `id`'s mesh as last drawn.
    /// None without drawn pawns.
    fn socket_rows(&self, _world: &World, _id: i32, _socket: &str) -> Option<[f64; 9]> {
        None
    }
}

/// No drawn pawns: a bone is the hit's own place and the ring-out entry the asset's first.
// Unverified: the first entry stands for the pick of an asset with several (the pick of the build is random).
pub struct NoViews;

impl Views for NoViews {
    fn nearest_bone(&self, _victim: i32, hit: V3) -> V3 {
        hit
    }
    fn ko_sound(&mut self, world: &World, asset: &str) -> Option<(String, String)> {
        let t = &world.data.ring_outs;
        let a = t
            .assets
            .get(asset)
            .filter(|a| !a.entries.is_empty())
            .or_else(|| t.assets.get(mvs_sim::world::blast::DEFAULT_RING_OUT))?;
        let e = a.entries.first()?;
        Some((e.ally.clone(), e.enemy.clone()))
    }
}

impl Views for mvs_render::fx::Fx {
    fn nearest_bone(&self, victim: i32, hit: V3) -> V3 {
        mvs_render::fx::Fx::nearest_bone(self, victim, hit)
    }
    fn ko_sound(&mut self, _world: &World, _asset: &str) -> Option<(String, String)> {
        (!self.ko_picks.is_empty()).then(|| self.ko_picks.remove(0))
    }
    fn muted_by_intro(&self, world: &World, e: &Event) -> bool {
        mvs_render::fx::Fx::muted_by_intro(self, world, e)
    }
    /// `Fx::sounds`, taken (the elemental buffs' host.soundAt).
    fn take_sounds(&mut self) -> Vec<(String, V3)> {
        std::mem::take(&mut self.sounds)
    }
    fn socket_rows(&self, world: &World, id: i32, socket: &str) -> Option<[f64; 9]> {
        mvs_render::fx::Fx::socket_rows(self, world, id, socket)
    }
}

/// The calls of one step or frame, on one tick.
struct Calls<'a> {
    tick: u32,
    out: &'a mut Vec<Line>,
}

impl Calls<'_> {
    fn push(&mut self, call: Call) {
        self.out.push(Line { tick: self.tick, call });
    }
    fn post(&mut self, ev: &str, owner: Option<i32>, key: Option<String>, stop_key: Option<String>, at: Option<V3>) {
        self.push(Call::Post { ev: ev.to_owned(), owner, key, stop_key, at });
    }
    /// post(ev, { owner })
    fn post_on(&mut self, ev: &str, owner: i32) {
        self.post(ev, Some(owner), None, None, None);
    }
    fn rtpc(&mut self, name: &str, value: f64, owner: i32, interp_ms: f64) {
        self.push(Call::SetRtpc { name: name.to_owned(), value, owner, interp_ms });
    }
    fn switch(&mut self, group: &str, value: &str, owner: i32) {
        self.push(Call::SetSwitch { group: group.to_owned(), value: value.to_owned(), owner });
    }
    fn state(&mut self, group: &str, value: &str) {
        self.push(Call::SetState { group: group.to_owned(), value: value.to_owned() });
    }
    fn emitter(&mut self, owner: i32, pos: V3, front: V3) {
        self.push(Call::SetEmitter { owner, pos, front });
    }
    fn scope(&mut self, name: &str, keep: Vec<String>, lazy: Vec<String>) {
        self.push(Call::Scope { name: name.to_owned(), keep, lazy });
    }
}

/// The music's state: `t` is the match's music clock, -1 before it starts.
#[derive(Clone, Debug)]
struct Music {
    t: f64,
    state: String,
    timer: f64,
    match_point: bool,
    skin: String,
}

/// The match's sound glue. Each method appends its calls to `out`; `tick` is the World's flow tick.
pub struct MatchAudio {
    /// The scenery sink's queue.
    pub(crate) scene: SceneQueue,
    known: Index,
    lab: bool,
    /// The level goes on from a Lab restart: its music, ambience and reverb are not begun again.
    kept: bool,
    /// The player this machine controls in a net match; None offline (every pawn is local).
    pub net_me: Option<i32>,
    /// The local player's fighter (a Lab carry's local slot, else 0; the net seat online).
    me: usize,
    /// Whether the match is a net match.
    online: bool,
    /// The events the open scenery's blocks name and post (`BeginOpts::scenery_events`), for the stage scope.
    scenery_events: Vec<String>,
    pool: usize,
    /// By player index: the default skin's audio name and the applied skin's (a swap of an event's name).
    swaps: Vec<Option<(String, String)>>,
    /// The events of the applied skins' override classes (loaded after Comp_Pawn_SkinManager's Delay(1.0)).
    skin_loaded: HashSet<String>,
    skin_load_frames: f64,
    music: Music,
    /// The default announcer pack's event (lower case) to its alias (lower case).
    aliases: HashMap<String, String>,
    /// This machine's announcer pack, read at the match's start (PandaGameState ReceiveBeginPlay's
    /// LoadStoredAnnouncerPackAssetData @20356).
    announcer: mvs_data::AnnouncerPack,
    scenery_objects: Vec<String>,
    /// The last spawned component's index (`AK_SPAWNED` + it).
    spawned: usize,
    /// Jake's stretches' head pitch (from the drawn pose).
    head_pitch: jake::HeadPitch,
    /// `BeginOpts::podium`.
    podium: Vec<(String, PodiumMontages)>,
}

/// What a match's sound begins with besides its World.
#[derive(Clone, Debug, Default)]
pub struct BeginOpts {
    /// The music flow of the match before a Lab restart that keeps the level: the level's music, ambience and
    /// reverb volumes are not begun again (the kept level's carried music).
    pub kept: Option<LabMusic>,
    /// The level's music skin, given (a tape's: the build draws it at random); None: drawn.
    pub music_skin: Option<String>,
    /// The player this machine controls in a net match (its seat in a net match); None offline.
    pub net_me: Option<i32>,
    /// The events the open scenery's blocks name in their data and post by name, as given
    /// (`Renderer::scenery_sound_events`), for the stage scope.
    pub scenery_events: Vec<String>,
    /// The local player's fighter offline (a Lab carry's local slot, else 0); online the seat
    /// `net_me` names is used.
    pub me: usize,
    /// This machine's announcer pack (`mvs_data::stored_announcer_pack`); None: the match data's default pack.
    pub announcer: Option<mvs_data::AnnouncerPack>,
    /// By roster key, the podium victory and defeat montages (`podium_montages`), for the podium scope of a match
    /// that is not the Lab's; None: read from the renderer's store at such a match's begin (empty: the scope holds
    /// the wind bed alone).
    pub podium: Option<Vec<(String, PodiumMontages)>>,
}

/// A fighter's podium victory and defeat montages: the default skin's, and those of each skin with an entry of its
/// own (VictoryMontageOverride / DefeatMontageOverride, BP_CharacterPreview @32102-33303).
#[derive(Clone, Debug, Default)]
pub struct PodiumMontages {
    pub default: [String; 2],
    pub skins: Vec<(String, [String; 2])>,
}

/// The podium's victory and defeat montages by roster key (the menuscene kind's Podium entry
/// fighters list); none when the store has no such entry (said once).
pub fn podium_montages(store: &mvs_store::Store) -> Vec<(String, PodiumMontages)> {
    use mvs_render::menuscene::format::{LevelName, SceneData, load_level};
    let pair = |f: &mvs_render::menuscene::format::PodiumFighter| [f.victory.name.clone(), f.defeat.name.clone()];
    match load_level(store, LevelName::Podium).map(|e| e.data) {
        Ok(SceneData::Podium(p)) => p
            .fighters
            .iter()
            .map(|(k, f)| {
                let skins = f.skins.iter().flatten().map(|(s, o)| (s.clone(), pair(o))).collect();
                (k.clone(), PodiumMontages { default: pair(&f.default), skins })
            })
            .collect(),
        Ok(_) => Vec::new(),
        Err(e) => {
            eprintln!("mvs: audio: the podium's montages: {e}");
            Vec::new()
        }
    }
}

impl MatchAudio {
    /// The match begins: its scopes, the stage's ambience and reverb volumes, the music, the scenery's sink (its
    /// game parameters from `rtpc`). `init` is the audio's InitBank (read once, when the audio opened).
    /// `opts.scenery_events` is the renderer's open scenery's.
    pub fn begin(
        world: &World,
        renderer: &mut Renderer,
        init: &mvs_data::sound::InitBank,
        rtpc: Option<Arc<RtpcMirror>>,
        opts: BeginOpts,
        out: &mut Vec<Line>,
    ) -> Result<MatchAudio> {
        Self::begin_known(world, renderer, Index::new(init.events.keys().cloned()), rtpc, opts, out)
    }

    /// [`MatchAudio::begin`] with the InitBank's event index given (one a loader thread made).
    pub(crate) fn begin_known(
        world: &World,
        renderer: &mut Renderer,
        known: Index,
        rtpc: Option<Arc<RtpcMirror>>,
        mut opts: BeginOpts,
        out: &mut Vec<Line>,
    ) -> Result<MatchAudio> {
        opts.scenery_events = renderer.scenery_sound_events();
        if opts.podium.is_none() && world.setup.flow {
            opts.podium = Some(podium_montages(renderer.assets().store()));
        }
        let m = MatchAudio::with_opts(world, known, out, opts);
        m.attach(renderer, rtpc);
        Ok(m)
    }

    /// The scenery's sink on the renderer, its queue this glue's (the end of [`MatchAudio::begin`]).
    pub(crate) fn attach(&self, renderer: &mut Renderer, rtpc: Option<Arc<RtpcMirror>>) {
        renderer.set_scenery_sound(Box::new(SceneSound { calls: self.scene.clone(), rtpc }));
    }

    /// The events of the scenery it was begun with (`BeginOpts::scenery_events`).
    pub(crate) fn scenery_events(&self) -> &[String] {
        &self.scenery_events
    }

    /// The music flow of the match before a Lab restart that keeps the level, on a glue begun with another
    /// (`BeginOpts::kept`: the begin's calls do not read it).
    pub(crate) fn resume_music(&mut self, k: &LabMusic) {
        let k = k.clone();
        self.music = Music { t: k.t, state: k.state, timer: k.timer, match_point: k.match_point, skin: k.skin };
    }

    /// [`MatchAudio::begin`] with the event index given (the build's InitBank events) and no scenery sink.
    pub fn with_index(world: &World, known: Index, out: &mut Vec<Line>) -> MatchAudio {
        Self::with_opts(world, known, out, BeginOpts::default())
    }

    /// [`MatchAudio::with_index`] on a Lab level that goes on (`kept`: the music flow of the match before a Lab restart
    /// that keeps the level).
    pub fn with_index_kept(world: &World, known: Index, out: &mut Vec<Line>, kept: Option<&LabMusic>) -> MatchAudio {
        Self::with_opts(world, known, out, BeginOpts { kept: kept.cloned(), ..BeginOpts::default() })
    }

    /// [`MatchAudio::with_index`] with the begin's options.
    pub fn with_opts(world: &World, known: Index, out: &mut Vec<Line>, opts: BeginOpts) -> MatchAudio {
        let BeginOpts { kept, music_skin, net_me, scenery_events, me, announcer, podium } = opts;
        let lab = !world.setup.flow;
        let skin = music_skin.unwrap_or_else(|| Self::draw_music_skin(world));
        let mut m = MatchAudio {
            kept: kept.is_some(),
            scene: SceneQueue::default(),
            known,
            lab,
            net_me,
            me: net_me.map_or(me, |n| usize::try_from(n).unwrap_or(0)),
            online: net_me.is_some(),
            scenery_events,
            pool: 0,
            swaps: Vec::new(),
            skin_loaded: HashSet::new(),
            skin_load_frames: f64::from(latent_frames(1.0)),
            music: Music { t: -1.0, state: "none".into(), timer: -1.0, match_point: false, skin },
            aliases: scopes::announcer_aliases(&world.data.announcer.rows),
            announcer: announcer.unwrap_or_else(|| world.data.announcer.clone()),
            scenery_objects: Vec::new(),
            spawned: AK_SPAWNED_COUNT - 1,
            head_pitch: jake::HeadPitch::default(),
            podium: podium.unwrap_or_default(),
        };
        if let Some(k) = kept {
            m.music = Music { t: k.t, state: k.state, timer: k.timer, match_point: k.match_point, skin: k.skin };
        }
        m.bind_skins(world);
        let tick = u32::try_from(world.flow.tick).unwrap_or(0);
        let mut c = Calls { tick, out };
        m.begin_calls(world, &mut c);
        m
    }

    /// The level's music skin: one of the stage's drawn at the match's start (the build draws it at the Audio
    /// sub-level's BeginPlay).
    // Deviation: the draw is from the clock, where the build's is random: the World's stream is not touched.
    fn draw_music_skin(world: &World) -> String {
        let skins: Vec<&str> = world
            .data
            .stage
            .audio
            .as_ref()
            .map_or_else(|| vec!["mus_base"], |a| a.music.skins.iter().map(String::as_str).collect());
        let n =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
        skins.get(n % skins.len().max(1)).copied().unwrap_or("mus_base").to_owned()
    }

    /// The pawn records' skin swaps: each player's applied skin's audio name.
    fn bind_skins(&mut self, world: &World) {
        for (i, p) in world.setup.players.iter().enumerate() {
            let key = p.character.as_deref().unwrap_or(&world.data.default_character);
            let swap = world.data.chars.get(key).and_then(|cd| {
                let t = cd.skin_audio.as_ref()?;
                let s = applied_skin_audio(Some(world), cd, cd, p.skin.as_deref())?;
                self.skin_loaded.extend(s.events.iter().cloned());
                Some((t.default.clone(), s.audio_name.clone()))
            });
            if self.swaps.len() <= i {
                self.swaps.resize(i + 1, None);
            }
            self.swaps[i] = swap;
        }
    }

    /// What the match's start calls, in this order (the stage scene's reverb volumes, the stage music scope,
    /// scopeStageAudio, scopeMatchAudio, scopeRingOutAudio, scopeTutorialAudio, the music skin, scopeItemsAudio, the
    /// level's ambience, the transition, the first input's resume).
    fn begin_calls(&mut self, world: &World, c: &mut Calls) {
        let audio = world.data.stage.audio.as_ref();
        if self.kept {
            self.begin_match_calls(world, c);
            return;
        }
        c.push(Call::SetReverbVolumes {
            list: audio
                .map(|a| {
                    a.reverb
                        .iter()
                        .map(|r| ReverbArg {
                            bus: r.bus.clone(),
                            loc: r.loc.clone(),
                            scale: r.scale.clone(),
                            level: None,
                            rate: None,
                            priority: None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        });
        // The music: the level's map and skin and the states its flow takes.
        let map = audio.map_or("mus_map_lionwatch", |a| a.music.map.as_str()).to_owned();
        let mut values = vec![map, self.music.skin.clone()];
        values.extend(["mus_intro", "mus_main_a", "mus_main_b", "mus_nocombat"].map(String::from));
        // TutorialGameMode Start Tutorial (5065)'s switch: mus to mus_tutorial (modes/tutorial/run.rs start).
        // Deviation: the stage music's scope leaves it out, so a tutorial's music had nothing decoded; it is added
        // here.
        if matches!(world.setup.mode, Some(mvs_sim::setup::MatchMode::Tutorial { .. })) {
            values.push("mus_tutorial".into());
        }
        if !self.lab {
            values.push("mus_victory".into());
        }
        // (the Lab's states follow one another in a fixed order: only the playing one and the next stay decoded)
        let flow = self.lab.then(|| FlowArg {
            group: "mus_map_state".into(),
            start: "mus_intro".into(),
            next: [
                ("mus_intro", vec!["mus_main_a"]),
                ("mus_main_a", vec!["mus_nocombat"]),
                ("mus_nocombat", vec!["mus_main_b"]),
                ("mus_main_b", vec!["mus_nocombat"]),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.into_iter().map(String::from).collect()))
            .collect(),
        });
        c.push(Call::ScopeMusic { ev: "mus_play".into(), values, flow });
        self.begin_match_calls(world, c);
        let skin = self.music.skin.clone();
        c.switch("mus_skin", &skin, -1);
        c.scope("items", self.item_events(world), Vec::new());
        // The Env_Audio_Emit actors post their events on their own AkComponents at BeginPlay unless IsManualStart.
        c.push(Call::StopKey { key: STAGE_AMBIENCE.into(), fade: 0.0, curve: None });
        for e in audio.into_iter().flat_map(|a| &a.emitters) {
            // (an emitter whose event the build does not ship, Throne Room's eleven (ledger "Throne Room audio"), posts
            // a null event: nothing, and no warning)
            if let Some(ev) =
                e.event.as_deref().filter(|ev| e.manual != Some(true) && self.known.has(&ev.to_lowercase()))
            {
                let at = [e.at[0], e.at[1], e.at[2]];
                c.post(ev, Some(-1), Some(STAGE_AMBIENCE.into()), None, Some(at));
            }
        }
        // The menu's fade to black left the transiiton parameter at 0: the opened level brings it back over 3 s.
        c.rtpc("transiiton", 1.0, -1, 3000.0);
        // The first input: the audio resumes, the music clock starts.
        c.push(Call::Resume);
        self.music.t = 0.0;
    }

    /// The scopes a match's fighters and the stage bring (the level's music and ambience are `begin_calls`').
    fn begin_match_calls(&mut self, world: &World, c: &mut Calls) {
        let stage = self.stage_lists(world);
        c.scope("stage", stage.keep, stage.lazy);
        let (m, podium) = self.match_lists(world);
        c.scope("match", m.keep, m.lazy);
        c.scope("podium", podium, Vec::new());
        c.scope("ringout", self.ring_out_events(world), Vec::new());
        let tutorial = self.tutorial_lines(world);
        let steps = self.tutorial_sounds(world);
        c.scope("tutorial", steps, tutorial);
        if self.kept {
            let items = self.item_events(world);
            c.scope("items", items, Vec::new());
        }
    }

    /// The music flow, for a Lab restart that keeps the level.
    pub fn music_flow(&self) -> LabMusic {
        let m = &self.music;
        LabMusic { t: m.t, state: m.state.clone(), timer: m.timer, match_point: m.match_point, skin: m.skin.clone() }
    }

    /// (the port's own) A Lab map load's cover came up: the level's music fades out under it (its bus's volume
    /// parameter, the Audio screen's music slider, to 0) and its ambience with it, as its level is unloaded.
    pub fn map_cover(&mut self, tick: i32, out: &mut Vec<Line>) {
        let mut c = Calls { tick: u32::try_from(tick).unwrap_or(0), out };
        c.rtpc(MUSIC_SLIDER, 0.0, -1, COVER_MUSIC_OUT_MS);
        c.push(Call::StopKey { key: STAGE_AMBIENCE.into(), fade: COVER_MUSIC_OUT_MS / 1000.0, curve: None });
    }

    /// (the port's own) The match after a Lab map load begins: the music fades back in to the Audio screen's
    /// `volume` (the music slider's value) as the new level's music starts under the cover.
    pub fn map_cover_end(tick: i32, volume: f64, out: &mut Vec<Line>) {
        let mut c = Calls { tick: u32::try_from(tick).unwrap_or(0), out };
        c.rtpc(MUSIC_SLIDER, volume, -1, COVER_MUSIC_IN_MS);
    }

    /// A match or the Lab left with its pause menu up
    /// (UI_LAB_PauseMenu Destruct @2684): the pause mix goes, or its Mute of the SFX bus
    /// and its Pause All outlive the match and every effect of the session after it is silent while the music, left
    /// out of both, plays on.
    pub fn left_paused(tick: i32, out: &mut Vec<Line>) {
        let mut c = Calls { tick: u32::try_from(tick).unwrap_or(0), out };
        c.post_on(PAUSE_MIX_OFF, -1);
    }

    /// labReset's sound: every voice but the level's ambience stopped (audio.stopAll(0.1, STAGE_AMBIENCE)).
    pub fn lab_reset(&mut self, tick: i32, out: &mut Vec<Line>) {
        let mut c = Calls { tick: u32::try_from(tick).unwrap_or(0), out };
        c.push(Call::StopAll { fade: 0.1, keep: Some(STAGE_AMBIENCE.into()) });
    }

    /// The characters of the match: each player's key, or in a mode's match its cast alone
    /// (the tutorial's characters' skins are none).
    fn characters(world: &World) -> Vec<&str> {
        if let Some(mvs_sim::setup::MatchMode::Tutorial { cast, .. }) = &world.setup.mode {
            return cast.iter().map(String::as_str).collect();
        }
        world.setup.players.iter().map(|p| p.character.as_deref().unwrap_or(&world.data.default_character)).collect()
    }

    /// scopeMatchAudio: per fighter of the match, the events its data names and its set carries, its applied skin's,
    /// and, for a match that is not the Lab's, those of its reveal, its end and its podium; the podium scope's.
    fn match_lists(&self, world: &World) -> (Lists, Vec<String>) {
        let chars = Self::characters(world);
        let mut sets: Vec<Lists> = Vec::new();
        let mut done = HashSet::new();
        for k in &chars {
            if done.insert(*k)
                && let Some(cd) = world.data.chars.get(*k)
            {
                sets.push(scopes::char_events(
                    k,
                    &world.data.default_character,
                    &cd.fighter,
                    cd.skin_audio.as_ref(),
                    &self.known,
                    posted::POSTED,
                    mvs_render::fx::chars::render_sounds(k),
                ));
            }
        }
        for (i, k) in chars.iter().enumerate() {
            let Some(cd) = world.data.chars.get(*k) else { continue };
            let Some(t) = cd.skin_audio.as_ref() else { continue };
            // (a mode's cast has no skins)
            let skin = if world.setup.mode.is_some() {
                None
            } else {
                world.setup.players.get(i).and_then(|p| p.skin.as_deref())
            };
            let Some(s) = applied_skin_audio(Some(world), cd, cd, skin) else { continue };
            sets.push(scopes::skin_events(t, s, &self.known));
        }
        sets.push(Lists { keep: self.lent_events(world, &chars), lazy: Vec::new() });
        // Deviation: the match scope reads no perk tables; these are the events not already in it.
        let had: HashSet<&String> = sets.iter().flat_map(|s| s.keep.iter().chain(&s.lazy)).collect();
        let perk: Vec<String> = self.perk_events(world, &chars).into_iter().filter(|e| !had.contains(e)).collect();
        sets.push(Lists { keep: perk, lazy: Vec::new() });
        let podium = !self.lab;
        let mut keep: Vec<String> = sets.iter().flat_map(|s| s.keep.iter().cloned()).collect();
        let lazy = sets.iter().flat_map(|s| s.lazy.iter().cloned()).collect();
        if podium {
            keep.extend(scopes::match_ui(&self.known));
            keep.extend(self.announcer_lines());
        }
        let mut wind: Vec<String> = podium.then(|| "amb_map_shared_bed_wind_cliff".to_owned()).into_iter().collect();
        if podium {
            wind.extend(self.podium_events(world));
        }
        (Lists { keep, lazy }, wind)
    }

    /// The one-shots of the fighters' podium victory and defeat montages, posted at the podium's preview actors
    /// per character once, in the players' order, each
    /// montage's Ak_OneShot and Ak_VO_Effort events (no notify state), each once in it, lower case: the default
    /// skin's montages (the default list), then the events not held yet of the players' skins' own montages (the
    /// build plays the skin's VictoryMontageOverride / DefeatMontageOverride, BP_CharacterPreview @32102-33303, and
    /// the podium posts from them), each montage's notifies the skin's own entry's, else the character's.
    // Deviation: not the default skin's montages alone: the players' skins' montages are held.
    fn podium_events(&self, world: &World) -> Vec<String> {
        let chars = Self::characters(world);
        // (a mode's cast has no skins)
        let skin_of = |i: usize| {
            if world.setup.mode.is_some() {
                None
            } else {
                world.setup.players.get(i).and_then(|p| p.skin.as_deref()).filter(|s| !s.is_empty())
            }
        };
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let push = |cd: &mvs_data::CharData, skin: Option<&str>, m: &str, out: &mut Vec<String>| {
            let own = skin.and_then(|s| cd.fighter.skins.as_ref()?.get(s)?.montages.as_ref()?.get(m));
            let Some(mon) = own.or_else(|| cd.fighter.montages.get(m)) else { return };
            let from = out.len();
            for n in &mon.notifies {
                if n.state || !["Ak_OneShot_C", "Ak_VO_Effort_C"].contains(&n.cls.as_str()) {
                    continue;
                }
                let Some(ev) = n.p.get("AkEvent").and_then(mvs_data::Value::as_str).filter(|e| !e.is_empty()) else {
                    continue;
                };
                let ev = ev.to_lowercase();
                // (once per montage; a skin's montage adds only what is not held yet)
                if !out[from..].contains(&ev) && (skin.is_none() || !out.contains(&ev)) {
                    out.push(ev);
                }
            }
        };
        for (i, c) in chars.iter().enumerate() {
            let Some((_, pm)) = self.podium.iter().find(|(k, _)| k == c) else { continue };
            let Some(cd) = world.data.chars.get(*c) else { continue };
            if seen.insert(*c) {
                for m in &pm.default {
                    push(cd, None, m, &mut out);
                }
            }
            let Some(skin) = skin_of(i) else { continue };
            let Some((_, pair)) = pm.skins.iter().find(|(s, _)| s == skin) else { continue };
            for m in pair {
                push(cd, Some(skin), m, &mut out);
            }
        }
        out
    }

    /// A character needed beside the players' (MATCH_NEEDS) lends the events of its montages that the fighters' buffs
    /// name, which play on the mesh a buff makes of it (Buff_C020Polymorph's Creature taunt and turn).
    fn lent_events(&self, world: &World, chars: &[&str]) -> Vec<String> {
        let needs: Vec<&String> = world
            .data
            .chars
            .keys()
            .filter(|n| *n != &world.data.default_character && !chars.contains(&n.as_str()))
            .collect();
        if needs.is_empty() {
            return Vec::new();
        }
        let mut seen = HashSet::new();
        let buff_text: Vec<String> = chars
            .iter()
            .filter(|c| seen.insert(**c))
            .map(|c| {
                world
                    .data
                    .chars
                    .get(*c)
                    .and_then(|cd| cd.fighter.buffs.as_ref())
                    .map_or_else(|| "{}".to_owned(), |b| serde_json::to_string(b).unwrap_or_default())
            })
            .collect();
        let mut out = Vec::new();
        for n in needs {
            let Some(cd) = world.data.chars.get(n) else { continue };
            let named: Vec<&mvs_data::fighter::Montage> = cd
                .fighter
                .montages
                .iter()
                .filter(|(k, _)| buff_text.iter().any(|t| t.contains(&format!("\"{k}\""))))
                .map(|(_, m)| m)
                .collect();
            out.extend(scopes::events_named(&serde_json::to_string(&named).unwrap_or_default(), &self.known));
        }
        out
    }

    /// The events of the buffs the players' perks give, found in the perk tables the sim reads (`perk_buff_def`:
    /// the general table, then the match's characters' parts), and of every buff class those defs name in turn
    /// (Perk_BugsBunny_LingeringLove's buff gives Buff_ScoobySnack through its OnHitBuffs).
    fn perk_events(&self, world: &World, chars: &[&str]) -> Vec<String> {
        let tables: Vec<&mvs_data::tables::PerkTable> = std::iter::once(&world.data.perks)
            .chain(chars.iter().filter_map(|c| world.data.chars.get(*c)?.perks.as_ref()))
            .collect();
        let mut todo: Vec<&str> = Vec::new();
        // (a mode's match loads no player's fighter: its players' perks are not scoped)
        let players = if world.setup.mode.is_some() { &[][..] } else { &world.setup.players[..] };
        for p in players {
            for id in &p.perks {
                if let Some(e) = tables.iter().find_map(|t| t.perks.get(id)) {
                    todo.extend(e.buffs.iter().map(String::as_str));
                }
            }
        }
        let mut seen: HashSet<&str> = HashSet::new();
        let mut out = Vec::new();
        while let Some(cls) = todo.pop() {
            if !seen.insert(cls) {
                continue;
            }
            let Some(def) = tables.iter().find_map(|t| t.buffs.get(cls)) else { continue };
            let text = serde_json::to_string(def).unwrap_or_default();
            out.extend(scopes::events_named(&text, &self.known));
            for t in &tables {
                todo.extend(t.buffs.keys().map(String::as_str).filter(|k| !seen.contains(k) && text.contains(*k)));
            }
        }
        let mut dup = HashSet::new();
        out.retain(|e| dup.insert(e.clone()));
        out
    }

    /// scopeTutorialAudio: a tutorial's announcer lines (the tutorial actors' BPI_PlayVO, TUTORIAL_ANNOUNCER): this
    /// machine's pack's rows of the aliases the actors name (every actor class's introVO and step VO; only
    /// DefaultAnnouncerPack has them), held as files and decoded when posted, as a fighter's VO; any other match
    /// lets them go.
    fn tutorial_lines(&self, world: &World) -> Vec<String> {
        if !matches!(world.setup.mode, Some(mvs_sim::setup::MatchMode::Tutorial { .. })) {
            return Vec::new();
        }
        let named: HashSet<String> = world
            .data
            .tutorials
            .values()
            .flat_map(|a| a.steps.iter().map(|s| s.vo.as_ref()).chain([a.intro_vo.as_ref()]))
            .flatten()
            .map(|a| a.to_lowercase())
            .collect();
        self.announcer
            .rows
            .iter()
            .filter(|r| {
                let alias = r.0.to_lowercase();
                r.1.is_empty() && alias.starts_with("vo_announcer_tutorial_") && named.contains(&alias)
            })
            .map(|r| r.3.to_lowercase())
            .filter(|e| self.known.has(e))
            .collect()
    }

    /// The step machine's sounds (modes/tutorial/run.rs AudioStep's ui_tutorial_step_complete_0N), held in a tutorial
    /// only: `scopes::TUTORIAL_SOUNDS`.
    // Deviation: the names are built by concatenation, so no scope held them and its step sounds were never
    // loaded; the build plays them.
    fn tutorial_sounds(&self, world: &World) -> Vec<String> {
        if !matches!(world.setup.mode, Some(mvs_sim::setup::MatchMode::Tutorial { .. })) {
            return Vec::new();
        }
        self.known.all().iter().filter(|e| scopes::is_tutorial_sound(e)).cloned().collect()
    }

    /// TutorialActor ControlTutorialPawn @2096 / @2277 and UncontrolTutorialPawn @2255 / @2436: SetRTPCValue
    /// (tutorial_controlled_pawn, 1 / 0) on the spawned pawn `pawn`.
    pub fn tutorial_rtpc(&mut self, tick: i32, pawn: i32, value: f64, out: &mut Vec<Line>) {
        let mut c = Calls { tick: u32::try_from(tick).unwrap_or(0), out };
        c.rtpc("tutorial_controlled_pawn", value, pawn, 0.0);
    }

    /// This machine's pack's events of a match that is not the Lab's, less the pre-match screen's two and the
    /// tutorial's.
    fn announcer_lines(&self) -> Vec<String> {
        self.announcer
            .rows
            .iter()
            .filter(|r| {
                !r.0.to_lowercase().starts_with("announcer_characterselect_")
                    && !r.0.to_lowercase().starts_with("vo_announcer_tutorial_")
            })
            .map(|r| r.3.to_lowercase())
            .filter(|e| self.known.has(e))
            .collect()
    }

    /// scopeStageAudio: the events the stage's data names, those its scenery blocks name and post by name
    /// (Scenery.named / sounds), those its placed actors' classes post by name (StageActorKind.sounds) and those of
    /// its map's name.
    fn stage_lists(&self, world: &World) -> Lists {
        let map = world.data.stage_key.split('_').next().unwrap_or("");
        let named = scopes::events_named(&serde_json::to_string(&world.data.stage).unwrap_or_default(), &self.known);
        let (amb, sfx) = (format!("amb_map_{map}_"), format!("sfx_map_{map}_"));
        let mut classes: Vec<&str> = Vec::new();
        for p in world.data.stage.placed.iter().flatten() {
            if !classes.contains(&p.cls.as_str()) {
                classes.push(&p.cls);
            }
        }
        let actors = classes.iter().flat_map(|c| world.registry.stage_actors.get(c).map_or(&[][..], |k| k.sounds));
        let code = self
            .scenery_events
            .iter()
            .map(String::as_str)
            .chain(actors.copied())
            .map(str::to_lowercase)
            .filter(|e| self.known.has(e));
        let mut seen = HashSet::new();
        let evs: Vec<String> = named
            .into_iter()
            .chain(code)
            .chain(self.known.all().iter().filter(|e| e.starts_with(&amb) || e.starts_with(&sfx)).cloned())
            .filter(|e| seen.insert(e.clone()))
            .collect();
        // (a map's emitter one-shots start on their own random timers, not on a frame: held as files)
        let lazy_emit = |e: &str| {
            e.strip_prefix("amb_map_")
                .and_then(|r| r.split_once('_'))
                .is_some_and(|(m, r)| !m.is_empty() && r.starts_with("emit_"))
        };
        Lists {
            keep: evs.iter().filter(|e| !lazy_emit(e)).cloned().collect(),
            lazy: evs.iter().filter(|e| lazy_emit(e)).cloned().collect(),
        }
    }

    /// scopeRingOutAudio: every entry's pair of each asset the players' ring-outs name; ROV_Default's pair is the
    /// shared scope's.
    fn ring_out_events(&self, world: &World) -> Vec<String> {
        let mut assets: Vec<&str> = Vec::new();
        for p in &world.setup.players {
            let a = ring_out_asset_name(world, p.ring_out.as_deref());
            if !assets.contains(&a) {
                assets.push(a);
            }
        }
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for a in assets {
            let Some(asset) = world.data.ring_outs.assets.get(a) else { continue };
            for e in &asset.entries {
                for ev in [&e.ally, &e.enemy] {
                    if !ev.is_empty()
                        && !ev.starts_with("sfx_blastbox_default_")
                        && self.known.has(ev)
                        && seen.insert(ev.clone())
                    {
                        out.push(ev.clone());
                    }
                }
            }
        }
        out
    }

    /// scopeItemsAudio: the items part's sounds for a match with random items.
    fn item_events(&self, world: &World) -> Vec<String> {
        match &world.data.items {
            Some(items) if mvs_sim::setup::items_on(&world.setup) => {
                scopes::item_events(&serde_json::to_string(items).unwrap_or_default(), &self.known)
            }
            _ => Vec::new(),
        }
    }

    // ---- the step

    /// A step's sound events posted and the fighters' emitters placed.
    pub fn tick(&mut self, world: &World, events: &[Event], out: &mut Vec<Line>) {
        self.tick_with(world, events, &[], &mut NoViews, out);
    }

    /// [`MatchAudio::tick`] with the skins' own intros' sounds of the tick (ViewPoser::intro_sounds: the fighter's id
    /// and the AkEvent), the drawn pawns' last pose and the effects' ring-out picks.
    pub fn tick_with(
        &mut self,
        world: &World,
        events: &[Event],
        intro: &[(i32, String)],
        views: &mut dyn Views,
        out: &mut Vec<Line>,
    ) {
        let mut c = Calls { tick: u32::try_from(world.flow.tick).unwrap_or(0), out };
        // The fighters' Wwise game objects: actor location, forward (0, -1, 0) facing right (yaw -90).
        for f in world.all_ids() {
            let f = world.fighter(f);
            c.emitter(
                f.id,
                [0.0, f64::from(-f.body.x), f64::from(f.body.y)],
                [0.0, f64::from(-f.body.facing_dir), 0.0],
            );
        }
        for (_, sh) in world.actors.ships.iter() {
            if !sh.dead {
                c.emitter(
                    AK_SHIP + world.fighter(sh.owner).id,
                    [0.0, f64::from(-sh.x), f64::from(sh.y)],
                    [0.0, -1.0, 0.0],
                );
            }
        }
        self.tick_music(world, events.iter().any(|e| matches!(e, Event::CombatInput { .. })), &mut c);
        // A skin's own intro: its sounds at the times the view's position passed this tick,
        // posted on the fighter's root as the sim's Ak_OneShot is; the default intro's sounds not played (event).
        for (id, ev) in intro {
            let ev = self.skin_event_name(world, *id, ev);
            c.post_on(&ev, *id);
        }
        for e in events {
            self.event(world, e, views, &mut c);
        }
        // (the stretch actors' Tick, on the pose just drawn)
        self.head_pitch.tick(world, views, &mut c);
        // (presentTick: the modules' place posts their sounds after handleEvents, then the scenery's tick)
        for (ev, at) in views.take_sounds() {
            c.post(&ev, Some(-1), None, None, Some(at));
        }
        self.flush_scene(&mut c);
    }

    /// A fighter id that names no pawn record: a minion, or a mode's pawn destroyed this tick.
    fn off_record(world: &World, id: i32) -> bool {
        id >= 0 && !world.by_id(id).is_some_and(|f| !world.fighter(f).life.is_minion)
    }

    fn locally_controlled(&self, i: i32) -> bool {
        self.net_me.is_none_or(|me| me == i)
    }

    /// Lib_Audio PF_Skin_Manager for a post whose actor is player fighter `i`: the event's name with the default
    /// skin's AudioName replaced by the applied skin's when that event is loaded, else the original.
    fn skin_event_name(&self, world: &World, fighter: i32, ev: &str) -> String {
        let swap = usize::try_from(fighter).ok().and_then(|i| self.swaps.get(i)).and_then(Option::as_ref);
        let loaded = |low: &str| {
            f64::from(world.flow.tick) >= self.skin_load_frames && self.skin_loaded.contains(low) && self.known.has(low)
        };
        match swap {
            Some((from, to)) => swap_skin_event(ev, from, to, loaded),
            None => ev.to_owned(),
        }
    }

    /// The sim's announcer event (the default pack's, World.announce) as this machine's pack's event of the same alias,
    /// None where that pack has no row of it (pfgPlayVO finds none: the tutorial's lines, which only
    /// DefaultAnnouncerPack holds); any other event as it is.
    // Deviation: the sim holds the fighters' lines back for the default pack's line's length.
    fn local_announcer_event(&self, ev: &str) -> Option<String> {
        let Some(alias) = self.aliases.get(&ev.to_lowercase()) else { return Some(ev.to_owned()) };
        self.announcer.rows.iter().find(|r| r.1.is_empty() && r.0.eq_ignore_ascii_case(alias)).map(|r| r.3.clone())
    }

    fn key_string(world: &World, fighter: i32, k: SoundKey) -> String {
        key_text(world.names.str(k.tag), fighter, k.n)
    }

    fn next_spawned(&mut self) -> i32 {
        self.spawned = (self.spawned + 1) % AK_SPAWNED_COUNT;
        AK_SPAWNED + self.spawned as i32
    }

    fn next_pool(&mut self) -> i32 {
        self.pool = (self.pool + 1) % AK_POOL_SIZE;
        AK_POOL + self.pool as i32
    }

    /// Comp_Actor_Audio_Container's VO_Pain of a fighter's data.
    fn vo_pain(world: &World, id: i32) -> Option<String> {
        let f = world.by_id(id)?;
        let comps = &world.char_data(f).fighter.components;
        const K: &str = "Comp_Actor_Audio_Container_GEN_VARIABLE";
        let c = comps.pawn.get(K).or_else(|| comps.base.get(K))?;
        c.p.as_obj()?.get("VO_Pain")?.as_str().map(str::to_owned)
    }

    /// PlayHitSound's victim part: a hit over 2.0 damage stops the victim's character sounds and posts its VO_Pain.
    fn pain_vo(&self, world: &World, id: i32, damage: f64, c: &mut Calls) {
        if f64::from(damage as f32) > 2.0 {
            c.post_on(SFX_STOP_ALL_CHARACTER, id);
            if let Some(vo) = Self::vo_pain(world, id) {
                let ev = self.skin_event_name(world, id, &vo);
                c.post_on(&ev, id);
            }
        }
    }

    fn event(&mut self, world: &World, e: &Event, views: &mut dyn Views, c: &mut Calls) {
        if views.muted_by_intro(world, e) {
            return;
        }
        let s = |n: mvs_sim::names::Name| world.names.str(n);
        match e {
            Event::Sound { fighter, ev, x, y, hit, pooled, key } => {
                let ev = s(*ev);
                if let Some(h) = hit {
                    self.hit_sound(world, *fighter, ev, x.map(f64::from), y.map(f64::from), h, views, c);
                } else if *pooled {
                    let comp = self.next_pool();
                    c.rtpc("cha_locally_controlled", 1.0, comp, 0.0);
                    c.post(
                        ev,
                        Some(comp),
                        None,
                        None,
                        Some([0.0, f64::from(-x.unwrap_or(0.0)), f64::from(y.unwrap_or(0.0))]),
                    );
                } else if ev.starts_with("sfx_cha_C019_spaceship_loop") {
                    // C019_Ship posts its loop and the loop's stop on itself: the actor is the emitter until destroyed.
                    let go = AK_SHIP + fighter;
                    if let Some(x) = x {
                        c.emitter(go, [0.0, f64::from(-x), f64::from(y.unwrap_or(0.0))], [0.0, -1.0, 0.0]);
                    }
                    c.rtpc("cha_locally_controlled", f64::from(u8::from(self.locally_controlled(*fighter))), go, 0.0);
                    c.post_on(ev, go);
                } else {
                    // (the game state's, fighter -1: the announcer's line in this machine's pack)
                    let ev = if *fighter == -1 {
                        self.local_announcer_event(ev)
                    } else {
                        Some(self.skin_event_name(world, *fighter, ev))
                    };
                    if let Some(ev) = ev {
                        let at = x.map(|x| {
                            let y = y.or_else(|| world.by_id(*fighter).map(|f| world.fighter(f).body.y)).unwrap_or(0.0);
                            [0.0, -f64::from(x), f64::from(y)]
                        });
                        c.post(&ev, Some(*fighter), key.map(|k| Self::key_string(world, *fighter, k)), None, at);
                    }
                }
            }
            Event::SpawnedSound { instigator, ev, x, y, rtpc, value } => {
                // the component at the place, its RTPC, then PF_Post_Event_On_AkComponent_WithInstigator: the
                // instigator's cha_locally_controlled on the component and the post on it (its PF_Skin_Manager reads
                // the component's owner's Instigator: none for a stage's, so the name is the event's own)
                let comp = self.next_spawned();
                c.rtpc(s(*rtpc), f64::from(*value), comp, 0.0);
                c.rtpc("cha_locally_controlled", f64::from(u8::from(self.locally_controlled(*instigator))), comp, 0.0);
                c.post(s(*ev), Some(comp), None, None, Some([0.0, f64::from(-x), f64::from(*y)]));
            }
            Event::SoundStart { fighter, ev, key } => {
                let ev = self.skin_event_name(world, *fighter, s(*ev));
                c.post(&ev, Some(*fighter), Some(Self::key_string(world, *fighter, *key)), None, None);
            }
            Event::SoundStop { fighter, key, fade, ev, curve, cut } => {
                let k = Self::key_string(world, *fighter, *key);
                match ev {
                    Some(ev) => {
                        let ev = self.skin_event_name(world, *fighter, s(*ev));
                        c.post(&ev, None, cut.then(|| k.clone()), Some(k), None);
                    }
                    None => c.push(Call::StopKey {
                        key: k,
                        fade: f64::from(fade.unwrap_or(0.05)),
                        curve: curve.map(|n| s(n).to_owned()),
                    }),
                }
            }
            Event::Rtpc { fighter, name, value, interp_ms } => {
                c.rtpc(s(*name), f64::from(*value), *fighter, f64::from(*interp_ms))
            }
            Event::Switch { fighter, group, value } => c.switch(s(*group), s(*value), *fighter),
            Event::Hit { victim, pain_damage, .. } => {
                if Self::off_record(world, *victim) {
                    self.minion_pain_vo(world, *victim, f64::from(*pain_damage), c);
                } else {
                    self.pain_vo(world, *victim, f64::from(*pain_damage), c);
                }
            }
            Event::PulseHit { fighter, pain_damage } => {
                if Self::off_record(world, *fighter) {
                    self.minion_pain_vo(world, *fighter, f64::from(*pain_damage), c);
                } else {
                    self.pain_vo(world, *fighter, f64::from(*pain_damage), c);
                }
            }
            Event::PopText { fighter, kind, stacks, .. } => {
                // Comp_State_Manager BuffDecayUI and HitDecayUIMulticast: the dealer's cue, after the pop text.
                let decay = match kind {
                    PopKind::BuffDecay => true,
                    PopKind::AttackDecay => *stacks > 3.0,
                    _ => false,
                };
                if decay && self.locally_controlled(*fighter) {
                    c.post_on("ui_ability_sameattack", *fighter);
                }
            }
            Event::MatchPoint { .. } => self.music.match_point = true,
            Event::MatchPointShow { .. } => self.music_state("main_b", c),
            Event::MatchOver { .. } => {
                self.music.match_point = false;
                self.music_state("outro", c);
            }
            Event::MatchEnd { phase, .. } => {
                if *phase == EndPhase::PreTravel {
                    c.rtpc("transiiton", 0.0, -1, 3000.0);
                }
            }
            Event::IntroFx { fighter } if !Self::off_record(world, *fighter) => {
                c.post("ui_characterselect_lightbeam", None, None, None, None);
            }
            Event::Abduct { fighter } if !Self::off_record(world, *fighter) => {
                if let Some(f) = world.by_id(*fighter) {
                    let b = &world.fighter(f).body;
                    c.post(
                        "sfx_map_shared_endgamelosers",
                        Some(-1),
                        None,
                        None,
                        Some([0.0, f64::from(-b.x), f64::from(b.y)]),
                    );
                }
            }
            Event::ArmorHit { fighter, .. } if !Self::off_record(world, *fighter) => {
                c.post_on("ui_ability_armorblock", *fighter)
            }
            Event::ArmorBreak { fighter } if !Self::off_record(world, *fighter) => {
                c.post_on("ui_ability_armorbreak", *fighter)
            }
            Event::KoVfx { fighter, asset, p, .. } if !Self::off_record(world, *fighter) => {
                // HandleBlastBoxDeathFX: the entry's AllyAudioEvent for the local player's team (GetColorIndex != 0),
                // else its EnemyAudioEvent, each the default pair's when not valid, at the loser's place.
                if let Some((ally, enemy)) = views.ko_sound(world, s(*asset)) {
                    let is_ally = self.color_index(world, *fighter) != 0;
                    let ev = if is_ally { ally } else { enemy };
                    let ev = if ev.is_empty() {
                        if is_ally { "sfx_blastbox_default_ally" } else { "sfx_blastbox_default_enemy" }.to_owned()
                    } else {
                        ev
                    };
                    c.post(&ev, Some(-1), None, None, Some([0.0, f64::from(p[0]), f64::from(p[1])]));
                }
            }
            Event::BlastBoxSound { fighter, p } if !Self::off_record(world, *fighter) => {
                // HandleBlastBoxDeathFX with NotPlayerCharacter: the default pair (no emitter data on that path).
                let ev = if self.color_index(world, *fighter) != 0 {
                    "sfx_blastbox_default_ally"
                } else {
                    "sfx_blastbox_default_enemy"
                };
                c.post(ev, Some(-1), None, None, Some([0.0, f64::from(p[0]), f64::from(p[1])]));
            }
            Event::DisguiseFx { fighter, kind } => {
                // Buff_Disguise's poof: ActivateDisguiseMulticast U@23521-23707
                // posts sfx_shared_class_assassin_clone_cast, ReceiveEndPlay U@13965-14376
                // sfx_cha_batman_smokebomb_end, at the owner's actor location (no fighter's emitter: owner -1).
                if let Some(f) = world.by_id(*fighter) {
                    let ev = match kind {
                        DisguiseFxKind::Activate => "sfx_shared_class_assassin_clone_cast",
                        DisguiseFxKind::Destroy => "sfx_cha_batman_smokebomb_end",
                    };
                    let at = [0.0, -world.fighter(f).body.x, mvs_sim::fighter::actor_y(world, f)];
                    c.post(ev, Some(-1), None, None, Some(at.map(f64::from)));
                }
            }
            _ => {}
        }
    }

    /// GetColorIndex of a fighter's pawn record (its team's colour).
    fn color_index(&self, world: &World, id: i32) -> u8 {
        usize::try_from(id)
            .ok()
            .and_then(|i| {
                mvs_view::skins::fighter_colours(&world.setup, self.me, self.online, false)
                    .get(i)
                    .map(|c| c.color_index)
            })
            .unwrap_or(0)
    }

    /// A minion's own hit sound: the victim part of PlayHitSound as `pain_vo` has it, from the minion's own data.
    fn minion_pain_vo(&self, world: &World, id: i32, damage: f64, c: &mut Calls) {
        if world.by_id(id).is_none() || f64::from(damage as f32) <= 2.0 || damage.is_nan() {
            return;
        }
        c.post_on(SFX_STOP_ALL_CHARACTER, id);
        if let Some(vo) = Self::vo_pain(world, id) {
            c.post_on(&vo, id);
        }
    }

    /// Comp_Pawn_DamageManager PlayHitSound's hit sound: a pooled component moved to the hit's place, its switches
    /// and parameters set, the event posted on it.
    #[allow(clippy::too_many_arguments)]
    fn hit_sound(
        &mut self,
        world: &World,
        fighter: i32,
        ev: &str,
        x: Option<f64>,
        y: Option<f64>,
        h: &mvs_sim::events::SoundHit,
        views: &mut dyn Views,
        c: &mut Calls,
    ) {
        let comp = self.next_pool();
        let vic = world.by_id(h.victim).map(|f| world.fighter(f));
        let x = x.or_else(|| vic.map(|v| v.body.x).map(f64::from)).unwrap_or(0.0);
        let y = y.or_else(|| vic.map(|v| v.body.y).map(f64::from)).unwrap_or(0.0);
        let at = match h.z_floor {
            None => [0.0, -x, y],
            Some(z) => views.nearest_bone(h.victim, [0.0, -x, y.max(f64::from(z))]),
        };
        let surface = usize::try_from(h.surface.unwrap_or(6)).ok().and_then(|i| PHYSMAT.get(i));
        if let Some(surface) = surface {
            c.switch("physmat", surface, comp);
        }
        if let Some(charge) = h.charge {
            c.rtpc("cha_charge_hit", f64::from(charge), comp, 0.0);
        }
        let local = self.locally_controlled(fighter)
            || if h.victim >= 0 {
                self.locally_controlled(h.victim)
            } else {
                h.instigator.is_some_and(|i| self.locally_controlled(i))
            };
        c.rtpc("cha_locally_controlled", f64::from(u8::from(local)), comp, 0.0);
        let ev = self.skin_event_name(world, fighter, ev);
        c.post(&ev, Some(comp), None, None, Some(at));
    }

    // ---- the music

    fn music_state(&mut self, s: &str, c: &mut Calls) {
        self.music.state = s.to_owned();
        let value = if s == "none" { "z_mus_map_state_none".to_owned() } else { format!("mus_{s}") };
        c.switch("mus_map_state", &value, -1);
    }

    fn combat_timer(&mut self, world: &World, c: &mut Calls) {
        self.music.timer = 25.0;
        // The Lab's RoundTimeRemaining is -1 (a fresh match config): main_b.
        if self.music.state == "nocombat" {
            let a = !self.lab && world.round_time_remaining() > 60.0 && !self.music.match_point;
            self.music_state(if a { "main_a" } else { "main_b" }, c);
        }
    }

    /// Map_TrainingRoomSmall's level Blueprint and PandaGameState: the flow of the map's music, in game time.
    fn tick_music(&mut self, world: &World, press: bool, c: &mut Calls) {
        if self.music.t < 0.0 {
            return;
        }
        let was = self.music.t;
        self.music.t += STEP;
        let now = self.music.t;
        let at = |t: f64| was < t && now >= t;
        if at(0.1) {
            c.state("state_gameflow", "state_gameflow_map");
            let map = world.data.stage.audio.as_ref().map_or("mus_map_lionwatch", |a| a.music.map.as_str());
            c.switch("mus", map, -1);
            let skin = self.music.skin.clone();
            c.switch("mus_skin", &skin, -1);
            self.music_state("intro", c);
            // Unverified: the moment is taken with the gameflow state's; the Lab's team style is its native config's.
            let mode = ["1v1", "2v2", "freeforall", "null"][(world.setup.team_style as usize).min(3)];
            c.state("state_gamemode", &format!("state_gamemode_{mode}"));
            c.state(
                "state_network",
                if self.net_me.is_some() { "state_network_online" } else { "state_network_offline" },
            );
        }
        // (PandaGameMode BeginPlay -> Delay 0.2 -> StartNewMatch: the 25 s timer starts while the map state is intro)
        if at(0.2) {
            self.combat_timer(world, c);
        }
        if at(1.1) {
            self.music_state("main_a", c);
        }
        if self.music.timer > 0.0 {
            self.music.timer -= STEP;
            if self.music.timer <= 0.0 && (self.music.state == "main_a" || self.music.state == "main_b") {
                self.music_state("nocombat", c);
            }
        }
        if press && now >= 1.1 {
            self.combat_timer(world, c);
        }
    }

    // ---- the frame

    /// The HUD's posts (UI_Game's PF_Post_Global_Event) of the frame.
    pub fn hud_sounds(&mut self, tick: i32, events: Vec<String>, out: &mut Vec<Line>) {
        let mut c = Calls { tick: u32::try_from(tick).unwrap_or(0), out };
        for ev in events {
            c.post_on(&ev, -1);
        }
    }

    /// The frame's listener (the camera's point of view as drawn), the effects' own sounds and the scenery's queued
    /// calls (`Views::take_sounds`: an effect's sound
    /// posted with no owner, at its place). The caller runs it after both.
    pub fn frame(&mut self, tick: i32, pov: &Pov, views: &mut dyn Views, out: &mut Vec<Line>) {
        let mut c = Calls { tick: u32::try_from(tick).unwrap_or(0), out };
        for (ev, at) in views.take_sounds() {
            c.post(&ev, Some(-1), None, None, Some(at));
        }
        self.flush_scene(&mut c);
        c.push(Call::SetListener { pos: pov.pos, right: pov.right, up: pov.up, front: pov.front });
    }

    /// The match left: its sounds stopped, its scopes let go.
    pub fn end(&mut self, tick: i32, level_goes_on: bool, out: &mut Vec<Line>) {
        let mut c = Calls { tick: u32::try_from(tick).unwrap_or(0), out };
        c.push(Call::StopAll { fade: 0.1, keep: level_goes_on.then(|| STAGE_AMBIENCE.into()) });
    }

    /// A scenery actor's own game object, by the key it was first placed under.
    fn scenery_object(&mut self, key: &str) -> i32 {
        let i = match self.scenery_objects.iter().position(|k| k == key) {
            Some(i) => i,
            None => {
                self.scenery_objects.push(key.to_owned());
                self.scenery_objects.len() - 1
            }
        };
        AK_SCENERY + i as i32
    }

    /// The scenery's calls since the last flush, in order.
    fn flush_scene(&mut self, c: &mut Calls) {
        let calls = std::mem::take(&mut *queue(&self.scene));
        for call in calls {
            match call {
                SceneCall::PostAt { event, at, key } => {
                    let k = key.as_ref().map_or_else(|| STAGE_AMBIENCE.to_owned(), |k| format!("{STAGE_AMBIENCE}:{k}"));
                    c.post(&event, Some(-1), Some(k.clone()), key.map(|_| k), Some(at));
                }
                SceneCall::PostOn { event, key, at } => {
                    let go = self.scenery_object(&key);
                    c.emitter(go, at, [1.0, 0.0, 0.0]);
                    let k = format!("{STAGE_AMBIENCE}:{key}");
                    c.post(&event, Some(go), Some(k.clone()), Some(k), None);
                }
                SceneCall::Place { key, at } => {
                    let go = self.scenery_object(&key);
                    c.emitter(go, at, [1.0, 0.0, 0.0]);
                }
            }
        }
    }
}

/// A sim sound key as a string: its tag and number (`SoundKey`), the event's fighter where the key has two.
fn key_text(tag: &str, fighter: i32, n: i32) -> String {
    // (a key held as a bare string, the Court's `m010ShotClock`: its tag alone)
    if n == i32::MIN {
        return tag.to_owned();
    }
    // (the keys the sim builds from the fighter's id and a number: `{fighter}:vo{n}`, `{fighter}:charge{n}`,
    // `{fighter}:{n}`)
    if let Some((pre, post)) = tag.split_once('#') {
        format!("{pre}{n}{post}")
    } else if matches!(tag, ":vo" | ":charge" | ":") {
        format!("{fighter}{tag}{n}")
    } else if tag.starts_with(':') {
        format!("{n}{tag}")
    } else {
        format!("{tag}{n}")
    }
}

/// The exe's post tape (`--audio-tape <file>`): every line any screen hands the sound engine (a match's glue, the
/// menus' sink, the pre-match voice, the scopes), written before it plays, one JSON line each in the post tape's
/// format (mvs-audio tape/post.rs; the oracle's audio tape also holds the title's and the menus' calls
/// before a match's). Checked against the oracle's tape by `exe_audio_tape`.
pub struct TapeWriter {
    out: std::io::BufWriter<std::fs::File>,
}

impl TapeWriter {
    pub fn create(path: &Path) -> Result<TapeWriter> {
        let f = std::fs::File::create(path).map_err(|e| Error::Match(format!("{}: {e}", path.display())))?;
        Ok(TapeWriter { out: std::io::BufWriter::new(f) })
    }

    /// The tape the program writes from now on (`--audio-tape`): one a run, for every screen.
    pub fn open(path: &Path) -> Result<()> {
        let t = TapeWriter::create(path)?;
        *TAPE.lock().unwrap_or_else(PoisonError::into_inner) = Some(t);
        Ok(())
    }

    /// Whether the program writes a tape.
    pub fn open_now() -> bool {
        TAPE.lock().unwrap_or_else(PoisonError::into_inner).is_some()
    }

    /// The lines, in order: each is tried (a failed write is reported once per call, the next lines still go). A
    /// non-finite number is written as null, as JSON.stringify does (serde_json does the same).
    pub fn write(&mut self, lines: &[Line]) {
        let mut failed = false;
        for l in lines {
            let r = serde_json::to_writer(&mut self.out, l).map_err(std::io::Error::from);
            if let Err(e) = r.and_then(|()| self.out.write_all(b"\n")) {
                if !failed {
                    eprintln!("mvs: audio tape: {e}");
                }
                failed = true;
            }
        }
        // (flushed at once: the program's tape is never dropped, and a run that ends in an error keeps its lines)
        if let Err(e) = self.out.flush()
            && !failed
        {
            eprintln!("mvs: audio tape: {e}");
        }
    }
}

/// The program's post tape (`TapeWriter::open`): process-wide like the run's mute and background flags, so every
/// screen's sound calls reach it without a handle through each sink.
static TAPE: Mutex<Option<TapeWriter>> = Mutex::new(None);

/// `lines` written to the program's tape, when it writes one.
fn tape(lines: &[Line]) {
    if let Some(t) = TAPE.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        t.write(lines);
    }
}

/// One call handed to the sound engine (dropped without one), written to the program's tape first.
pub fn send(audio: &mut Option<mvs_audio::Audio>, line: &Line) {
    tape(std::slice::from_ref(line));
    let Some(a) = audio else { return };
    if let Err(e) = a.call(line) {
        eprintln!("mvs: audio: {e}");
    }
}

impl Drop for TapeWriter {
    fn drop(&mut self) {
        if let Err(e) = self.out.flush() {
            eprintln!("mvs: audio tape: {e}");
        }
    }
}

/// Holds the base scope (the shared events the port's code posts, whatever the match) from
/// the sound engine's start, the HUD bundle's named events with them (hud(): eventsNamed(app.hudAssets.data)).
pub fn scope_base(audio: &mut Option<mvs_audio::Audio>) {
    let Some(a) = audio else { return };
    let known = Index::new(a.init().events.keys().cloned());
    let hud = hud_named(a.store(), &known);
    let call = Call::Scope { name: "base".to_owned(), keep: base_events(&known, &hud), lazy: Vec::new() };
    send(audio, &Line { tick: 0, call });
}

/// The events the HUD bundle's data names (the ui kind's `hud` entry, its string values), lower case, each once;
/// none when the store has no such entry.
pub fn hud_named(store: &mvs_store::Store, known: &Index) -> Vec<String> {
    let Ok(pack) = store.kind("ui") else { return Vec::new() };
    let Some(path) = pack.paths().find(|p| p.ends_with("/hud.hud")).map(str::to_owned) else { return Vec::new() };
    let Ok(bytes) = pack.bytes(&path) else { return Vec::new() };
    scopes::events_named(&String::from_utf8_lossy(&bytes), known)
}

/// The match and its results left for the menus: no stage's music, its
/// media let go as they play out under the menu's.
pub fn release_stage_music(audio: &mut Option<mvs_audio::Audio>) {
    if audio.is_none() {
        return;
    }
    let call = Call::ScopeMusic { ev: "mus_play".to_owned(), values: Vec::new(), flow: None };
    send(audio, &Line { tick: 0, call });
}

/// Hands the glue's calls to the sound engine (dropped without one), each written to the program's tape first.
pub fn play(audio: &mut Option<mvs_audio::Audio>, calls: &mut Vec<Line>) {
    tape(calls);
    let Some(a) = audio else {
        calls.clear();
        return;
    };
    for l in calls.drain(..) {
        if let Err(e) = a.call(&l) {
            eprintln!("mvs: audio: {e}");
        }
    }
}

/// What the sound engine said, kept across frames: each warning printed once, all counted.
#[derive(Debug, Default)]
pub struct NoteLog {
    /// Warnings by what and id, with how often each came.
    pub counts: indexmap::IndexMap<(&'static str, u64), u32>,
    /// The warnings by what since the last summary line, and when that line was due.
    window: indexmap::IndexMap<&'static str, u32>,
    window_from: Option<std::time::Instant>,
    /// The posts the command queue had dropped at the last summary.
    queue_dropped: u32,
}

/// How often the summary of the warnings that came is printed at most (and only when some came).
const NOTE_SUMMARY_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// Takes the sound engine's notes once a frame (which also sends the lazy posts whose media are in): a warning is
/// printed the first time it comes, and counted; every few seconds in which warnings came, one line counts them by
/// what (a warning printed once per id says nothing of a hundred drops that follow it), the command queue's dropped
/// posts among them. A bus mute and an unmute are printed each time.
pub fn drain_notes(audio: &mut Option<mvs_audio::Audio>, log: &mut NoteLog) {
    use mvs_audio::ctl::post::{BUSES_MUTED, BUSES_UNMUTED};
    let Some(a) = audio else { return };
    for n in a.take_notes() {
        if let mvs_audio::Note::Warn { what, id } = n {
            let c = log.counts.entry((what, id)).or_insert(0);
            if *c == 0 || what == BUSES_MUTED || what == BUSES_UNMUTED {
                eprintln!("mvs: audio: {what} ({id})");
            }
            *c += 1;
            *log.window.entry(what).or_insert(0) += 1;
        }
    }
    let queue = a.dropped();
    if queue != log.queue_dropped {
        *log.window.entry("posts dropped by the command queue").or_insert(0) += queue.wrapping_sub(log.queue_dropped);
        log.queue_dropped = queue;
    }
    let now = std::time::Instant::now();
    let from = *log.window_from.get_or_insert(now);
    if now.duration_since(from) < NOTE_SUMMARY_EVERY {
        return;
    }
    log.window_from = Some(now);
    log.window.retain(|w, _| *w != BUSES_MUTED && *w != BUSES_UNMUTED);
    if log.window.is_empty() {
        return;
    }
    let parts: Vec<String> = log.window.drain(..).map(|(what, n)| format!("{what} x{n}")).collect();
    eprintln!("mvs: audio: in the last {} s: {}", NOTE_SUMMARY_EVERY.as_secs(), parts.join(", "));
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_sim_trace::harness;

    /// A trace match stepped from `frame` with its recorded inputs until `found` holds for a step's events (at most
    /// `steps`): the harness and those events. None without the baseline traces.
    fn step_to(
        name: &str,
        frame: usize,
        steps: usize,
        found: impl Fn(&World, &[Event]) -> bool,
    ) -> Option<(harness::Harness, Vec<Event>)> {
        let mut h = harness::at(name, frame)?;
        for _ in 0..steps {
            h.step_recorded();
            let ev = std::mem::take(&mut h.w.events);
            if found(&h.w, &ev) {
                return Some((h, ev));
            }
        }
        panic!("{name}: nothing found from frame {frame} in {steps} steps");
    }

    /// The trampoline's bounce: its RTPC on a new component, the Instigator's cha_locally_controlled, the post on that
    /// component at the tree (TreeFortTrampolineBounceAudio).
    #[test]
    fn a_trampoline_bounce_posts_on_its_own_component_after_its_rtpc() {
        let bounce = |_: &World, ev: &[Event]| ev.iter().any(|e| matches!(e, Event::SpawnedSound { .. }));
        let Some((h, ev)) = step_to("stage-treehouse", 990, 30, bounce) else { return };
        let mut out = Vec::new();
        let mut m = MatchAudio::with_index(&h.w, Index::new(Vec::<String>::new()), &mut out);
        out.clear();
        m.tick(&h.w, &ev, &mut out);
        let calls: Vec<&Call> = out.iter().map(|l| &l.call).filter(|c| !matches!(c, Call::SetEmitter { .. })).collect();
        let i = calls
            .iter()
            .position(|c| matches!(c, Call::SetRtpc { name, .. } if name == "map_treehouse_trampoline"))
            .expect("the bounce's RTPC");
        let Call::SetRtpc { value, owner, interp_ms, .. } = calls[i] else { unreachable!() };
        assert_eq!(*owner, AK_SPAWNED);
        assert!((0.0..=1.0).contains(value) && *interp_ms == 0.0, "{value} {interp_ms}");
        assert!(
            matches!(calls[i + 1], Call::SetRtpc { name, value, owner, .. }
                if name == "cha_locally_controlled" && *value == 1.0 && *owner == AK_SPAWNED),
            "{:?}",
            calls[i + 1]
        );
        assert!(
            matches!(calls[i + 2], Call::Post { ev, owner: Some(o), at: Some(_), .. }
                if ev == "sfx_map_treehouse_trampoline_tree" && *o == AK_SPAWNED),
            "{:?}",
            calls[i + 2]
        );
    }

    /// Views whose head bone's X axis is pitched by the next of `pitches` (degrees) at each read.
    struct Pitched(std::cell::RefCell<Vec<f64>>);

    impl Views for Pitched {
        fn nearest_bone(&self, _victim: i32, hit: V3) -> V3 {
            hit
        }
        fn ko_sound(&mut self, _world: &World, _asset: &str) -> Option<(String, String)> {
            None
        }
        fn socket_rows(&self, _world: &World, _id: i32, socket: &str) -> Option<[f64; 9]> {
            assert_eq!(socket, "fml_un_C_head");
            let p = self.0.borrow_mut().remove(0).to_radians();
            Some([p.cos(), 0.0, p.sin(), 0.0, 1.0, 0.0, -p.sin(), 0.0, p.cos()])
        }
    }

    /// JakeStretch's Tick: cha_jake_pitch = FClamp(|head Pitch| - the last |Pitch|, 0, 2) on Jake over 10 ms.
    #[test]
    fn a_stretch_sends_its_head_pitch_change_each_tick() {
        let ticking = |w: &World, _: &[Event]| {
            w.actors.actors.iter().any(|(_, x)| {
                !x.dead && matches!(&x.state, mvs_sim::actors::world_actor::WorldActorState::Stretch(s) if s.ticking)
            })
        };
        let Some((h, _)) = step_to("jake-combat", 0, 3000, ticking) else { return };
        let mut out = Vec::new();
        let mut m = MatchAudio::with_index(&h.w, Index::new(Vec::<String>::new()), &mut out);
        let mut views = Pitched(std::cell::RefCell::new(vec![-30.0, -30.5, -40.0, 10.0]));
        let mut sent = Vec::new();
        for _ in 0..4 {
            out.clear();
            m.tick_with(&h.w, &[], &[], &mut views, &mut out);
            for l in &out {
                if let Call::SetRtpc { name, value, interp_ms, .. } = &l.call
                    && name == "cha_jake_pitch"
                {
                    assert_eq!(*interp_ms, 10.0);
                    sent.push(*value);
                }
            }
        }
        let want = [2.0, 0.5, 2.0, 0.0];
        assert_eq!(sent.len(), 4, "{sent:?}");
        for (a, b) in sent.iter().zip(want) {
            assert!((a - b).abs() < 1e-9, "{sent:?}");
        }
    }

    #[test]
    fn sound_keys_as_the_page_writes_them() {
        assert_eq!(key_text("m010ShotClock", 0, i32::MIN), "m010ShotClock");
        assert_eq!(key_text(":wind", 3, 1), "1:wind");
        assert_eq!(key_text("proj", 0, 7), "proj7");
        assert_eq!(key_text("proj#d", 0, 7), "proj7d");
        assert_eq!(key_text(":vo", 1, 3), "1:vo3");
        assert_eq!(key_text(":", 1, 6), "1:6");
    }
}
