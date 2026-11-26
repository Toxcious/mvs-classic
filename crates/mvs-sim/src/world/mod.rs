//! The World: the match's whole state and its step.
//!
//! Every field is here, typed and grouped by subject; what the state record skips (the set-up's
//! fixed tables, the step's scratch, the events) is `#[state(skip)]`. The fighters are one arena (players and pawns
//! alike); `players` are the fighters by player index, `pawns` every fighter of the match.
//!
//! [`World::step`] is the order of calls of one tick. Each call is a function of the module that owns its behaviour:
//! [`flow`], [`blast`], [`spawning`] (the spawn code and the step's spawn passes),
//! [`items`], [`pawns`], [`stage`], [`crate::fighter::tick`] (the
//! fighter's own ticks) and [`crate::combat::phases`] (the hits). A function is empty until its port.

pub mod bind;
pub mod blast;
pub mod catch_up;
pub mod depth3d;
pub mod flow;
pub mod items;
pub mod mutators;
pub mod own_rules;
pub mod pawns;
pub mod profiles;
pub mod reset;
pub mod spawn_spot;
pub mod spawned_platform;
pub mod spawning;
pub mod stage;
pub mod stage_actors;
pub mod stage_query;
pub mod starts;
pub mod wall_bp;
pub mod wheel;
pub mod wheel_events;

pub use blast::{BlastBounds, KoBox, Shrunk};
pub use flow::{BonusTracker, MatchState, Timer, TimerCmd, VoShared};
pub use items::Reach;
pub use stage::StageState;

use crate::Real;
use crate::State;
use crate::actors::lists::{Accelerator, AimPunch, Marker, MortarAim, Ship};
use crate::actors::{Item, Projectile, WorldActor};
use crate::combat::decay::ComboReset;
use crate::combat::{BoxSampler, phases};
use crate::events::Event;
use crate::fighter::{Fighter, FighterId, Hooks, StagePlatform, tick};
use crate::ids::Arena;
use crate::input::{InputState, NO_INPUT, canon_input};
use crate::names::{Name, Names};
use crate::registry::{BindError, Registry, registry};
use crate::rng::Rng;
use crate::setup::MatchSetup;
use crate::world::spawned_platform::{BlockKind, BoxMark};
use indexmap::{IndexMap, IndexSet};
use mvs_collision::Ground;
use mvs_data::GameData;
use std::sync::Arc;

/// The character a player without one plays.
pub const DEFAULT_CHARACTER: &str = "shaggy";

/// A PlayerStart: where a player spawns, and its facing (UE yaw -90 faces -Y, the sim's +x).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct SpawnPoint {
    pub x: crate::Real,
    pub y: crate::Real,
    pub facing: crate::Real,
}

/// PandaGameState CenterStageLocation / StageRadius.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct StageCenter {
    pub loc: [crate::Real; 3],
    pub r: crate::Real,
}

/// The set-up's tables, fixed for the match (the state record skips them): the players' perks, the arcade rows, the
/// mutators, the stage's centre. Their element types come with the ports that read them.
#[derive(Clone, Debug, Default)]
pub struct Fixed {
    /// `perks`: each player's perk buff classes, by player index.
    pub perks: Vec<Vec<Name>>,
    /// `arcadeRows`: each player's ApplyArcadeModeBuffs rows, by player index.
    pub arcade_rows: Vec<Vec<flow::PassiveRow>>,
    /// `mutators`: the match's FighterMutators and WorldMutators, by class name.
    pub mutators: Option<mutators::MatchMutators>,
    pub stage_center: StageCenter,
    /// `samplers`: the box tables by character key (`boxes` is the default character's). Built at the bind for
    /// every character of the match's data (the cast, `matchNeeds`'s keys and a mode's pawns: what a spawn may
    /// reach, as `bornPawn` refuses a key without data). A sampler is the character's data alone, so one made at a
    /// pawn's first spawn would hold the same tables.
    pub samplers: IndexMap<Name, BoxSampler>,
    /// `skelOf`: the skeleton of the box index by character key, for every character of the
    /// match's data whose index has one (made here, not at its first use per index).
    pub skels: IndexMap<Name, Arc<mvs_anim::skel_pose::Skel>>,
}

/// The stage's geometry: the floor, the blast box and the spawn points (fixed but `ko_box` and the shrink, which
/// the FreeForAll shrink moves).
#[derive(State, Debug, Default)]
pub struct Arena2d {
    pub ground: Ground,
    /// `blast`: CollisionComp (hitstun).
    pub blast: BlastBounds,
    /// `blastOuter`: padded (not in hitstun).
    pub blast_outer: BlastBounds,
    pub ko_box: KoBox,
    pub ko_box0: KoBox,
    pub shrunk: Option<Shrunk>,
    pub spawns: Vec<SpawnPoint>,
    pub respawns: Vec<SpawnPoint>,
    /// `stagePlatforms`: the stage's pass-through platforms.
    pub stage_platforms: Vec<StagePlatform>,
}

/// The spawned actors, one arena per list, each in spawn order.
#[derive(State, Debug, Default)]
pub struct Actors {
    pub projectiles: Arena<Projectile>,
    /// `actors`: the registered actor classes.
    pub actors: Arena<WorldActor>,
    pub items: Arena<Item>,
    pub markers: Arena<Marker>,
    pub ships: Arena<Ship>,
    pub mortars: Arena<MortarAim>,
    pub aim_punches: Arena<AimPunch>,
    pub accelerators: Arena<Accelerator>,
    /// `reach`: Comp_ItemManager per player.
    pub reach: Vec<Reach>,
    pub item_seq: i32,
    /// `actorSeq`: the ids of spawned projectiles and actors.
    pub actor_seq: i32,
    /// What `PlatformPart.canPassThrough` reads of a SpawnedPlatformParent child that left its arena while a fighter
    /// still stands on it (the part keeps its actor's `p` and `plat.team`), kept while a fighter's base names
    /// it.
    pub gone_plats: Vec<crate::world::spawned_platform::GonePlat>,
    /// The mortar aims gone from `mortars` that a projectile's or an item's `hit_actors` still names, as they were
    /// at their removal: kept so the per-tick trace's record writes it whole. Nothing in the sim reads
    /// them.
    pub gone_mortars: Vec<(crate::actors::MortarId, crate::actors::lists::MortarAim)>,
    /// The items gone from `items` that a Finn aimer's TeleportActor or a projectile's or an item's `hit_actors` still
    /// names, as they were at their removal (kept so the per-tick trace's record writes it whole). Nothing in
    /// the sim reads them.
    pub gone_items: Vec<(crate::actors::ItemId, crate::actors::item::Item)>,
    /// The world actors gone from `actors` that a projectile's or an item's `hit_actors` still names, as they were at
    /// their removal (kept so the per-tick trace's record writes it whole). Nothing in the sim reads
    /// them.
    pub gone_actors: Vec<(crate::actors::ActorId, crate::actors::world_actor::WorldActor)>,
    /// The boxes of the projectiles gone from `projectiles` that a fighter's base still names, with the projectile's
    /// id, as they ended (kept so the per-tick trace's record writes it whole). Nothing in the sim reads
    /// them.
    pub gone_boxes: Vec<(crate::actors::ProjectileId, i32, Vec<crate::actors::projectile::ProjectileBox>)>,
    /// The place and velocity of the projectiles removed from `projectiles` in the last `GONE_MOTION_TICKS` ticks, as
    /// they ended: a bot's blackboard key holds the actor until the tree writes it again, and the actor
    /// answers GetVelocity and GetActorLocation with its last values (`ai::util::vel` / `place`). The trace record
    /// holds no equivalent (it writes the lists only).
    pub gone_motion: Vec<GoneMotion>,
}

/// How long `Actors::gone_motion` keeps an entry.
// Unverified: a bound of the port's own: BotDetectEnemy refills its keys every 0.5 s +- 0.1 (36 ticks at most), but
// another key can hold a removed actor longer (TomAndJerry_TennisBallInPlay's TennisBall while its branch is
// inactive): past the bound it reads place and velocity (0, 0) where the held actor keeps the last values.
pub const GONE_MOTION_TICKS: i32 = 90;

/// A removed projectile's last place and velocity (sim x / y), and its id.
#[derive(State, Debug)]
pub struct GoneMotion {
    pub id: crate::actors::ProjectileId,
    /// Its id (`seq`): the record writes a reference to it by that id. Nothing in the sim reads it.
    pub seq: i32,
    /// The tick of the removal.
    pub tick: i32,
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
}

/// The step's scratch: written in a step before it is read, never state.
#[derive(Clone, Debug, Default)]
pub struct Scratch {
    /// The step's canonical inputs.
    pub inputs: Vec<InputState>,
    /// Marvin's homing victim: the duration the refreshing spawn passed (RefreshBuff's RefreshDuration), for the
    /// status hook's MulticastUI; set for the length of one `spawn_homing_victim` call only.
    pub homing_refresh_dur: Option<Real>,
    /// `fs`: the fighters of the physics passes, taken once per step ahead of them (`this.pawns.slice()`, else the
    /// players). Nothing writes it until the next step's, so a pawn spawned in a pass is in none of them.
    pub fs: Vec<FighterId>,
    /// `ps` (`Pawns.live`): the fighters of the passes from the owners' requests on, taken once per step after the
    /// first ticks of the pawns spawned in the physics passes; a pawn still in `newPawns` is in none of them.
    pub ps: Vec<FighterId>,
    /// The copy of a list read live (`this.fighters`, `this.all`) while a pass over it runs.
    pub order: Vec<FighterId>,
    /// `buf`, `vbuf`, `obuf`: the pose buffers.
    pub buf: Vec<f32>,
    pub vbuf: Vec<f32>,
    pub obuf: Vec<f32>,
    /// `frameActorSeq`: `actorSeq` at the start of the step.
    pub frame_actor_seq: i32,
    /// `stepAt`: 0 outside a step, 1 up to the fighters' `duringPhysicsTick`, 2 after it, 3 from the projectiles'
    /// pass on.
    pub step_at: u8,
    /// No input this frame (`closed`: `matchInputOpen`).
    pub closed: bool,
    /// `platforms`: the frame's platforms (`spawning::platforms_of_frame`), which every fighter of `fs` takes as
    /// its `platforms`.
    pub platforms: Vec<crate::fighter::PlatformRef>,
    /// `platforms !== this.ships`: the frame's list is `platformsOfFrame`'s, not the ships' alone.
    pub platforms_built: bool,
    /// `resetPass`: inside `destroyAllSpawned`'s rounds.
    pub reset_pass: bool,
    /// The `World::blockers_into` list of the projectiles' and mortars' passes and the items' bodies.
    pub blockers: Vec<mvs_collision::Ground>,
    /// The shape queries' bodies, hits and buffers (`None` while a query has them out, or before the first).
    pub traces: Option<crate::fighter::traces::TraceScratch>,
    /// `floorList`'s list (`fighter::body::floor_list`).
    pub floors: Vec<mvs_collision::Ground>,
    /// `inSink`: the fighter whose Disguise copy's sink is running (`sinkOf`'s calls set and restore it).
    pub copy_sink: Option<FighterId>,
    /// `inSink`: the fighter whose Polymorph's sink is running (`call` sets and restores it).
    pub poly_sink: Option<FighterId>,
    /// `pawnGone`: true inside `spawn`'s buff ends (set and cleared in that one call).
    pub pawn_gone: bool,
    /// `followerTouches`'s shapes, a buffer reused (taken out while the test runs; read through `&World`).
    pub follower_shapes: std::cell::RefCell<Vec<crate::actors::follower::FollowerShape>>,
    /// `elemTick`'s walk over the host's buff serials, a buffer reused (taken out while the walk runs).
    pub elem_serials: Vec<u32>,
    /// `THROW_HITS`: the frame's ItemThrowHit calls of an item whose class overrides it, in call order; the world runs
    /// the class's part behind each caller (`items::item_throw_hits`). Empty between steps (`clearItemQueues`).
    pub throw_hits: Vec<crate::actors::item::ThrowHitCall>,
    /// `USES`: AnimNotify_ItemUse -> Comp_ItemManager.ActivateItem -> BPI_ActivateObject on an item, made by the world
    /// in its spawn pass of the user, ahead of the user's spawn requests. Empty between steps (`clearItemQueues`).
    pub item_uses: Vec<(crate::actors::ItemId, FighterId)>,
    /// `itemBoxes`: the items pass's `blockers('item')` with their marks, and its contacts buffer (`Item::step`).
    pub item_boxes: Vec<crate::actors::item::ItemBox>,
    pub item_contacts: Vec<crate::actors::item::ItemContact>,
    /// The marks of `item_boxes` as `blockers_marked_into` gives them, a buffer kept across steps.
    pub item_marks: Vec<crate::world::spawned_platform::BoxMark>,
    /// The statics a rigid body's step meets (`w.blockers('nav')` with their marks), a buffer kept across steps.
    pub sphere_statics: Vec<mvs_collision::rigid_sphere::Static>,
    /// The buffers of the buff walks, a pool: a walk takes one and hands it back, a walk inside a walk takes another.
    pub buff_refs: Vec<Vec<crate::combat::buffs::BuffRef>>,
    /// The fighter lists of the walks over `matchFighters` / `Pawns.live` (`pawns::match_fighters_list`), a pool.
    pub fighter_lists: Vec<Vec<FighterId>>,
    /// The pose buffers of the box samples (`fighter::anim::sampled_boxes`, `posed_boxes`), a pool; a cell because
    /// the samplers read the World shared.
    pub poses: std::cell::RefCell<Vec<Vec<f32>>>,
    /// The skeleton poses' scratch (`Skel.tmp` and its row buffers), out while a pose is composed.
    pub skel: std::cell::RefCell<Option<crate::fighter::anim::SkelWork>>,
    /// `classTimers`' serials of a fighter's timed buffs.
    pub timer_serials: Vec<u32>,
    /// `matchTick`'s due timers.
    pub timers_due: Vec<crate::world::flow::Timer>,
    /// `SOLIDS`: the stage's solids as `fighter::traces::stage_solids` made them at its first
    /// call with solids; data of the stage alone, kept across loads.
    pub solids: Option<Arc<crate::fighter::traces::Solids>>,
    /// `slotBlendStep`'s marks and lists, reused (taken out while a walk runs: its inputs read the World).
    pub slot_walk: crate::fighter::anim::SlotScratch,
    /// The players of the slot walk running now, as a transition rule reads them (`graph_player_time_from_end`,
    /// `graph_relevant_anim_time_remaining`: the rule reads the one live object the walk updates); the walk takes its
    /// players out of the mix, so it files a copy here before each rule input (into the same lists each time). `.0`
    /// false outside a walk.
    pub live_players: std::cell::RefCell<(bool, crate::fighter::anim::SlotPlayers)>,
    /// The anim code's lists (`fighter::anim::AnimScratch`).
    pub anim: crate::fighter::anim::AnimScratch,
    /// A world actor's `hitShapes` as the barrier's tests read them, a buffer reused (taken out while a test runs;
    /// read through `&World`).
    pub actor_shapes: std::cell::RefCell<Vec<crate::combat::boxes::WorldSphere>>,
    /// The hurt capsules a projectile's box touches (`chars::lebron::ball`'s overlap), a buffer reused.
    pub touched: Vec<Real>,
    /// The fighters' hurt shapes as the step found them, which another actor's overlap meets
    /// (`fighter::anim::struck_boxes`); taken at the step's start, dropped at its end.
    pub struck: Vec<crate::fighter::anim::StruckPose>,
    /// The actor and the instance (its number) whose notifies the sink is taking (`curA`, `curP`).
    pub actor_sink: Option<(crate::actors::ActorRef, u32)>,
    /// `deferred`: the named windows' OnNotifyEnd held while a Stop's OnInterrupted is owed (actor, name, montage).
    pub actor_deferred: Option<Vec<(crate::actors::ActorRef, Name, Name)>>,
    /// The actors' instances a player still runs (a player's object outlives its place), and the ones of them a
    /// call dropped from their place, kept until the run ends (`actors::actor_montage`).
    pub actor_running: Vec<(crate::actors::ActorRef, u32)>,
    pub actor_limbo: Vec<(crate::actors::ActorRef, crate::fighter::MontageState)>,
    /// The fighters' active Hitframes as the hit tests read them (`combat::phases::take_actives`), a pool: a test takes
    /// one and hands it back, a test inside a test takes another.
    pub hit_actives: Vec<crate::combat::phases::Actives>,
    /// The hit shapes a test places (`combat::phases::take_spheres`), a pool as above.
    pub hit_spheres: Vec<Vec<crate::combat::boxes::WorldSphere>>,
    /// The deferred hits of an overlap update (`combat::phases::overlaps`), a pool as above.
    pub pending_hits: Vec<Vec<crate::combat::phases::PendingHit>>,
    /// The packets of the deferred hits, free (`combat::phases::pooled_packet`).
    pub hit_packets: Vec<Box<crate::combat::HitPacket>>,
}

/// `World`.
#[derive(State, Debug)]
pub struct World {
    /// `data`: the match's bound data.
    #[state(skip)]
    pub data: Arc<GameData>,
    /// The names the World's [`Name`]s stand for.
    #[state(skip)]
    pub names: Arc<Names>,
    /// `setup` (without the players' perks, which are `fixed.perks`).
    #[state(skip)]
    pub setup: Arc<MatchSetup>,
    #[state(skip)]
    pub registry: &'static Registry,
    #[state(skip)]
    pub fixed: Arc<Fixed>,

    /// Every fighter: the players, then the pawns in spawn order.
    pub fighters: Arena<Fighter>,
    /// `fighters`: the players, by player index.
    pub players: Vec<FighterId>,
    /// `pawns`: every fighter of the match, made at the first pawn's spawn.
    pub pawns: Option<Vec<FighterId>>,
    pub pawn_seq: Option<i32>,

    pub stage: Arena2d,
    pub actors: Actors,
    pub flow: MatchState,
    /// `comboReset`: the one pending combo-counter reset every fighter shares.
    pub combo_reset: ComboReset,
    /// `frameHits`: the attacker / victim pairs that connected this frame (`hitKey`).
    pub frame_hits: IndexSet<i64>,
    /// True inside a projectile's overlap call made from a fighter's own overlap update.
    pub overlap_mid_frame: bool,
    pub rng: Rng,
    /// UWorld TimeSeconds (GetTimeSeconds): a float32 running sum of DeltaSeconds ([`consts::DELTA_SECONDS`]), one
    /// per world tick from the World's start (a Lab reset keeps it, as the fighters' clocks).
    ///
    /// [`consts::DELTA_SECONDS`]: crate::consts::DELTA_SECONDS
    pub time_seconds: Real,

    /// `stageRun`: the stage's own state, where the stage has some.
    pub stage_run: Option<StageState>,
    /// `itemArea`: FFAGameMode's random items.
    pub item_area: Option<items::ItemArea>,
    /// `worldMutators`: the world mutators' handler.
    pub world_mutators: Option<flow::WorldMutatorHandler>,
    /// `runtimeSolids`: the solids spawned mid-match.
    pub runtime_solids: Option<Vec<stage::RuntimeSolid>>,
    /// `mode` and `tutorial`: the running tutorial's game mode and actor (`modes/tutorial/`), set by its begin
    /// play (`modes::tutorial::run::start_tutorial`).
    pub tutorial: Option<Box<crate::modes::tutorial::TutorialRun>>,
    /// The port's own Waves run (`modes/waves/`), set by `modes::waves::start`.
    pub waves: Option<Box<crate::modes::waves::WavesRun>>,
    /// The port's own Chaos Wheel rule (`world/wheel.rs`), in a match whose set-up has a wheel.
    pub wheel: Option<wheel::WheelState>,
    /// (the port's own, dev) The 3D feel test (`world/depth3d.rs`), switched on by `depth3d::enable`; None: off.
    pub depth3d: Option<depth3d::Depth3d>,
    /// `mode`: the set-up mode's hooks. Skipped from the state: they
    /// are `fn` pointers bound from the set-up's mode at `World::new` and never changed, so a save would copy the
    /// same value.
    #[state(skip)]
    pub mode: Option<pawns::ModeHooks>,

    /// `botTrees`: the bot trees a minion's controller runs, set by the match's set-up (`None`: a World with none,
    /// as a trace's); a controller reads them here, never from `data`.
    #[state(skip)]
    pub bot_trees: Option<Arc<mvs_data::BotTrees>>,
    /// `ACTOR`: the spawned actors' delegates, by the actor; an entry goes at the despawn.
    pub actor_delegates: IndexMap<crate::actors::ActorRef, flow::ActorDelegates>,
    /// `predicted`: a predicted step of a rollback session.
    #[state(skip)]
    pub predicted: bool,
    /// `pawnCtl` and `newPawns`: the pawns' controllers and the pawns spawned in this step.
    #[state(skip)]
    pub pawn_ctl: pawns::PawnControllers,
    /// The step's events, drained by the caller after each step.
    #[state(skip)]
    pub events: Vec<Event>,
    #[state(skip)]
    pub scratch: Scratch,
    /// The messages `warnOnce` wrote (`fighter::common::warn_text`): each goes to stderr once per World. A
    /// diagnostic, not state; a cell because the readers that warn hold the World shared.
    #[state(skip)]
    pub warned: std::cell::RefCell<indexmap::IndexSet<Box<str>>>,
    /// The (check, class) pairs a once-per-class check has run for (`fighter::common::first_check`,
    /// such as the barrier's `UNMET_CHECKED`). A diagnostic, not state.
    #[state(skip)]
    pub checked: std::cell::RefCell<IndexSet<(&'static str, Name)>>,
}

impl World {
    /// `new World(data, seed, setup)`: binds the set-up's names to the registry and the data, then makes the
    /// fighters and the stage. The constructor's body past the binds (the geometry, the spawns, the fighters'
    /// set-up, `reset`, the tutorial's start) is [`flow::construct`]'s.
    pub fn new(data: Arc<GameData>, seed: i32, setup: &MatchSetup) -> Result<World, BindError> {
        let registry = registry();
        let mut names = Names::default();
        let mut keys = Vec::with_capacity(setup.players.len());
        for (i, p) in setup.players.iter().enumerate() {
            let key = p.character.as_deref().unwrap_or(DEFAULT_CHARACTER);
            let asset = || format!("MatchSetup.players[{i}].character");
            if !data.chars.contains_key(key) {
                return Err(BindError { family: "character data", name: key.to_owned(), asset: asset() });
            }
            let hooks = Hooks::bind(registry, key, &asset())?;
            keys.push((names.intern(key), hooks));
        }
        let mut samplers = IndexMap::with_capacity(data.chars.len());
        for (k, cd) in &data.chars {
            let s = BoxSampler::new(&cd.boxes, &mut names).map_err(|e| BindError {
                family: "box index",
                name: k.clone(),
                asset: e,
            })?;
            samplers.insert(names.intern(k), s);
        }
        let mut skels = IndexMap::new();
        for (k, cd) in &data.chars {
            let Some(sk) = &cd.boxes.skel else { continue };
            let clips: Vec<String> = sk.clips.keys().cloned().collect();
            let blend: Vec<String> = sk.blend.keys().cloned().collect();
            let s = mvs_anim::skel_pose::Skel::new(cd.boxes.clone(), sk.clone(), cd.box_data.0.clone(), &clips, &blend)
                .map_err(|e| BindError { family: "box index skel", name: k.clone(), asset: e })?;
            skels.insert(names.intern(k), Arc::new(s));
        }
        bind::intern_all(&mut names, &data);
        bind::intern_setup(&mut names, setup);
        let mode_spawns = setup.mode.is_some();
        let mut fighters = Arena::default();
        let mut players = Vec::new();
        if !mode_spawns {
            for (i, &(ch, hooks)) in keys.iter().enumerate() {
                let rng = Rng::new(crate::math::dbl::to_int32(f64::from(seed) * 7919.0 + i as f64 * 104729.0));
                players.push(fighters.insert(Fighter::new(i32::try_from(i).expect("a player slot"), ch, hooks, rng)));
            }
        }
        let mut w = World {
            data,
            names: Arc::new(names),
            setup: Arc::new(setup.clone()),
            registry,
            fixed: Arc::new(Fixed { samplers, skels, ..Fixed::default() }),
            fighters,
            players,
            pawns: None,
            pawn_seq: None,
            stage: Arena2d::default(),
            actors: Actors::default(),
            flow: MatchState { same_attack_penalty: true, ..MatchState::default() },
            combo_reset: ComboReset { fighter: -1, pending: false, left: 0.0 },
            frame_hits: IndexSet::new(),
            overlap_mid_frame: false,
            rng: Rng::new(crate::math::dbl::to_int32(f64::from(seed) * 31337.0 + 7.0)),
            time_seconds: 0.0,
            stage_run: None,
            item_area: None,
            world_mutators: None,
            runtime_solids: None,
            tutorial: None,
            waves: None,
            wheel: None,
            depth3d: None,
            mode: None,
            predicted: false,
            bot_trees: None,
            actor_delegates: IndexMap::new(),
            pawn_ctl: pawns::PawnControllers::default(),
            events: Vec::new(),
            // (a frame's due timers are a few: the first one due takes no allocation)
            scratch: Scratch { timers_due: Vec::with_capacity(8), ..Scratch::default() },
            warned: std::cell::RefCell::default(),
            checked: std::cell::RefCell::default(),
        };
        // The data part of the Fighter constructor, per player in player order.
        for f in w.players.clone() {
            crate::fighter::set_up(&mut w, f);
        }
        w.wheel = wheel::new_state(setup, seed);
        reset::construct(&mut w);
        Ok(w)
    }

    /// One tick, in the order of the step.
    pub fn step(&mut self, inputs: &[InputState]) {
        let w = self;
        w.scratch.inputs.clear();
        w.scratch.inputs.extend(inputs.iter().map(canon_input));
        w.scratch.frame_actor_seq = w.actors.actor_seq;
        crate::fighter::anim::take_struck_poses(w);
        spawning::morty_lists(w);
        w.scratch.closed = flow::intro_tick(w);
        if w.scratch.closed {
            w.scratch.inputs.fill(NO_INPUT);
        }
        w.flow.tick += 1;
        w.time_seconds += crate::consts::DELTA_SECONDS;
        depth3d::tick(w); // the port's own 3D feel test, when on
        blast::shrink_tick(w);
        items::destroy_pooled(w);
        spawning::ship_ticks(w);
        spawning::platforms_of_frame(w);

        // The fighters' ticks (`fs`: the pawns, else the players).
        take_fs(w);
        pawns::prune_controllers(w);
        w.scratch.step_at = 1;
        each(w, Fs, tick::platforms_of_frame);
        each(w, Fs, flow::bind_ffa_damage);
        each(w, Fs, tick::begin_frame);
        phases::combo_reset_tick(w);
        if w.scratch.platforms_built {
            phases::wallbounce_tick(w);
        }
        phases::clear_frame_hits(w);
        stage::mesh_ticks(w);
        each(w, Fs, tick::set_on_turn);
        stage::pre_tick(w);
        each(w, Fs, tick::controller_tick);
        each(w, Fs, tick::cmc_tick);
        stage::solids_moved(w);
        each(w, Fs, tick::pawn_tick);
        each(w, Fs, tick::clear_on_turn);
        each(w, Fs, tick::mesh_tick);
        tick::follow(w);
        each(w, Fs, tick::during_physics_tick);
        w.scratch.step_at = 2;

        // The owners' requests (`ps`: the live pawns).
        pawns::first_ticks(w, pawns::FirstTicks::OwnerTick);
        take_ps(w);
        each(w, Ps, char_world_tick);
        items::item_manager_tick(w);
        each(w, Ps, pickup_tick);
        each(w, Ps, items::pick_up_request);
        each(w, Ps, items::release_items);
        each(w, Players, flow::respawn_if_owed);
        each(w, Ps, spawning::serve_requests);

        // The actors' passes.
        w.scratch.step_at = 3;
        spawning::projectiles_pass(w);
        spawning::lists_pass(w);
        phases::detect_projectile_hits(w);
        each(w, Ps, phases::buff_move_tick);
        each(w, Ps, items::release_items);
        items::items_pass(w);
        phases::item_spawned_hits(w);
        phases::dodge_ticks(w);

        // The leftover latent pass and the timers.
        each(w, Ps, tick::fire_wall_tick);
        each(w, Ps, tick::reset_aim_timer);
        each(w, Ps, char_timer_tick);
        each(w, Ps, tick::hold_charge_timer);
        each(w, Ps, tick::charge_timer_tick);
        each(w, Ps, tick::jump_hold_timer_tick);
        each(w, Fs, crate::fighter::match_side::vo_tags_latent);
        flow::latent(w);
        each(w, Players, catch_up::latent);
        items::class_timers(w);
        stage::timers(w);
        each(w, Ps, blast::rescue_tick);
        each(w, Ps, blast::check_blast);
        pawns::first_ticks(w, pawns::FirstTicks::Leftover);
        w.scratch.step_at = 0;

        // The frame's tail.
        items::item_uses_unowned(w);
        spawning::end_dead(w);
        flow::match_tick(w);
        items::update_item_reach_all(w);
        spawning::end_projectiles(w);
        each(w, All, pawns::flush_events);
        crate::fighter::anim::drop_struck_poses(w);
    }

    /// The fighter `id`. A stale id is a bug of the caller.
    pub fn fighter(&self, id: FighterId) -> &Fighter {
        self.fighters.get(id).expect("a live fighter id")
    }

    pub fn fighter_mut(&mut self, id: FighterId) -> &mut Fighter {
        self.fighters.get_mut(id).expect("a live fighter id")
    }

    /// `randomInt(n)`.
    pub fn random_int(&mut self, n: crate::Real) -> crate::Real {
        self.rng.int(n)
    }

    /// `randomFloat()`.
    pub fn random_float(&mut self) -> crate::Real {
        self.rng.next()
    }
}

/// `const fs = this.pawns ? this.pawns.slice() : this.fighters`.
fn take_fs(w: &mut World) {
    w.scratch.fs.clear();
    w.scratch.fs.extend_from_slice(w.pawns.as_deref().unwrap_or(&w.players));
}

/// `const ps = Pawns.live(this)`: the pawns but those in `newPawns` (their first tick is still to come), else the
/// players.
fn take_ps(w: &mut World) {
    w.scratch.ps.clear();
    match &w.pawns {
        None => w.scratch.ps.extend_from_slice(&w.players),
        Some(pawns) => {
            let new = &w.pawn_ctl.new_pawns;
            w.scratch.ps.extend(pawns.iter().copied().filter(|&f| !new.iter().any(|n| n.m == f)));
        }
    }
}

/// The fighter list a pass walks.
#[derive(Clone, Copy)]
enum List {
    /// `fs`, as the step took it.
    Fs,
    /// `ps`, as the step took it.
    Ps,
    /// `this.fighters`: the players.
    Players,
    /// `this.all`: the pawns, else the players, as they are at the pass.
    All,
}
use List::{All, Fs, Players, Ps};

/// Runs `f` for every fighter of `list`, in its order, allocating nothing once the scratch has grown. `fs` and
/// `ps` are walked in place (nothing writes them in a step); a live list is copied into `order` first, as a pass
/// may spawn or end a pawn.
fn each(w: &mut World, list: List, f: fn(&mut World, FighterId)) {
    match list {
        Fs | Ps => {
            let mut i = 0;
            loop {
                let ids = if matches!(list, Fs) { &w.scratch.fs } else { &w.scratch.ps };
                let Some(&id) = ids.get(i) else { break };
                f(w, id);
                i += 1;
            }
        }
        Players | All => {
            let mut order = std::mem::take(&mut w.scratch.order);
            order.clear();
            let ids = match list {
                Players => &w.players,
                _ => w.pawns.as_ref().unwrap_or(&w.players),
            };
            order.extend_from_slice(ids);
            for &id in &order {
                f(w, id);
            }
            w.scratch.order = order;
        }
    }
}

/// `CHARS.get(f.ch.key)?.worldTick?.(w, f)`.
fn char_world_tick(w: &mut World, f: FighterId) {
    if let Some(h) = w.fighter(f).hooks.char.world_tick {
        h(w, f);
    }
}

/// `CHARS.get(f.ch.key)?.timerTick?.(w, f)`.
fn char_timer_tick(w: &mut World, f: FighterId) {
    if let Some(h) = w.fighter(f).hooks.char.timer_tick {
        h(w, f);
    }
}

/// `PICKUPS.get(f.ch.key)?.tick?.(w, f)`, bound with the fighter's hooks.
fn pickup_tick(w: &mut World, f: FighterId) {
    if let Some(h) = w.fighter(f).hooks.pickup.and_then(|p| p.tick) {
        h(w, f);
    }
}

// ---- the call surface of the World (its own)

impl World {
    /// `byId`: a fighter of the match by its fighter id (a player's index, a minion's MINION_ID_BASE + n).
    pub fn by_id(&self, id: i32) -> Option<FighterId> {
        if let Ok(i) = usize::try_from(id)
            && let Some(&f) = self.players.get(i)
        {
            return Some(f);
        }
        self.pawns.as_ref()?.iter().copied().find(|&f| self.fighter(f).id == id)
    }

    /// `all`: every fighter of the match (the pawns, else the players), copied: a caller's pass may spawn or end
    /// one.
    pub fn all_ids(&self) -> Vec<FighterId> {
        self.pawns.as_ref().unwrap_or(&self.players).clone()
    }

    /// The [`Name`] of a string of the data or the set-up, which the bind interned (a literal of the code's own is
    /// `name!`).
    pub fn lit(&self, s: &str) -> Name {
        self.names.get(s).unwrap_or_else(|| panic!("{s:?} is not interned: a string of the data or the set-up"))
    }

    /// `f.ch`: the fighter's character data.
    pub fn char_data(&self, f: FighterId) -> &mvs_data::CharData {
        &self.data.chars[self.names.str(self.fighter(f).ch)]
    }

    /// `blockers`: the boxes of object type Platform that answer Block to a query of `kind`: the stage platforms,
    /// the pieces of every live ship with their collision on that answer Block (`chars::morty::ship_boxes`), the
    /// boxes of the spawned platforms and the projectiles (`platform_blockers`), the stage's solids
    /// (`solid_blockers`) and the run-time ones (`runtime_blockers`). Made per call (ships move), into a new list:
    /// a reader of every tick keeps a buffer and calls [`World::blockers_into`].
    pub fn blockers(&mut self, kind: BlockKind) -> Vec<Ground> {
        let mut out = Vec::new();
        self.blockers_into(kind, &mut out);
        out
    }

    /// [`World::blockers`] into `out` (cleared first): the stage platforms, then what the passes add to them. `out`
    /// holds the stage's list from the start, so the passes are given it as their list already made.
    pub fn blockers_into(&mut self, kind: BlockKind, out: &mut Vec<Ground>) {
        self.blockers_walk(kind, out, None);
    }

    /// [`World::blockers_into`] that also gives each box's [`BoxMark`] (`marks`, cleared first, one per box of
    /// `out`): the marks, which the bots' sweep reads.
    pub fn blockers_marked_into(&mut self, kind: BlockKind, out: &mut Vec<Ground>, marks: &mut Vec<BoxMark>) {
        marks.clear();
        self.blockers_walk(kind, out, Some(marks));
    }

    /// [`World::blockers`] that reads the World alone (the view's): the same boxes; the stage solids, where no step
    /// has kept them yet, are made for the call and their boxes not marked (no reader of the list reads a mark).
    pub fn blockers_read(&self, kind: BlockKind) -> Vec<Ground> {
        let mut out = Vec::new();
        self.blockers_read_into(kind, &mut out, None);
        out
    }

    /// The walk of both: `marks`, when given, holds one entry per box of `out` as the passes push. `stageSolids` is
    /// called first (the solids kept, their WorldStatic boxes marked), as the solids' pass would.
    fn blockers_walk(&mut self, kind: BlockKind, out: &mut Vec<Ground>, marks: Option<&mut Vec<BoxMark>>) {
        if self.stage_run.as_ref().is_some_and(|r| !r.solids.is_empty()) {
            crate::fighter::traces::stage_solids(self);
        }
        self.blockers_read_into(kind, out, marks);
    }

    /// The walk over the World as it is.
    fn blockers_read_into(&self, kind: BlockKind, out: &mut Vec<Ground>, mut marks: Option<&mut Vec<BoxMark>>) {
        out.clear();
        out.extend(self.stage.stage_platforms.iter().map(|p| Ground {
            left: f64::from(p.left),
            right: f64::from(p.right),
            bottom: f64::from(p.bottom),
            top: f64::from(p.top),
        }));
        if let Some(m) = marks.as_deref_mut() {
            m.resize(out.len(), BoxMark::Plain);
        }
        let mut o = Some(std::mem::take(out));
        let mut i = 0;
        while let Some(sh) = self.actors.ships.nth(i) {
            i += 1;
            let s = self.actors.ships.get(sh).expect("a live ship id");
            if !s.dead {
                let owner = s.owner;
                let list = o.as_mut().expect("the list");
                crate::chars::morty::ship_boxes(self, sh, kind, list);
                if let Some(m) = marks.as_deref_mut() {
                    m.resize(list.len(), BoxMark::Ship(owner));
                }
            }
        }
        if !self.actors.actors.is_empty() || !self.actors.projectiles.is_empty() {
            o = spawned_platform::platform_blockers_marked(self, kind, &[], o, marks.as_deref_mut());
        }
        // (`solidBlockers` adds nothing and warns of nothing without solids)
        if self.stage_run.as_ref().is_some_and(|r| !r.solids.is_empty()) {
            o = crate::fighter::traces::solids::solid_blockers_read(self, kind, &[], o, marks.as_deref_mut());
        }
        if self.runtime_solids.is_some() {
            o = stage::runtime_blockers(self, kind, &[], o);
            if let (Some(m), Some(list)) = (marks.as_mut(), o.as_ref()) {
                m.resize(list.len(), BoxMark::Plain);
            }
        }
        *out = o.expect("the list");
    }

    /// Whether `blockers` may hold a stage solid of object type WorldStatic (`isStaticSolid`): the stage's solids or
    /// the run-time ones are there. A `Ground` does not carry the mark, so a reader that tells those apart
    /// (a projectile's platforms, a root that does not block WorldStatic) is not ported for them.
    pub fn may_hold_static_solids(&self) -> bool {
        self.stage_run.as_ref().is_some_and(|r| !r.solids.is_empty())
            || self.runtime_solids.as_ref().is_some_and(|r| !r.is_empty())
    }

    /// `sphereTraceMulti` (`capsulesInSphere` over `all`): the fighters whose capsule meets the sphere at (x, y): a
    /// dead one and one with its collision off are left out; the capsule is the segment of its half height less its
    /// radius about the actor's place, met within `radius` plus its radius.
    pub fn sphere_trace_multi(&self, x: Real, y: Real, radius: Real) -> Vec<FighterId> {
        capsules_in_sphere(self, self.pawns.as_deref().unwrap_or(&self.players), x, y, radius)
    }
}

/// `capsulesInSphere`: `World.sphereTraceMulti`'s test over a list of fighters, for a graph the world does not call
/// (a buff actor's).
pub fn capsules_in_sphere(w: &World, fighters: &[FighterId], x: Real, y: Real, radius: Real) -> Vec<FighterId> {
    let mut out = Vec::new();
    for &f in fighters {
        if w.fighter(f).life.dead != 0.0 || crate::fighter::no_collision(w, f) {
            continue;
        }
        let r = crate::fighter::cap_radius(w, f);
        let s = crate::math::max(crate::fighter::capsule_half(w, f) - r, 0.0);
        let cy = crate::fighter::actor_y(w, f);
        let dy = crate::math::max(crate::math::max(cy - s - y, 0.0), y - (cy + s));
        if crate::math::ue::size2d([w.fighter(f).body.x - x, dy]) <= radius + r {
            out.push(f);
        }
    }
    out
}
