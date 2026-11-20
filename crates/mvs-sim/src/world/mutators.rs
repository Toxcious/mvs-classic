//! The match's mutators: the list the set-up names, and the world mutators:
//! PandaGameMode InitializeMutators' WorldMutatorHandler_C and the Mutator_Parent children it
//! holds (Mutators/MutatorBuffs/WorldMutatorBuffs), with Apply Mutators To Fighter. Class
//! defaults: the mutators' data, read only in a match with mutators.

use crate::State;
use crate::combat::buffs;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::{latent_frames, warn_text};
use crate::math;
use crate::names::Name;
use crate::setup::{self as S, MatchSetup};
use crate::world::spawn_spot::truthy;
use crate::world::{World, blast, flow, items};
use indexmap::IndexMap;
use mvs_data::value::{Props, Value};

/// Whether a match's mutators can give Mutator_GrowWithDamage: listed, or one of Mutator_RandomMutatorSelector's
/// FighterMutatorMap (class default: BigHeadAbilityBoost, GrowWithDamage).
const GROW_MUTATORS: [&str; 2] = ["Mutator_GrowWithDamage", "Mutator_RandomMutatorSelector"];

/// `growOn`.
pub fn grow_on(setup: &MatchSetup) -> bool {
    setup.mutators.iter().any(|m| GROW_MUTATORS.contains(&m.as_str()))
}

/// `MatchMutators`: the FighterMutators (buff class -> TeamIndex, -1 every team) and the WorldMutators (buff classes,
/// in the list's order).
#[derive(Clone, Debug, Default)]
pub struct MatchMutators {
    pub fighter: IndexMap<Name, i32>,
    pub world: Vec<Name>,
}

/// `initializeMutators`: PandaGameMode InitializeMutators (@171-2390), at the game mode's Init, ahead of every pawn's
/// spawn: each listed MutatorBuffName in order, Map_Find in MutatorData_Map's MutatorMap (@577: a miss goes to the
/// next; a class with no asset is not in `map`, as the failed cast @956 skips it), then IsWorldMutator (@1224) adds
/// the class to WorldMutators (Array_Add @1247: a class listed twice is there twice) or Map_Add(FighterMutators,
/// class, TeamIndex) (@2390: one key per class, the last entry's TeamIndex wins). `world` is the list the
/// WorldMutatorHandler is given (@1316-1971), not spawned yet.
/// Deviation: the build's match config carries a TeamIndex per entry; the set-up's mutators carry names alone, so
/// every fighter mutator is for every team (-1).
pub fn initialize_mutators(w: &World) -> Option<MatchMutators> {
    let m = flow::mutator_data(w)?;
    let mut out = MatchMutators::default();
    for key in &w.setup.mutators {
        let Some(cls) = m.map.get(key).filter(|c| !c.is_empty()) else { continue };
        let n = w.lit(cls);
        if m.world.contains(cls) {
            out.world.push(n);
        } else {
            out.fighter.insert(n, -1);
        }
    }
    Some(out)
}

/// `applyMutatorsToFighter`: PandaGameMode Apply Mutators To Fighter (@588-794), from SpawnCharacter @3834 (behind
/// the arcade buffs, the team index, the passives and the possession's perks: `World::reset`) and RespawnCharacter
/// @1560 (behind BPI_ResetFighter: `flow::respawn`): each FighterMutators class whose TeamIndex is -1 or the pawn's
/// Comp_Actor_Destructible TeamIndex, SpawnBuffActor(class, pawn, pawn, 0 stacks, 0.0 duration, true, false) on the
/// pawn's manager (@794). Not ported: the "MUTATOR FAILED" print and loop exit of a failed class cast (@1044): every
/// map class is a buff.
pub fn apply_mutators_to_fighter(w: &mut World, f: FighterId) {
    let mut i = 0;
    while let Some((cls, t)) = w.fixed.mutators.as_ref().and_then(|m| m.fighter.get_index(i)).map(|(&c, &t)| (c, t)) {
        i += 1;
        if t == -1 || t == w.fighter(f).team.team {
            buffs::spawn_buff(w, f, cls, 0.0, Some(0), Some(f), true, None);
        }
    }
}

/// A world mutator on the handler: plain fields, one shape for every class.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub struct WorldMutator {
    pub cls: Name,
    /// Mutator_RandomMutatorSelector ChosenMutatorString (`Name::NONE` for the other classes).
    pub chosen: Name,
    /// The class's binding to the game mode is live: the selector's ApplyBufff on the SpawnedFighters'
    /// OnFighterReset (BindToPlayerRespawn), Mutator_ShrinkBlastBoxLastPoint's OnPlayerKnockedOut on PlayerRespawned.
    pub bound: bool,
    /// Mutator_ShrinkBlastBoxLastPoint's Delay(ShrinkDelay): the tick it fires, -1 for none.
    pub delay_at: i32,
}

const SELECTOR: &str = "Mutator_RandomMutatorSelector_C";
const ITEMS: &str = "Mutator_RandomItemSpawns_C";
const SHRINK: &str = "Mutator_ShrinkBlastBoxLastPoint_C";

/// The world mutator classes with a port.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Selector,
    Items,
    Shrink,
}

fn class_of(w: &World, cls: Name) -> Option<Class> {
    match w.names.str(cls) {
        SELECTOR => Some(Class::Selector),
        ITEMS => Some(Class::Items),
        SHRINK => Some(Class::Shrink),
        _ => None,
    }
}

/// `defaults`: the class's defaults in the mutators' data.
fn defaults<'a>(w: &'a World, cls: &str) -> Option<&'a Props> {
    flow::mutator_data(w)?.buffs.get(cls)
}

/// A number default, 0 when absent.
fn def_num(w: &World, cls: &str, k: &str) -> crate::Real {
    crate::math::narrow(defaults(w, cls).and_then(|d| d.get(k)).and_then(Value::as_f64).unwrap_or(0.0))
}

/// A TMap of the class defaults as the bind keeps it (`[{ Key, Value }]`, the asset's order: Map_Keys' order).
fn pairs<'a>(w: &'a World, cls: &str, k: &str) -> Vec<(&'a str, &'a str)> {
    let Some(arr) = defaults(w, cls).and_then(|d| d.get(k)).and_then(Value::as_arr) else { return Vec::new() };
    arr.iter()
        .filter_map(Value::as_obj)
        .map(|e| {
            let s = |n: &str| e.get(n).and_then(Value::as_str).unwrap_or("");
            (s("Key"), s("Value"))
        })
        .collect()
}

/// `mapFind`.
fn map_find<'a>(m: &[(&str, &'a str)], key: &str) -> &'a str {
    m.iter().find(|e| e.0 == key).map_or("", |e| e.1)
}

/// The tick a Delay(`ShrinkDelay`) set now fires.
fn shrink_delay_at(w: &World) -> i32 {
    let d = def_num(w, SHRINK, "ShrinkDelay");
    math::to_int32(crate::real(w.flow.tick) + latent_frames(d))
}

/// `beginWorldMutators`: PandaGameMode InitializeMutators @1316-1971, at the game mode's Init: WorldMutators not
/// empty -> one WorldMutatorHandler_C at the identity transform, then SpawnBuffActor(class, handler, handler, 0, 0.0,
/// true, false) on its Comp_State_Manager for each world class in order. Then AllPlayersSpawned (PandaGameMode
/// @1828, behind every SpawnAndInitPlayer): the bindings the mutators made at their BeginPlay.
/// Deviation: `World::reset` spawns the pawns first and runs this at its end, so the Init's draws and spawns come
/// behind the pawns' spawn in the frame, where the build has them ahead (no spawn of the pawns reads them).
pub fn begin_world_mutators(w: &mut World) {
    let classes = w.fixed.mutators.as_ref().map(|m| m.world.clone()).unwrap_or_default();
    if classes.is_empty() {
        w.world_mutators = None;
        return;
    }
    w.world_mutators = Some(flow::WorldMutatorHandler::default());
    for cls in classes {
        spawn_world_mutator(w, cls);
    }
    let mut i = 0;
    while let Some(m) = w.world_mutators.as_ref().and_then(|h| h.buffs.get(i)).copied() {
        if class_of(w, m.cls) == Some(Class::Selector) {
            selector_all_players_spawned(w, m);
        }
        i += 1;
    }
}

/// `spawnWorldMutator`: SpawnBuffActor on the handler's Comp_State_Manager. SweepBuffState refuses a class the
/// handler holds already when its defaults have no CanOverride (Mutator_Parent: none), so a class listed twice, or
/// given by the list and by the selector, is there once. A class with no port here is skipped and warned.
/// Unverified: the new entry joins the list ahead of its BeginPlay (a nested spawn in it comes behind it).
fn spawn_world_mutator(w: &mut World, cls: Name) {
    let Some(k) = class_of(w, cls) else {
        let msg = format!("world mutator {} not ported (skipped)", w.names.str(cls));
        warn_text(w, &msg);
        return;
    };
    if w.world_mutators.as_ref().expect("the handler").buffs.iter().any(|b| b.cls == cls) {
        let name = w.names.str(cls);
        if defaults(w, name).is_some_and(|d| truthy(d, "CanOverride")) {
            let msg = format!("world mutator {name}: CanOverride on the handler not ported (skipped)");
            warn_text(w, &msg);
        }
        return;
    }
    let h = w.world_mutators.as_mut().expect("the handler");
    h.buffs.push(WorldMutator { cls, chosen: Name::NONE, bound: false, delay_at: -1 });
    let i = h.buffs.len() - 1;
    match k {
        Class::Selector => selector_begin(w, i),
        Class::Items => items_begin(w),
        Class::Shrink => w.world_mutators.as_mut().expect("the handler").buffs[i].bound = true,
    }
}

/// Mutator_RandomMutatorSelector BeginPlay @2471, with authority @1471: Map_Keys(FighterMutatorMap),
/// RandomIntegerInRange(0, keys - 1) @1663 (one draw for the match; none for an empty map: RandHelper(0)), the key ->
/// ChosenMutatorString @1764; SetModeTextParams @3192 (ModeText's text = DisplayNameMap[chosen], read at the pop
/// text); ApplyMutatorsToPlayers @1901 and BindToPlayerRespawn @1965 bound to the game mode's AllPlayersSpawned;
/// MulticastApplyWorldMutators @2029 -> @2615: each WorldMutatorMap class on the handler (SpawnBuffActor @456,
/// handler as Owner and Instigator).
// Deviation: the draw is the world's stream, not FMath::RandRange's.
fn selector_begin(w: &mut World, i: usize) {
    let keys: Vec<String> = pairs(w, SELECTOR, "FighterMutatorMap").iter().map(|e| e.0.to_owned()).collect();
    let chosen = if keys.is_empty() {
        Name::NONE
    } else {
        let n = crate::real(u32::try_from(keys.len()).expect("a short map"));
        let j = usize::try_from(math::to_int32(w.random_int(n))).expect("a draw below the bound");
        w.lit(&keys[j])
    };
    let m = &mut w.world_mutators.as_mut().expect("the handler").buffs[i];
    m.chosen = chosen;
    m.bound = true;
    let world: Vec<String> =
        pairs(w, SELECTOR, "WorldMutatorMap").iter().filter(|e| !e.1.is_empty()).map(|e| e.1.to_owned()).collect();
    for cls in world {
        let n = w.lit(&cls);
        spawn_world_mutator(w, n);
    }
}

/// The selector's AllPlayersSpawned: ApplyMutatorsToPlayers @799: for each SpawnedFighters pawn,
/// FighterMutatorMap[chosen] (a miss: nothing) -> MulticastApplyBuff @1310 (SpawnBuffActor(class, the pawn, self, 0,
/// 0.0, false, false) @2681) and MulticastHandlePopText(ModeText) @1373 (UI_Functions PopText @3136: one per pawn).
/// Then BindToPlayerRespawn @2044: ApplyBufff bound to each pawn's OnFighterReset (@2332-2355). The selector actor as
/// the Instigator is no pawn: the buff has no instigating fighter.
fn selector_all_players_spawned(w: &mut World, m: WorldMutator) {
    if !m.bound {
        return;
    }
    let chosen = w.names.str(m.chosen).to_owned();
    let cls = map_find(&pairs(w, SELECTOR, "FighterMutatorMap"), &chosen).to_owned();
    if cls.is_empty() {
        return;
    }
    let text = map_find(&pairs(w, SELECTOR, "DisplayNameMap"), &chosen).to_owned();
    let (cls, text) = (w.lit(&cls), w.lit(&text));
    let (sel, field) = (crate::name!("Mutator_RandomMutatorSelector_C"), crate::name!("ModeText"));
    for f in w.players.clone() {
        buffs::spawn_buff(w, f, cls, 0.0, Some(0), None, false, None);
        w.events.push(Event::MutatorPopText { cls: sel, field, text: Some(text) });
    }
}

/// Mutator_RandomItemSpawns BeginPlay @10 (no authority gate): GetAllActorsOfClass(MapRandomActorSpawnArea)[0], valid
/// @144 -> StartTimeSpawnerServer(Items) @213 (its own 20-entry list) and SpawnCenterActor(BugsBunnyNeutralSafe_C)
/// @313. The spawner is FFAGameMode's (`world::items`), started here at the Init, ahead of the first step.
/// Unverified: the timer set at the Init is taken as pending until the first step's timer pass (a timer set before
/// the manager's tick of a frame), so it counts from the second step.
/// Deviation: in a FreeForAll that also has this mutator (the port's custom lobby: Random Items or Silly in
/// FreeForAll; `flow::ffa_items_on`) the mode's StartNewMatch starts the one spawner and this does nothing, where the
/// build would spawn two centre safes and the mode's list would replace the mutator's 0.2 s later.
fn items_begin(w: &mut World) {
    if flow::ffa_items_on(&w.setup) {
        return;
    }
    let Some(area) = items::actor_of_class(w) else { return };
    if w.item_area.is_none() {
        warn_text(w, "Mutator_RandomItemSpawns: the match has no item area (`itemsOn`)");
        return;
    }
    let list = random_items_list(w);
    let from = crate::real(w.flow.tick + 1);
    items::start_time_spawner_server(w, &list, from);
    let safe = crate::name!("BugsBunnyNeutralSafe_C");
    items::spawn_center_actor(w, &area, safe);
}

/// Mutator_RandomItemSpawns' own Items list (its class defaults), the one StartTimeSpawnerServer takes.
pub fn random_items_list(w: &World) -> Vec<Name> {
    defaults(w, ITEMS)
        .and_then(|d| d.get("Items"))
        .and_then(Value::as_arr)
        .map(|a| a.iter().filter_map(Value::as_str).map(|s| w.lit(s)).collect())
        .unwrap_or_default()
}

/// The fighter mutator classes the match's own mutators give each life: the set-up's list's
/// (`initialize_mutators`) and the Silly selector's pick (its FighterMutatorMap[ChosenMutatorString]).
pub fn match_fighter_classes(w: &World) -> Vec<String> {
    let mut out: Vec<String> = w
        .fixed
        .mutators
        .as_ref()
        .map(|m| m.fighter.keys().map(|&c| w.names.str(c).to_owned()).collect())
        .unwrap_or_default();
    let mut i = 0;
    while let Some(m) = mutator_at(w, i) {
        i += 1;
        if class_of(w, m.cls) == Some(Class::Selector) && m.bound {
            let cls = map_find(&pairs(w, SELECTOR, "FighterMutatorMap"), w.names.str(m.chosen)).to_owned();
            if !cls.is_empty() {
                out.push(cls);
            }
        }
    }
    out
}

/// The handler's entry `i` (a pass walks the live list by index, as a `for of` does).
fn mutator_at(w: &World, i: usize) -> Option<WorldMutator> {
    w.world_mutators.as_ref().and_then(|h| h.buffs.get(i)).copied()
}

fn set_mutator(w: &mut World, i: usize, m: WorldMutator) {
    if let Some(h) = w.world_mutators.as_mut() {
        h.buffs[i] = m;
    }
}

/// `worldMutatorsFighterReset`: a SpawnedFighters pawn's OnFighterReset (BaseFighter ResetFighterLogic @1106), from
/// `flow::respawn` behind its BPI_ResetFighter and ahead of Apply Mutators To Fighter (RespawnCharacter @1560).
/// The selector's ApplyBufff @3362: the pawn's Comp_State_Manager valid -> @2864 FighterMutatorMap[chosen] ->
/// SpawnBuffActor(class, the pawn, self, 0, 0.0, false, false) @3059. No pop text.
pub fn world_mutators_fighter_reset(w: &mut World, f: FighterId) {
    let mut i = 0;
    while let Some(m) = mutator_at(w, i) {
        i += 1;
        if class_of(w, m.cls) != Some(Class::Selector) || !m.bound {
            continue;
        }
        if usize::try_from(w.fighter(f).id).map_or(true, |i| i >= w.players.len()) {
            continue;
        }
        let chosen = w.names.str(m.chosen).to_owned();
        let cls = map_find(&pairs(w, SELECTOR, "FighterMutatorMap"), &chosen).to_owned();
        if !cls.is_empty() {
            let cls = w.lit(&cls);
            buffs::spawn_buff(w, f, cls, 0.0, Some(0), None, false, None);
        }
    }
}

/// `worldMutatorsRespawned`: ParentGameMode PlayerRespawned, broadcast at the end of BPI_RespawnFighter (@26815).
/// Mutator_ShrinkBlastBoxLastPoint OnPlayerKnockedOut @3101 (each respawn): BPI_GetKnockoutsToWin <= 0 -> unbound
/// @3570-3593, never a shrink (a timed match). Else the teams 0..NumTeams - 1 in order, the first whose
/// BPI_GetTeamKnockouts == KnockoutsToWin - 1 (@2744-2879) -> HandleBlastBoxShrink @2889 -> Delay(ShrinkDelay) @299;
/// a Delay node already pending ignores the call.
/// Deviation: under the port's experimental stock rule TeamKnockouts holds lives, where the test has no meaning;
/// there the respawn does nothing and the shrink comes from `world_mutators_stock_life_lost`.
/// Unverified: the Delay made inside the respawn counts from the next frame.
pub fn world_mutators_respawned(w: &mut World, _f: FighterId) {
    let mut i = 0;
    while let Some(mut m) = mutator_at(w, i) {
        i += 1;
        let i = i - 1;
        if class_of(w, m.cls) != Some(Class::Shrink) || !m.bound {
            continue;
        }
        let to_win = w.setup.rules.map_or(0.0, |r| r.knockouts_to_win);
        if to_win <= 0.0 {
            m.bound = false;
            set_mutator(w, i, m);
            continue;
        }
        if w.flow.stocks.is_some() {
            continue;
        }
        for t in 0..usize::from(S::team_count(w.setup.team_style)) {
            if w.flow.team_knockouts.get(t).copied().unwrap_or(0.0) != to_win - 1.0 {
                continue;
            }
            if m.delay_at < 0 {
                m.delay_at = shrink_delay_at(w);
            }
            break;
        }
        set_mutator(w, i, m);
    }
}

/// `worldMutatorsStockLifeLost`: the port's stock rule (no event of the build's, whose mutator hears
/// PlayerRespawned only).
/// Deviation: at each life lost that the match goes on from, a respawn or not, HandleBlastBoxShrink when the match
/// is one ring-out from its end (`flow::stock_match_point`), so a last life lost with no respawn reaches it too.
pub fn world_mutators_stock_life_lost(w: &mut World) {
    let mut i = 0;
    while let Some(mut m) = mutator_at(w, i) {
        i += 1;
        let i = i - 1;
        if class_of(w, m.cls) == Some(Class::Shrink) && m.bound && flow::stock_match_point(w) && m.delay_at < 0 {
            m.delay_at = shrink_delay_at(w);
            set_mutator(w, i, m);
        }
    }
}

/// `worldMutatorsLatent`: the world mutators' Delay nodes, in the world's leftover latent pass (ahead of the item
/// area's). Mutator_ShrinkBlastBoxLastPoint's Delay link @358: BPI_ShrinkBlastBox(ShrinkBlastBoxTime) on every blast
/// box (@827), every CameraCenter_C's AdjustCameraForShrinkingBlastBox(ShrinkBlastBoxTime) (@1208; both
/// `blast::start_blast_shrink`), HandlePopTextService(ShrinkingPopText) @1328 (the `MutatorPopText` event), then
/// OnPlayerKnockedOut unbound from PlayerRespawned @1470: one shrink per match.
/// Unverified: their place in the pass against the game mode's and the area's objects.
pub fn world_mutators_latent(w: &mut World) {
    let mut i = 0;
    while let Some(mut m) = mutator_at(w, i) {
        i += 1;
        let i = i - 1;
        if class_of(w, m.cls) != Some(Class::Shrink) || m.delay_at < 0 || w.flow.tick < m.delay_at {
            continue;
        }
        m.delay_at = -1;
        let time = def_num(w, SHRINK, "ShrinkBlastBoxTime");
        let by = crate::name!("Mutator_ShrinkBlastBoxLastPoint_C");
        blast::start_blast_shrink(w, time, by);
        let field = crate::name!("ShrinkingPopText");
        w.events.push(Event::MutatorPopText { cls: by, field, text: None });
        m.bound = false;
        set_mutator(w, i, m);
    }
}
