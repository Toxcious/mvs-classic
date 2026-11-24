//! The Rust side of the per-tick trace (the crate README): [`frame_of`] writes the World as the oracle writer does
//! (the same bytes), [`load_frame`] sets a World from a frame, [`world_for`]
//! makes the World of a trace's header. Every Rust to oracle mapping of a struct is in one place of this crate; the sim
//! crates know nothing of the trace.

mod actors;
mod arya;
mod barrier;
mod black_adam;
pub mod bot_log;
mod bugs;
mod conv;
pub mod coverage;
mod creature;
mod cx;
mod events;
mod fighter;
mod finn;
mod garnet;
mod gizmo;
mod harley;
pub mod harness;
mod hit;
mod holds;
pub mod inputs;
mod iron_giant;
mod items;
mod jake;
mod lebron;
pub mod machine;
mod marvin;
pub mod matches;
mod morty;
mod moves;
mod mutators;
mod owners;
mod parts;
mod polymorph;
mod rick;
pub mod run;
mod shaggy;
mod shield_actor;
mod stages;
mod steven;
mod stripe;
mod superman;
mod taz;
mod tom_and_jerry;
mod tutorial;
mod velma;
pub mod waves;
pub mod wheel;
mod wonder_woman;
mod world;

use std::sync::Arc;

use mvs_data::GameData;
use mvs_sim::ai::bt::{BtBot, LAB_BOTS};
use mvs_sim::events::Event;
use mvs_sim::setup::{MatchMode, MatchPlayer, MatchSetup, Rules, TeamStyle};
use mvs_sim::{InputState, World};
use mvs_trace::{Frame, Header, InputRecord, WorldRecord};

pub use cx::Book;
pub use owners::{is_owner_prefix, module_of_existing_file, module_of_file, owner, owner_in};

use conv::Rec;
use cx::{Lx, Wx};

/// The stage of a trace match that names none (the recorder's
/// default; a match's `stage` names another, the header's `stage`).
pub const TRACE_STAGE: &str = "trainingroomsmall";

/// A frame as the writer wrote it, and what it could not write.
pub struct Written {
    pub frame: Frame,
    /// The values with a place in the record the writer has no rule for, each named once: written as null or a
    /// stand-in, so the frame is not the oracle one.
    pub unmapped: Vec<String>,
    /// The Rust-only state the record has no place for (the stage's run, the item area, ...), each named once.
    pub unrecorded: Vec<String>,
}

/// A frame as the oracle writer writes it after a step of `w` with `inputs`, `events` the step's.
pub fn frame_of(w: &World, book: &Book, inputs: &[InputState], events: &[Event]) -> Written {
    let x = Wx::new(w, book);
    let mut r = WorldRecord::default();
    world::put(w, &x, &mut r);
    let ids = w.pawns.as_ref().unwrap_or(&w.players);
    r.fighters = ids.iter().map(|&id| fighter::put(w.fighter(id), &x)).collect();
    (r.projectiles, r.actors) = actors::put_all(&x);
    r.events = events.iter().map(|e| events::put(e, &x)).collect();
    let inputs = inputs.iter().map(|i| i.put(&x)).collect();
    let frame = Frame { inputs, world: r };
    Written { frame, unmapped: x.unmapped.into_inner(), unrecorded: x.unrecorded.into_inner() }
}

/// What a load could not set, each named by its record path.
#[derive(Debug, Default)]
pub struct Loaded {
    /// The frame's inputs as the sim takes them.
    pub inputs: Vec<InputState>,
    /// The frame's events as Rust events (those with a Rust form).
    pub events: Vec<Event>,
    pub misses: Vec<String>,
}

/// Sets `w` to the World a frame records: each fighter by its id, the actor lists rebuilt in the record's order, the
/// World's rows and rest. What the record leaves out (the crate README, "What a load leaves") stays as it was.
pub fn load_frame(w: &mut World, book: &Book, frame: &Frame) -> Loaded {
    // A World at a frame's end holds no pending events (the writer took them): what the World held before the load
    // (a fresh World's construction events, a caught step's) is not the frame's and would lead the next step's.
    w.events.clear();
    let ids: Vec<_> = w.fighters.ids().collect();
    for id in ids {
        if let Some(f) = w.fighters.get_mut(id) {
            f.events.clear();
        }
    }
    let mut l = Lx::new(w, book);
    let r = &frame.world;
    if let mvs_trace::Value::Obj(o) = &r.rest
        && let Some(mvs_trace::Value::Obj(s)) = o.get("stageRun")
        && let Some(mvs_trace::Value::List(b)) = s.get("boxes")
    {
        l.stage_boxes.clone_from(b);
    }
    // (the pawns first: an actor of the record can name a mode pawn)
    sync_pawns(w, r, &mut l);
    if let mvs_trace::Value::Obj(o) = &r.rest {
        tutorial::make_parts(w, o, &mut l);
    }
    // (the live ones too: an item that died on the record's tick was live in the World's last load)
    let mut kept_items = std::mem::take(&mut w.actors.gone_items);
    kept_items.extend(w.actors.items.iter().map(|(id, i)| (id, i.clone())));
    actors::load_all(w, &r.projectiles, &r.actors, &mut l);
    l.index(w);
    let ids: Vec<_> = w.pawns.clone().unwrap_or_else(|| w.players.clone());
    if ids.len() != r.fighters.len() {
        l.miss(format_args!("{} fighters in the record, {} in the World", r.fighters.len(), ids.len()));
    }
    for (i, (id, fr)) in ids.iter().zip(&r.fighters).enumerate() {
        l.at(&format!("fighters[{i}]"), |l| fighter::load(w.fighter_mut(*id), fr, l));
    }
    for id in ids.iter().copied() {
        fighter::resolve_data_packs(w, id, &mut l);
    }
    w.actors.gone_boxes = std::mem::take(&mut l.gone_boxes);
    // Unverified: the record holds no `team` or drop-through answers for a gone box (`GonePlat`): left as a plain
    // box's (team 0, no drop through).
    w.actors.gone_plats = std::mem::take(&mut l.gone_plats)
        .into_iter()
        .map(|(id, parts)| mvs_sim::world::spawned_platform::GonePlat {
            id,
            team: 0,
            allies: false,
            enemies: false,
            parts,
        })
        .collect();
    world::load(w, r, &mut l);
    keep_reached_items(w, kept_items, &mut l);
    let inputs = l.at("inputs", |l| frame.inputs.iter().map(|i| InputState::make(i, l).unwrap_or_default()).collect());
    let events = l.at("events", |l| {
        frame
            .world
            .events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| l.at(&format!("[{i}]"), |l| events::make(e, l)))
            .collect()
    });
    w.names = Arc::clone(&l.names);
    Loaded { inputs, events, misses: l.misses }
}

/// The gone items a reach list of the record still names (a destroy while its fighter's BlockPickUp was set): the
/// record names each by its id alone (`spawned`), so the copy is the one the World kept or had live before the load.
/// One the World never had (the oracle destroyed what the port had not) is a miss and leaves the list.
fn keep_reached_items(
    w: &mut World,
    mut kept: Vec<(mvs_sim::actors::ItemId, mvs_sim::actors::item::Item)>,
    l: &mut Lx,
) {
    use mvs_sim::actors::ActorRef;
    let mut keep = |w: &mut World, list: &mut Vec<ActorRef>, closest: &mut Option<ActorRef>, l: &mut Lx| {
        let mut ok = |w: &mut World, r: ActorRef, l: &mut Lx| {
            let ActorRef::Item(id) = r else { return true };
            if w.actors.items.get(id).is_some() || w.actors.gone_items.iter().any(|(g, _)| *g == id) {
                return true;
            }
            match kept.iter().position(|(g, _)| *g == id) {
                Some(i) => {
                    w.actors.gone_items.push(kept.swap_remove(i));
                    true
                }
                None => {
                    l.miss(format_args!("a reach list names item {id:?}, gone and never kept by the World"));
                    false
                }
            }
        };
        list.retain(|r| ok(w, *r, l));
        if let Some(c) = *closest
            && !ok(w, c, l)
        {
            *closest = None;
        }
    };
    for i in 0..w.actors.reach.len() {
        let (mut list, mut closest) = (std::mem::take(&mut w.actors.reach[i].items), w.actors.reach[i].closest);
        keep(w, &mut list, &mut closest, l);
        w.actors.reach[i].items = list;
        w.actors.reach[i].closest = closest;
    }
    let ids: Vec<_> = w.fighters.ids().collect();
    for f in ids {
        let Some(r) = w.fighter_mut(f).items.item_reach.as_mut() else { continue };
        let (mut list, mut closest) = (std::mem::take(&mut r.items), r.closest);
        keep(w, &mut list, &mut closest, l);
        if let Some(r) = w.fighter_mut(f).items.item_reach.as_mut() {
            r.items = list;
            r.closest = closest;
        }
    }
}

/// The World's pawns made those of the record: a pawn past the players the record has not is out of the list (as a
/// destroyed minion: still in the arena, dead), one the record has and the World not is taken back from the arena by
/// its id or made as a new minion of its `minionCreator` (`spawn_minion`, its id from the pawns' counter set to it),
/// and the list follows the record's order. The fighter's own load then sets its state.
// Unverified: a minion made here has a controller with no tree (the record names none), and one taken back from the
// arena none at all: a step from such a frame runs no bot for it.
fn sync_pawns(w: &mut World, r: &mvs_trace::WorldRecord, l: &mut Lx) {
    let n = w.players.len();
    if w.pawns.is_none() && r.fighters.len() == n {
        return;
    }
    let mut want: Vec<mvs_sim::fighter::FighterId> = w.players.clone();
    for (i, fr) in r.fighters.iter().enumerate().skip(n) {
        let Some(id) = conv::int(fr.id, l) else { continue };
        if let Some((m, _)) = w.fighters.iter().find(|(_, f)| f.id == id) {
            want.push(m);
            continue;
        }
        let at = format!("fighters[{i}]");
        let creator = match &fr.rest {
            // (a fighter's reference: `{ id, ref: "fighter" }`)
            mvs_trace::Value::Obj(o) => match o.get("minionCreator") {
                Some(mvs_trace::Value::Obj(c)) => match c.get("id") {
                    Some(mvs_trace::Value::Num(x)) => Some(*x),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        let Some(owner) = creator.and_then(|c| l.at(&at, |l| l.fighter(c))) else {
            // A pawn with no creator is a mode's own (the tutorial's `spawnFighter`, a Waves run's): its slot and team are the
            // record's.
            if w.tutorial.is_some() || w.waves.is_some() {
                want.push(mode_pawn(w, id, fr, l));
                continue;
            }
            l.at(&at, |l| l.miss("a pawn past the players with no minion creator (no rule makes it)"));
            continue;
        };
        let character = l.name(&fr.character);
        w.pawn_seq = Some(id - mvs_sim::world::pawns::MINION_ID_BASE);
        let m = mvs_sim::world::pawns::spawn_minion(
            w,
            mvs_sim::world::pawns::MinionSpawn {
                character,
                owner,
                x: mvs_sim::math::narrow(fr.x),
                y: mvs_sim::math::narrow(fr.y),
                facing: mvs_sim::math::narrow(fr.facing),
                tree: None,
                at: mvs_sim::world::pawns::FirstTicks::OwnerTick,
            },
        );
        want.push(m);
    }
    let gone: Vec<_> = w.pawns.iter().flatten().copied().filter(|m| !want.contains(m)).collect();
    for m in gone {
        w.pawn_ctl.ctl.shift_remove(&m);
    }
    w.pawns = Some(want);
    l.index(w);
}

/// A mode pawn of the record (`run::spawn_fighter`: `spawn_pawn`, `bind_slot`, the team): its id from the pawns'
/// counter set to it. The fighter's own load then sets its state.
fn mode_pawn(w: &mut World, id: i32, fr: &mvs_trace::FighterRecord, l: &mut Lx) -> mvs_sim::fighter::FighterId {
    use mvs_sim::world::pawns;
    let character = l.name(&fr.character);
    w.pawn_seq = Some(id - pawns::MINION_ID_BASE);
    let m = pawns::spawn_pawn(
        w,
        pawns::PawnSpawn {
            character,
            x: mvs_sim::math::narrow(fr.x),
            y: mvs_sim::math::narrow(fr.y),
            facing_left: fr.facing < 0.0,
            at: pawns::FirstTicks::Leftover,
        },
    );
    // (the SlotBound event is the spawn frame's: a loaded World holds none)
    let events = w.events.len();
    let slot = match &fr.rest {
        mvs_trace::Value::Obj(o) => match o.get("slot") {
            Some(mvs_trace::Value::Num(s)) => conv::int(*s, l),
            _ => None,
        },
        _ => None,
    };
    if let Some(s) = slot {
        pawns::bind_slot(w, m, s);
    }
    w.events.truncate(events);
    if let Some(t) = conv::int(fr.team, l) {
        w.fighter_mut(m).team.team = t;
    }
    m
}

/// An input record as the sim's input (`InputState`), its unset presses left default.
pub fn input_of(r: &InputRecord, w: &World, book: &Book) -> InputState {
    let mut l = Lx::new(w, book);
    InputState::make(r, &mut l).unwrap_or_default()
}

/// The match set-up a header holds (`MatchSetup` as the oracle writes it: `teamStyle`, `players`, `rules`, `flow`,
/// ...).
pub fn setup_of(v: &serde_json::Value) -> Result<MatchSetup, String> {
    use serde_json::Value as J;
    let o = v.as_object().ok_or("the set-up is not an object")?;
    // UMatchConfig's default: an absent allowHazards is true (the oracle's `setup.allowHazards ?? true`).
    let mut s = MatchSetup { allow_hazards: true, ..MatchSetup::default() };
    for (k, v) in o {
        match k.as_str() {
            "teamStyle" => {
                s.team_style = match v.as_u64() {
                    Some(0) => TeamStyle::OneVsOne,
                    Some(1) => TeamStyle::TwoVsTwo,
                    Some(2) => TeamStyle::FreeForAll,
                    _ => return Err(format!("teamStyle {v}")),
                }
            }
            "players" => {
                for p in v.as_array().ok_or("players is not a list")? {
                    s.players.push(player_of(p)?);
                }
            }
            "rules" => {
                let r = v.as_object().ok_or("rules is not an object")?;
                let num = |k: &str| r.get(k).and_then(J::as_f64).ok_or(format!("rules.{k}"));
                for k in r.keys() {
                    if !["knockoutsToWin", "duration", "stocks"].contains(&k.as_str()) {
                        return Err(format!("rules.{k} (no rule to read it)"));
                    }
                }
                s.rules = Some(Rules {
                    knockouts_to_win: mvs_sim::math::narrow(num("knockoutsToWin")?),
                    duration: mvs_sim::math::narrow(num("duration")?),
                    stocks: r.get("stocks").and_then(J::as_bool).unwrap_or(false),
                });
            }
            "flow" => s.flow = v.as_bool().ok_or("flow")?,
            "allowHazards" => s.allow_hazards = v.as_bool().ok_or("allowHazards")?,
            "localPlay" => s.local_play = v.as_bool().ok_or("localPlay")?,
            "mutators" => {
                s.mutators = v
                    .as_array()
                    .ok_or("mutators")?
                    .iter()
                    .map(|m| m.as_str().map(str::to_owned).ok_or("a mutator"))
                    .collect::<Result<_, _>>()?;
            }
            "mode" => s.mode = Some(mode_of(v)?.0),
            "wheel" => s.wheel = Some(wheel::setup_of(v)?),
            k => return Err(format!("the set-up's {k} (no rule to read it)")),
        }
    }
    Ok(s)
}

/// A set-up's `mode` (`record::TutorialMode`, `record::WavesMode`): the sim's, and the actor's tutorial data where the
/// header has it.
fn mode_of(v: &serde_json::Value) -> Result<(MatchMode, Option<mvs_data::tutorial::TutorialActorData>), String> {
    if v.get("kind").and_then(serde_json::Value::as_str) == Some("waves") {
        let m: mvs_trace::WavesMode = serde_json::from_value(v.clone()).map_err(|e| format!("mode: {e}"))?;
        let variant = match m.variant.as_str() {
            "ringOuts100" => mvs_sim::setup::WavesVariant::RingOuts100,
            "endless" => mvs_sim::setup::WavesVariant::Endless,
            s => return Err(format!("mode.variant {s} (no rule to read it)")),
        };
        return Ok((MatchMode::Waves { variant, cast: m.cast }, None));
    }
    let m: mvs_trace::TutorialMode = serde_json::from_value(v.clone()).map_err(|e| format!("mode: {e}"))?;
    if m.kind != "tutorial" {
        return Err(format!("mode.kind {} (no rule to read it)", m.kind));
    }
    let data = match m.data {
        Some(d) => Some(serde_json::from_value(d).map_err(|e| format!("mode.data: {e}"))?),
        None => None,
    };
    Ok((MatchMode::Tutorial { actor: m.actor, cast: m.cast }, data))
}

fn player_of(v: &serde_json::Value) -> Result<MatchPlayer, String> {
    let o = v.as_object().ok_or("a player is not an object")?;
    let mut p = MatchPlayer::default();
    let strs = |v: &serde_json::Value, k: &str| -> Result<Vec<String>, String> {
        v.as_array()
            .ok_or(format!("{k} is not a list"))?
            .iter()
            .map(|s| s.as_str().map(str::to_owned).ok_or(format!("{k}: not a string")))
            .collect()
    };
    for (k, v) in o {
        match k.as_str() {
            "bot" => p.bot = v.as_bool().ok_or("bot")?,
            "character" => p.character = Some(v.as_str().ok_or("character")?.to_owned()),
            "difficulty" => p.difficulty = Some(mvs_sim::math::narrow(v.as_f64().ok_or("difficulty")?)),
            "perks" => p.perks = strs(v, "perks")?,
            "skin" => p.skin = Some(v.as_str().ok_or("skin")?.to_owned()),
            "taunts" => p.taunts = strs(v, "taunts")?,
            "team" => p.team = Some(u8::try_from(v.as_u64().ok_or("team")?).map_err(|_| "team")?),
            k => return Err(format!("a player's {k} (no rule to read it)")),
        }
    }
    Ok(p)
}

/// The keys of the characters a set-up plays (the default character's where a player names none), then a mode's
/// cast (the World's `used`).
pub fn keys_of(s: &MatchSetup) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    let cast = s.mode.as_ref().map_or(&[][..], MatchMode::cast);
    for k in s
        .players
        .iter()
        .map(|p| p.character.clone().unwrap_or_else(|| mvs_sim::world::DEFAULT_CHARACTER.to_owned()))
        .chain(cast.iter().cloned())
    {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

/// A tutorial trace's setup with the characters its demo controllers' and minigames' actors are drawn with
/// (`classes::drawn_with`: Rick for Tutorial_Meeseeks) added to the mode's cast, as the app's tutorial cast has them;
/// the oracle's traces record the cast without them. For a replay or a picture: their assets load with the match's
/// views, and no sim step reads the cast past the data's load.
// Deviation: appended after the recorded cast, where the app's tutorial cast is in roster order (nothing reads the
// order).
pub fn drawn_with_cast(setup: &mut serde_json::Value) {
    let Some(mode) = setup.get_mut("mode") else { return };
    let Some(data) = mode.get("data").cloned() else { return };
    let Ok(d) = serde_json::from_value::<mvs_data::tutorial::TutorialActorData>(data) else { return };
    let Some(cast) = mode.get_mut("cast").and_then(serde_json::Value::as_array_mut) else { return };
    for s in &d.steps {
        for cls in s.demo.iter().chain(s.tasks.iter().flat_map(|t| t.custom_actors.iter())) {
            for k in mvs_sim::modes::tutorial::classes::drawn_with(cls) {
                if !cast.iter().any(|c| c.as_str() == Some(k)) {
                    cast.push(serde_json::Value::from(k));
                }
            }
        }
    }
}

/// The calls a tutorial trace makes before frame `frame`'s step (the header's `calls`), in their order.
pub fn page_calls(w: &mut World, header: &Header, frame: usize) {
    use mvs_sim::modes::tutorial::run;
    use mvs_trace::TutorialCall as C;
    for &(_, c) in header.calls.iter().filter(|(at, _)| usize::try_from(*at).is_ok_and(|a| a == frame)) {
        match c {
            C::IntroComplete => run::intro_complete(w),
            C::StartPressed => run::start_pressed(w),
            C::Advance => run::advance(w),
            C::SkipStep => run::skip_step(w),
            C::SkipToEnd => run::skip_to_end(w),
            C::DesaturateEnvironment => run::desaturate_environment(w),
            C::StartNext => run::start_next(w),
        }
    }
}

/// The store kinds only the oracle's page drew or played from: no sim reads them, so a trace's header is not compared
/// on them (a renderer kind's bump leaves the traces valid). The native `stage` kind holds the items' draws alone (the
/// sim reads the stages from the gamedata kind), and its version is not the oracle's.
const DRAWN_KINDS: &[&str] = &[
    "atmosphere",
    "banner",
    "drawmat",
    "font",
    "image",
    "lighting",
    "material",
    "menuscene",
    "mesh",
    "particles",
    "picture",
    "program",
    "scenery",
    "shader",
    "shadermap",
    "sky",
    "sound",
    "soundbank",
    "soundset",
    "stage",
    "stagedraw",
    "stagelight",
    "text",
    "texture",
    "tonemap",
    "ui",
];

/// Where the store differs from the one a trace's header names (`store`: its build id and each kind's version), as
/// `trace-diff` checks two headers: a trace of other data is no check of the sim.
pub fn store_differences(header: &Header, store: &mvs_store::Store) -> Vec<String> {
    let mut out = Vec::new();
    let m = store.manifest();
    match header.store.get("buildId").and_then(serde_json::Value::as_str) {
        Some(b) if b == m.build_id => {}
        b => out.push(format!("store.buildId: the trace's {b:?}, the store's {:?}", m.build_id)),
    }
    let kinds = header.store.get("kinds").and_then(serde_json::Value::as_object);
    for (k, v) in kinds.into_iter().flatten().filter(|(k, _)| !DRAWN_KINDS.contains(&k.as_str())) {
        let ours = store.version(k);
        // the gamedata kind by its format alone: the native importer's version folds its code's versions above the
        // format, which the oracle's (and the traces' headers) hold alone; the native pack's entries equal the oracle's
        let same = if k == mvs_data::KIND {
            v.as_u64().map(mvs_data::format_of) == ours.map(mvs_data::format_of)
        } else {
            v.as_u64() == ours
        };
        if !same {
            out.push(format!("store.kinds.{k}: the trace's {v}, the store's {ours:?}"));
        }
    }
    if kinds.is_none() {
        out.push("store.kinds: the trace's header has none".to_owned());
    }
    out
}

/// The World a trace's header was made with (`new World(data, header.worldSeed, header.setup)`), its data from
/// `store`.
pub fn world_for(header: &Header, store: &mvs_store::Store) -> Result<(World, Book), String> {
    let setup = setup_of(&header.setup)?;
    let keys = keys_of(&setup);
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let stage = header.stage.as_deref().unwrap_or(TRACE_STAGE);
    let mut data = GameData::load(store, &refs, stage).map_err(|e| format!("the gamedata kind: {e:?}"))?;
    // A tutorial header's actor data: the World's tutorials (a store without the tutorial entry has none).
    if let Some(m) = header.setup.get("mode")
        && let (_, Some(d)) = mode_of(m)?
    {
        data.tutorials.insert(d.actor.clone(), d);
    }
    let book = Book::new(&data);
    // (the oracle's recorder sets World.botTrees when a player is a bot or the match's spec asks for it: a minion's
    // controller runs its tree by name)
    // A Waves run's enemies are bots: its World always holds them.
    let waves = matches!(setup.mode, Some(MatchMode::Waves { .. }));
    let bot_trees = (setup.players.iter().any(|p| p.bot) || header.spec["botTrees"].as_bool() == Some(true) || waves)
        .then(|| Arc::new(data.bot_trees.clone()));
    #[allow(clippy::cast_possible_truncation)]
    let seed = header.world_seed as i32;
    let mut w = World::new(Arc::new(data), seed, &setup).map_err(|e| format!("the bind: {e:?}"))?;
    w.bot_trees = bot_trees;
    Ok((w, book))
}

/// The match's player bots, one slot per player, made as the app does: a Lab bot where the
/// header's spec names a LAB_BOTS entry for the player (`labBots`, seed 12345), else a match bot at the player's
/// difficulty (seed 12345 + the slot). The World holds the trees ([`world_for`]).
pub fn bots_for(spec: &serde_json::Value, w: &World) -> Vec<Option<BtBot>> {
    let Some(trees) = &w.bot_trees else { return vec![None; w.setup.players.len()] };
    w.setup
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| {
            // (a mode World's players are its pawns: a bot among them is the mode's own controller)
            if !p.bot || w.setup.mode.is_some() {
                return None;
            }
            let seed = i32::try_from(i).expect("a few players");
            Some(match spec["labBots"][i].as_u64() {
                Some(e) => BtBot::lab_bot(12345, trees, &LAB_BOTS[usize::try_from(e).expect("an entry")]),
                #[allow(clippy::cast_possible_truncation)]
                None => BtBot::match_bot(12345 + seed, trees, p.difficulty.unwrap_or(0.0) as i32),
            })
        })
        .collect()
}

/// The Expert bot's lead by name, for the arena and `expert-data`: `x4` the hand-made shortlist, `net` the network
/// (the built-in weights, or the weights file `net`).
pub fn expert_lead(s: &str, net: Option<&str>) -> Result<mvs_sim::ai::expert::Lead, String> {
    use mvs_sim::ai::expert::{Lead, net::Net, net::built_in};
    match s {
        "x4" => Ok(Lead::Shortlist),
        "net" => Ok(Lead::Net(match net {
            None => built_in(),
            Some(f) => {
                let b = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
                std::sync::Arc::new(Net::from_bytes(&b).map_err(|e| format!("{f}: {e}"))?)
            }
        })),
        _ => Err(format!("an Expert lead is x4 or net, not {s}")),
    }
}
