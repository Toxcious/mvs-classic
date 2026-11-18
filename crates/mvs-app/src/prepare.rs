//! A match's load off the frame thread: what `MatchLoop::new` reads and builds before its first frame (the data, the
//! World's bind, the views, the effects and their level, the GPU half of the warm-up, the sound's begin and its scopes'
//! banks and media, the HUD) built on the loader's threads ([`PrepareJob::run`]) while the frame thread goes on drawing
//! the loading screen or the match before it; `MatchLoop::new` then installs it (`MatchStart::prepared`).
//!
//! What a job built is used only by the match it was made for: [`PrepKey`] holds every input that changes it, and a
//! start whose key differs loads on the frame thread as before. A part whose own inputs moved since the job was made
//! (the HUD's set-up, the scenery's sound events) is made again at the install.

use std::sync::Arc;
use std::time::Instant;

use mvs_audio::tape::post::{Call, Line};
use mvs_audio::{MusicFlow, PreparedMusic, PreparedScope, ScopeSource};
use mvs_data::GameData;
use mvs_render::fx::Fx;
use mvs_render::fx::surfaces::level_surfaces;
use mvs_render::scenery::level_emitters;
use mvs_render::stage::{StageJob, StagePrep};
use mvs_render::warm::{WarmJob, WarmLists, WarmMade};
use mvs_sim::{MatchSetup, World};
use mvs_ui::hud::Hud;
use mvs_ui::text::GameText;
use mvs_view::ViewPoser;
use mvs_view::skins::ViewPrefs;

use crate::app::{MatchKind, MatchStart, Shell};
use crate::audio::scopes::Index;
use crate::audio::{BeginOpts, MatchAudio};
use crate::lab_setup::LabMusic;
use crate::run::{HudInputs, HudMade, hud_inputs, hud_setup};

/// The inputs a prepared match was built from: a start whose key is the same takes it.
#[derive(Clone, Debug, PartialEq)]
pub struct PrepKey {
    pub setup: MatchSetup,
    pub seed: i32,
    /// The stage's key as the match opens it (a map resolved).
    pub stage: String,
    pub me: usize,
    pub online: bool,
    pub prefs: ViewPrefs,
    /// The level goes on from the match before (a Lab fighter change): the warm-up read the set stage.
    pub level_kept: bool,
}

/// What a job built for its match.
pub struct Prepared {
    pub key: PrepKey,
    pub world: World,
    pub poser: ViewPoser,
    pub parts: Parts,
    /// The job's steps, ms.
    pub laps: Vec<(&'static str, f64)>,
}

/// What a job built besides the World and the views.
pub struct Parts {
    /// The stage's set, prepared (None: a level that goes on, or no renderer).
    pub stage: Option<StagePrep>,
    /// The match's effects, its level begun (None: a level that goes on, whose player the install resumes).
    pub fx: Option<Fx>,
    /// The warm-up's lists and the GPU work its job did (None: no renderer).
    pub warm: Option<(WarmLists, WarmMade)>,
    /// The sound's glue begun and the calls of its begin (None: no sound engine).
    pub audio: Option<(MatchAudio, Vec<Line>)>,
    /// The sound scopes and music scopes the begin calls for (`Audio::offer_scopes`).
    pub scopes: Vec<PreparedScope>,
    pub music: Vec<PreparedMusic>,
    /// The InitBank's event index and this machine's announcer pack, for a begin made again at the install.
    pub known: Option<Index>,
    pub announcer: Option<mvs_data::AnnouncerPack>,
    /// The HUD (None: no UI).
    pub hud: Option<HudMade>,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared").field("key", &self.key).field("laps", &self.laps).finish()
    }
}

/// A match's load for a loader thread: what it reads, taken from the shell on the frame thread.
pub struct PrepareJob {
    key: PrepKey,
    store: mvs_store::Store,
    team_skins: bool,
    stage: Option<StageJob>,
    warm: Option<WarmJob>,
    audio: Option<AudioJob>,
    hud: Option<HudJob>,
}

/// The sound's part of a job: what `MatchAudio::begin` will be given.
struct AudioJob {
    source: ScopeSource,
    announcer_pack: String,
    /// The open scenery's events (a kept level's), else None: the job reads the stage's.
    scenery_events: Option<Vec<String>>,
    net_me: Option<i32>,
}

/// The HUD's part of a job.
struct HudJob {
    text: Arc<GameText>,
    inputs: HudInputs,
    usernames: Vec<String>,
    /// The renderer's assets, which the HUD's pictures and UI materials are warmed into (None: no renderer).
    assets: Option<Arc<mvs_render::assets::Assets>>,
}

/// The key `start` loads with on this shell (None: a replay, which loads its trace on the frame thread).
fn key_of(sh: &Shell, start: &MatchStart, level_kept: bool) -> Option<PrepKey> {
    if matches!(start.kind, MatchKind::Replay { .. }) {
        return None;
    }
    let stage = match &start.map {
        Some(m) => crate::run::stage_of_map(sh, m, start.stage.clone()),
        None => start.stage.clone(),
    };
    Some(PrepKey {
        setup: start.setup.clone(),
        seed: start.seed,
        stage,
        me: start.me,
        online: matches!(start.kind, MatchKind::Online { .. }),
        prefs: crate::run::view_prefs(&sh.settings.ui),
        level_kept,
    })
}

impl PrepareJob {
    /// The job of `start` on the shell's store, renderer, sound and UI; `level_kept`: the level the match before
    /// leaves goes on (a Lab fighter change). None for a replay or without an open store.
    pub fn of(sh: &Shell, start: &MatchStart, level_kept: bool) -> Option<PrepareJob> {
        let key = key_of(sh, start, level_kept)?;
        let store = sh.store()?.clone();
        let stage = sh.renderer.as_ref().filter(|_| !key.level_kept).map(|r| r.stage_job(&key.stage));
        let warm = sh.renderer.as_ref().map(|r| r.warm_job((!key.level_kept).then_some(key.stage.as_str())));
        let scenery_events = match (&sh.renderer, key.level_kept) {
            (Some(r), true) => Some(r.scenery_sound_events()),
            _ => None,
        };
        let net_me = if key.online { i32::try_from(key.me).ok() } else { None };
        let audio = sh.audio.as_ref().map(|a| AudioJob {
            source: a.source(),
            announcer_pack: sh.settings.hydra.announcer_pack.path.clone(),
            scenery_events,
            net_me,
        });
        let hud = sh.ui.as_ref().map(|u| HudJob {
            text: u.text.clone(),
            inputs: hud_inputs(sh, &start.kind),
            usernames: start.usernames.clone(),
            assets: sh.renderer.as_ref().map(mvs_render::Renderer::assets_shared),
        });
        let team_skins = sh.settings.ui.world_use_team_colored_skins;
        Some(PrepareJob { key, store, team_skins, stage, warm, audio, hud })
    }

    /// Builds the match's parts on this thread (a loader's).
    pub fn run(self) -> Result<Prepared, String> {
        let PrepareJob { key, store, team_skins, stage, warm, audio, hud } = self;
        let mut laps = Vec::new();
        let mut t = Instant::now();
        let mut lap = |name: &'static str, laps: &mut Vec<(&'static str, f64)>| {
            let now = Instant::now();
            laps.push((name, (now - t).as_secs_f64() * 1e3));
            t = now;
        };
        let keys = mvs_sim_trace::keys_of(&key.setup);
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let data = GameData::load(&store, &refs, &key.stage).map_err(|e| e.to_string())?;
        lap("data", &mut laps);
        let world = World::new(Arc::new(data), key.seed, &key.setup).map_err(|e| format!("the bind: {e:?}"))?;
        lap("world", &mut laps);
        let poser = ViewPoser::new_local(&store, &world, key.me, key.online, &key.prefs).map_err(|e| e.to_string())?;
        lap("poser", &mut laps);
        // The match's effects as `MatchLoop::new` makes them on a new level: the level's Emitter actors spawned
        // before the first step. A level that goes on resumes its player at the install; the warm-up's lists are then
        // a player's of the job's own.
        #[allow(clippy::cast_sign_loss)]
        let mut fx = Fx::new(&store, key.seed as u32).map_err(|e| e.to_string())?;
        fx.set_local(key.me, key.online);
        fx.team_skins = team_skins;
        if !key.level_kept {
            let emitters = level_emitters(&store, &key.stage).map_err(|e| e.to_string())?;
            let surfaces = level_surfaces(&store, &key.stage).map_err(|e| e.to_string())?;
            fx.begin_level(&emitters, surfaces).map_err(|e| e.to_string())?;
        }
        lap("fx", &mut laps);
        let stage = match stage {
            Some(job) => {
                let p = job.run().map_err(|e| e.to_string())?;
                lap("stage", &mut laps);
                Some(p)
            }
            None => None,
        };
        let warm = match warm {
            Some(job) => {
                let lists = WarmLists::of(&world, &poser, &mut fx);
                let made = job.run(&lists).map_err(|e| e.to_string())?;
                lap("warm", &mut laps);
                Some((lists, made))
            }
            None => None,
        };
        let fx = (!key.level_kept).then_some(fx);
        let (audio, scopes, music, known, announcer) = match audio {
            Some(a) => {
                let s = begin_audio(&world, &key, a);
                lap("scopes", &mut laps);
                (Some((s.glue, s.calls)), s.scopes, s.music, Some(s.known), s.announcer)
            }
            None => (None, Vec::new(), Vec::new(), None, None),
        };
        let hud = match hud {
            Some(h) => {
                let bundle = mvs_ui::bundle::Bundle::load(&store, "hud").map_err(|e| e.to_string())?;
                let setup = hud_setup(&h.inputs, &world, key.me, key.online, &h.usernames);
                let hud = Hud::new(&bundle, &h.text, &world, &setup).map_err(|e| e.to_string())?;
                lap("hud", &mut laps);
                // (the HUD's pictures and UI materials made with the load, so its first frames draw them; a warm that
                // fails leaves it to the install's, which reports it)
                let warmed = h.assets.as_ref().and_then(|a| {
                    let chars: Vec<&str> =
                        world.players.iter().map(|&f| world.names.str(world.fighter(f).ch)).collect();
                    mvs_ui::element::warm_hud(a, &bundle, &chars)
                        .inspect_err(|e| eprintln!("mvs: the HUD warm off the frame: {e}"))
                        .ok()
                });
                lap("hud warm", &mut laps);
                Some(HudMade { bundle, setup, hud, warmed })
            }
            None => None,
        };
        let parts = Parts { stage, fx, warm, audio, scopes, music, known, announcer, hud };
        Ok(Prepared { key, world, poser, parts, laps })
    }
}

/// The sound's begin a job made.
struct AudioBegun {
    glue: MatchAudio,
    calls: Vec<Line>,
    scopes: Vec<PreparedScope>,
    music: Vec<PreparedMusic>,
    known: Index,
    announcer: Option<mvs_data::AnnouncerPack>,
}

/// The match's sound begun (`MatchAudio::begin` but its sink, which the install attaches) and the scopes its calls
/// name prepared: each scope's banks read and media decoded. A scope that fails is left to the call's own load.
fn begin_audio(world: &World, key: &PrepKey, a: AudioJob) -> AudioBegun {
    let known = Index::new(a.source.init().events.keys().cloned());
    let announcer = match mvs_data::stored_announcer_pack(a.source.store(), &a.announcer_pack) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("mvs: the announcer pack {}: {e}", a.announcer_pack);
            None
        }
    };
    // (the stage scope's lists read the scenery's events: the level's own once its stage is set)
    let scenery_events = a.scenery_events.unwrap_or_else(|| {
        mvs_render::scenery::sound_events(a.source.store(), &key.stage).unwrap_or_else(|e| {
            eprintln!("mvs: the scenery's sound events of {}: {e}", key.stage);
            Vec::new()
        })
    });
    // (a level that goes on: its music flow is set at the install, `MatchAudio::resume_music`; the begin's calls do
    // not read it)
    let opts = BeginOpts {
        kept: key.level_kept.then(LabMusic::default),
        net_me: a.net_me,
        me: key.me,
        announcer: announcer.clone(),
        scenery_events,
        // (the podium only follows a match that is not the Lab's)
        podium: world.setup.flow.then(|| crate::audio::podium_montages(a.source.store())),
        ..BeginOpts::default()
    };
    let mut calls: Vec<Line> = Vec::new();
    let glue = MatchAudio::with_opts(world, known.clone(), &mut calls, opts);
    let (mut scopes, mut music) = (Vec::new(), Vec::new());
    for l in &calls {
        match &l.call {
            Call::Scope { name, keep, lazy } => {
                let keep: Vec<&str> = keep.iter().map(String::as_str).collect();
                let lazy: Vec<&str> = lazy.iter().map(String::as_str).collect();
                match a.source.prepare(name, &keep, &lazy) {
                    Ok(p) => scopes.push(p),
                    Err(e) => eprintln!("mvs: audio: the scope {name} prepared: {e}"),
                }
            }
            Call::ScopeMusic { ev, values, flow } => {
                let values: Vec<&str> = values.iter().map(String::as_str).collect();
                match a.source.prepare_music(ev, &values, flow.as_ref().map(MusicFlow::of)) {
                    Ok(p) => music.push(p),
                    Err(e) => eprintln!("mvs: audio: the music {ev} prepared: {e}"),
                }
            }
            _ => {}
        }
    }
    AudioBegun { glue, calls, scopes, music, known, announcer }
}

/// A job running on the loader, for the match of its key.
pub struct Pending {
    pub key: PrepKey,
    ticket: mvs_load::Ticket<Result<Prepared, String>>,
    started: Instant,
}

impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pending").field("key", &self.key).finish()
    }
}

impl Pending {
    /// `job` started on the loader.
    pub fn start(job: PrepareJob) -> Pending {
        let key = job.key.clone();
        Pending { key, ticket: mvs_load::spawn(move || job.run()), started: Instant::now() }
    }

    /// The job has ended.
    pub fn is_ready(&mut self) -> bool {
        self.ticket.is_ready()
    }

    /// Its parts once it has ended (None: not yet). A job that failed gives Some(None) and says why: the start
    /// then loads on the frame thread, which meets the failure itself.
    pub fn take(&mut self) -> Option<Option<Box<Prepared>>> {
        let r = self.ticket.try_take()?;
        Some(ended(r, self.started))
    }

    /// Waits for its parts (a scripted run, whose frames must not depend on the load's time).
    pub fn wait(self) -> Option<Box<Prepared>> {
        ended(self.ticket.wait(), self.started)
    }
}

/// A job's result, its whole time noted.
fn ended(r: Result<Prepared, String>, started: Instant) -> Option<Box<Prepared>> {
    match r {
        Ok(mut p) => {
            p.laps.push(("prepared", started.elapsed().as_secs_f64() * 1e3));
            Some(Box::new(p))
        }
        Err(e) => {
            eprintln!("mvs: the match's load off the frame thread: {e}");
            None
        }
    }
}
