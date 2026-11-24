//! The World's set-up and its reset: the constructor's body past the binds, `reset`,
//! `newPawn`, `destroyAllSpawned` and the passive buffs.

use crate::Real;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::combat::{buffs, perks};
use crate::events::Event;
use crate::fighter::common::latent_frames;
use crate::fighter::{self, FighterId, StagePlatform, anim, control, match_side, moves};
use crate::math;
use crate::names::Name;
use crate::registry::ActorHooks;
use crate::setup::{self as S, TeamStyle};
use crate::world::flow::{self, PassiveRow, TimerCmd};
use crate::world::{SpawnPoint, StageCenter, World, blast, items, mutators, pawns, spawning, stage};
use mvs_collision::Ground;
use mvs_data::value::Value;

/// The constructor past the binds (`World::new` made the fighters and the samplers): the stage's geometry and
/// spawns, the fighters' set-up (taunts, team, starting damage, input preferences), the arcade rows, the mutators,
/// the stage's run, `reset` and the tutorial's start.
pub fn construct(w: &mut World) {
    let setup = w.setup.clone();
    let data = w.data.clone();
    let st = &data.stage;
    let mode = setup.mode.is_some();
    {
        let fx = std::sync::Arc::get_mut(&mut w.fixed).expect("the World's own tables at its set-up");
        // (the perks off the walked set-up: the same key in its place, the players' own keys in their order)
        fx.perks = setup
            .players
            .iter()
            .map(|p| p.perks.iter().map(|k| w.names.get(k).expect("a set-up name")).collect())
            .collect();
        fx.stage_center = StageCenter {
            loc: st.center.as_ref().map_or([0.0; 3], |c| {
                [crate::math::narrow(c.loc[0]), crate::math::narrow(c.loc[1]), crate::math::narrow(c.loc[2])]
            }),
            r: st.center.as_ref().map_or(0.0, |c| crate::math::narrow(c.stage_radius)),
        };
    }
    let fl = &st.floor;
    w.stage.ground = Ground {
        left: -(fl.center[1] + fl.half_extent[1]),
        right: -(fl.center[1] - fl.half_extent[1]),
        top: fl.top,
        bottom: fl.center[2] - fl.half_extent[2],
    };
    let bb = &st.blast_boxes["CollisionComp"];
    let v3 = |v: &[f64]| [v[0], v[1], v[2]];
    let ko = blast::KoBox {
        center: v3(&bb.center).map(crate::math::narrow),
        half_extent: v3(&bb.half_extent).map(crate::math::narrow),
        y: [crate::math::narrow(bb.y[0]), crate::math::narrow(bb.y[1])],
        z: [crate::math::narrow(bb.z[0]), crate::math::narrow(bb.z[1])],
    };
    w.stage.ko_box0 = ko;
    w.stage.ko_box = ko;
    w.stage.shrunk = st.blast_shrink.as_ref().map(|s| blast::Shrunk {
        center: v3(&s.center).map(crate::math::narrow),
        half_extent: v3(&s.half_extent).map(crate::math::narrow),
        jump_out_buffer_top: crate::math::narrow(s.jump_out_buffer_top),
    });
    let b = blast::bounds_of(bb);
    w.stage.blast = b;
    w.stage.blast_outer = blast::outer_of(b);
    // UE yaw -90 faces -Y = sim +x.
    let pt = |e: &mvs_data::stage::StageStart| SpawnPoint {
        x: crate::math::narrow(-e.loc[1]),
        y: crate::math::narrow(e.loc[2]),
        facing: if e.yaw < 0.0 { 1.0 } else { -1.0 },
    };
    // ParentGameMode.GetPlayerStart: the first PlayerStart whose PlayerStartTag is "Player<n>" / "Respawn<n>"
    // (n = team-sorted index + 1) and that has the team style's actor tag.
    let tag = S::START_TAGS[setup.team_style as usize];
    let start = |name: &str, list: &[mvs_data::stage::StageStart]| {
        pt(list
            .iter()
            .find(|s| s.tag.as_deref() == Some(name) && s.tags.iter().any(|t| t == tag))
            .unwrap_or_else(|| panic!("the stage has no PlayerStart {name} for {tag}")))
    };
    // Each pawn's Comp_Pawn_TauntManager slots from its player's taunts (not sim state).
    for (i, f) in w.players.clone().into_iter().enumerate() {
        let taunts: Vec<Name> =
            setup.players[i].taunts.iter().map(|k| w.names.get(k).expect("a set-up name")).collect();
        anim::set_taunt_slots(w, f, Some(&taunts));
    }
    // The stage's pass-through platforms (sim x = -Y), one list for every fighter.
    w.stage.stage_platforms = st
        .platforms
        .iter()
        .flatten()
        .enumerate()
        .map(|(i, p)| StagePlatform {
            stage_index: i32::try_from(i).expect("a platform index"),
            left: crate::math::narrow(-p.y[1]),
            right: crate::math::narrow(-p.y[0]),
            top: crate::math::narrow(p.top),
            bottom: crate::math::narrow(p.z[0]),
            ..StagePlatform::default()
        })
        .collect();
    let n_plat = u32::try_from(w.stage.stage_platforms.len()).expect("a platform count");
    // The characters this match's classes need beside its players' (`matchNeeds`): the data's load holds them
    // (`mvs_data`'s match needs), and `setMatch`'s companions are the World's data, which every call is given.
    let teams: Vec<i32> = (0..w.players.len()).map(|i| i32::from(S::player_team(&setup, i))).collect();
    for (i, f) in w.players.clone().into_iter().enumerate() {
        let p = &setup.players[i];
        let fr = w.fighter_mut(f);
        fr.platforms.stage_platforms = (0..n_plat).collect();
        fr.team.team = teams[i]; // AssignPlayerAndTeamIndex_Immediate: PlayerIndex % (2, 2, 4 by team style)
        fr.stats.starting_damage = p.starting_damage.unwrap_or(0.0);
        fr.team.bot_controlled = p.bot;
        fr.controls.bot_controlled = p.bot;
        fr.controls.prefs = S::resolve_input_prefs(p);
    }
    // SpawnCharacter @1955: the arcade row is on the manager before the pawn's SetTeamIndexOnSpawn (reset's
    // passiveBuffs) and its perks (addPerks), and stays on it for every reset.
    let rows = flow::arcade_rows(w, &teams);
    let muts = mutators::initialize_mutators(w);
    {
        let fx = std::sync::Arc::get_mut(&mut w.fixed).expect("the World's own tables at its set-up");
        fx.arcade_rows = rows;
        fx.mutators = muts;
    }
    // Convert Player Index to Team Sorted Index: only the 2v2 style sorts (the players of a team before this one
    // plus the sizes of the earlier teams, then 0, 2, 1, 3); the others use the player index.
    let sorted = |i: usize| -> usize {
        if setup.team_style != TeamStyle::TwoVsTwo {
            return setup.players[i].start.map_or(i, usize::from);
        }
        let t = teams[i];
        let local =
            (0..teams.len()).filter(|&o| teams[o] == t && o < i).count() + teams.iter().filter(|&&o| o < t).count();
        [0, 2, 1, 3][local]
    };
    if !mode {
        w.stage.spawns =
            (0..setup.players.len()).map(|i| start(&format!("Player{}", sorted(i) + 1), &st.spawns)).collect();
        w.stage.respawns =
            (0..setup.players.len()).map(|i| start(&format!("Respawn{}", sorted(i) + 1), &st.respawns)).collect();
    }
    let stride = usize::try_from(used_keys(w).iter().map(|k| data.chars[k.as_str()].boxes.stride).max().unwrap_or(0))
        .expect("a stride");
    w.scratch.buf = vec![0.0; stride];
    w.scratch.vbuf = vec![0.0; stride];
    w.scratch.obuf = vec![0.0; stride];
    // World.stageRun, or none
    stage::make_stage_run(w);
    reset(w);
    match setup.mode {
        Some(S::MatchMode::Tutorial { .. }) => flow::start_tutorial(w), // the tutorial level's begin play
        Some(S::MatchMode::Waves { .. }) => crate::modes::waves::start(w),
        None => {}
    }
}

/// The characters the match uses: the default one, the players', a mode's cast.
fn used_keys(w: &World) -> Vec<String> {
    let mut used = vec![crate::world::DEFAULT_CHARACTER.to_owned()];
    let cast = w.setup.mode.as_ref().map_or_else(Vec::new, |m| m.cast().to_vec());
    for k in w
        .setup
        .players
        .iter()
        .map(|p| p.character.clone().unwrap_or_else(|| crate::world::DEFAULT_CHARACTER.to_owned()))
        .chain(cast)
    {
        if !used.contains(&k) {
            used.push(k);
        }
    }
    used
}

/// A component map's Comp_Pawn_PassiveBuffManager Buffs.
fn buffs_of(m: &mvs_data::fighter::Map<mvs_data::fighter::Component>) -> Option<&[Value]> {
    m.get("Comp_Pawn_PassiveBuffManager_GEN_VARIABLE")
        .and_then(|c| c.p.as_obj())
        .and_then(|p| p.get("Buffs"))
        .and_then(Value::as_arr)
}

/// The fighter's Comp_Pawn_PassiveBuffManager Buffs: the pawn's list, else the base class's.
fn passive_list(w: &World, f: FighterId) -> Vec<PassiveRow> {
    let cs = &w.char_data(f).fighter.components;
    let Some(list) = buffs_of(&cs.pawn).or_else(|| buffs_of(&cs.base)) else { return Vec::new() };
    list.iter()
        .filter_map(Value::as_obj)
        .map(|e| {
            let b = |k: &str| e.get(k).and_then(Value::as_bool).unwrap_or(false);
            let buff = e.get("Buff").and_then(Value::as_str).map_or(Name::NONE, |s| w.lit(s));
            PassiveRow {
                buff,
                buff_self: b("BuffSelf"),
                buff_allies: b("BuffAllies"),
                buff_opponents: b("BuffOpponents"),
                duration: crate::math::narrow(e.get("Duration").and_then(Value::as_f64).unwrap_or(0.0)),
                starting_stacks: crate::math::dbl::to_int32(
                    e.get("StartingStacks").and_then(Value::as_f64).unwrap_or(0.0),
                ),
            }
        })
        .collect()
}

/// `hasBuffActor`: BPI_CheckIfHasBuff(BuffParentActor_C) (Comp_State_Manager CheckHasBuff: any BuffParentActor among
/// the owner's Comp_Actor_Parent.Children). Comp_Pawn_PassiveBuffManager.SpawnPassiveBuffs gives the fighter one buff
/// actor per Buffs entry with BuffSelf (the pawn's list replaces the base class's) at its spawn and at each respawn;
/// Morty's Buff_C019_ApplybuffOnOverlap never destroys itself on a fighter (ubergraph 15..860) and goes with its
/// owner's defeat.
/// Unverified: taken as present while the fighter is not defeated, for every passive buff class (one that ends
/// itself earlier is not told apart), and the timed buff actors the port models are not asked: they add nothing
/// while the passive one is there.
pub fn has_buff_actor(w: &World, f: FighterId) -> bool {
    let id = usize::try_from(w.fighter(f).id).ok();
    w.fighter(f).life.dead == 0.0
        && (passive_list(w, f).iter().any(|r| r.buff_self)
            || id.and_then(|i| w.fixed.arcade_rows.get(i)).is_some_and(|r| !r.is_empty()))
}

/// `passiveBuffs`: Comp_Pawn_PassiveBuffManager SpawnPassiveBuffs(targets) of owner's manager, for the passive
/// classes that are registered buffs (the others stay under `has_buff_actor`'s model): per Buffs entry and target,
/// the owner itself with BuffSelf (@1023), another fighter of its team with BuffAllies (@1592) or of another team with
/// BuffOpponents (@1768) -> SpawnBuffActor(Buff, the target, the owner as Instigator, StartingStacks, Duration)
/// @1259. A target that still has the buff keeps it (SpawnBuffActor's own rules). The data's list, then the rows
/// ApplyArcadeModeBuffs appended (`arcade_rows`).
fn passive_buffs(w: &mut World, owner: FighterId, targets: &[FighterId]) {
    let list = passive_list(w, owner);
    passive_rows(w, owner, targets, &list);
    let id = usize::try_from(w.fighter(owner).id).ok();
    if let Some(extra) = id.and_then(|i| w.fixed.arcade_rows.get(i)).filter(|r| !r.is_empty()).cloned() {
        passive_rows(w, owner, targets, &extra);
    }
}

/// `passiveRows`.
fn passive_rows(w: &mut World, owner: FighterId, targets: &[FighterId], list: &[PassiveRow]) {
    for e in list {
        if e.buff == Name::NONE || !w.registry.buffs.contains(w.names.str(e.buff)) {
            continue;
        }
        for &t in targets {
            let give = if t == owner {
                e.buff_self
            } else if w.fighter(t).team.team == w.fighter(owner).team.team {
                e.buff_allies
            } else {
                e.buff_opponents
            };
            if give {
                buffs::spawn_buff(w, t, e.buff, e.duration, Some(e.starting_stacks), Some(owner), true, None);
            }
        }
    }
}

/// `resetPassives`: a reset fighter's passive buffs. The manager's BeginPlay @162 (authority) binds its owner's
/// Destructible.TeamIndexSetOnSpawn -> ApplyPassiveBuffsToAllFighters (@430 -> @927: SpawnPassiveBuffs of every
/// BP_FighterCharacter) and the game state's OnBaseFighterBeginPlay -> ApplyPassiveBuffsToNewActor (@591 -> @815);
/// SpawnPassiveBuffs binds the TeamIndexSetOnSpawn of a target without TeamIndexWasSetOnSpawn ->
/// ApplyPassiveBuffsAfterTeamIndexSet (@1527 -> @1064: SpawnPassiveBuffs of that target), and never unbinds.
/// BaseFighter ResetFighterLogic broadcasts the pawn's TeamIndexSetOnSpawn (@633), so a reset gives the fighter's
/// own passives to everyone again, and a fighter gets back those of the managers bound to it.
/// Unverified: which managers are bound to a fighter is taken from PandaGameMode SpawnCharacter (BeginPlay inside
/// FinishSpawningActor @1334, SetTeamIndexOnSpawn after it @2317) with the pawns spawned in player order: those of
/// the fighters spawned before it. A buff given while the fighter was defeated stays on the pawn through its reset
/// (kept); the port's spawn drops every buff, so it is given again here.
pub fn reset_passives(w: &mut World, f: FighterId, kept: &[FighterId]) {
    let players = w.players.clone();
    passive_buffs(w, f, &players);
    let id = w.fighter(f).id;
    for o in players {
        if o != f && (w.fighter(o).id < id || kept.contains(&o)) {
            passive_buffs(w, o, &[f]);
        }
    }
}

/// `destroyAllSpawned`: TrainGameMode 'Destroy All Players' (ubergraph 3360, through 'Spawn All Players' @0 ahead of
/// the new pawns' PopulatePlayerDataManager): AbductPlayer on each SpawnedFighters pawn (@1096-1620; the pawn stays
/// until its Delay(1.0), not ported here), then GetAllActorsOfClass(SpawnedActorParent_C) @1636 and K2_DestroyActor
/// on each @1711-2019. UWorld::DestroyActor runs the actor's EndPlay inside the call, with the old pawns still there,
/// so each actor's own end path runs here before the fighters are spawned again: the one the port runs when the
/// actor is destroyed in play (SpawnedActorParent ReceiveEndPlay @15507: DeathVFX and 'Ak Death Location'
/// @17018-18222, then the class's own end; an item's ClearHeldItem and pickup label exit; a marker's owner calls, an
/// accelerator's sounds; a registered actor's `end` hook). DestroySpawnedActor differs from K2_DestroyActor only
/// ahead of the destroy: the wait for the actor's hit pause (DestroyAfterHitPause @206-231) and ReleaseToPool with
/// CanBePooled (@247-275); the EndPlay is the same. Order: GetAllActorsOfClass groups the actors by class, each
/// class's in creation order; here the port's lists in creation order, grouped by class in the order a class first
/// appears.
/// Unverified: the class order (the walk of SpawnedActorParent's derived classes, 1415cd9f0) is not read.
/// Not ported: the EndPlay's SpawnActorAtDeath roll @16070-16684 is not run (Firebolt, C017_CannonShot,
/// ShaggySandwichProjectile), in a nested destroy too: the build's CompSpawnActor goes through the old pawn's
/// Comp_Spawner queue, and that actor (Die With Instigator true for all three) lives until the old pawn's teardown a
/// second later, which the port, whose new pawn is the same Fighter, does not have.
/// Deviation: an actor an end path spawns here (Firebolt's gate wall, ReceiveDestroyed) is not in the build's
/// GetAllActorsOfClass array and lives on, owned by the old pawn; here its own end path runs in a later round of the
/// pass (its end a second early at most when it has Die With Instigator).
/// The old pawns and the new are one Fighter here: while the pass runs no end path hits, buffs, moves or plays a
/// montage on a fighter (`Scratch::reset_pass`), the actors' own effects stay.
fn destroy_all_spawned(w: &mut World) {
    let mut done: Vec<ActorRef> = Vec::new();
    w.scratch.reset_pass = true;
    let mut finished = false;
    for round in 0..8 {
        let mut seq: Vec<(Name, ActorRef)> = Vec::new();
        let mut add = |cls: Name, dead: bool, a: ActorRef| {
            if done.contains(&a) || (round > 0 && dead) {
                return; // a later round: what an end path spawned and left alive
            }
            done.push(a);
            seq.push((cls, a));
        };
        let ac = &w.actors;
        for (id, pr) in ac.projectiles.iter() {
            add(pr.cls, pr.dead, ActorRef::Projectile(id));
        }
        for (id, it) in ac.items.iter() {
            add(it.cls, it.dead, ActorRef::Item(id));
        }
        for (id, m) in ac.markers.iter() {
            add(m.cls, m.dead, ActorRef::Marker(id));
        }
        for (id, mo) in ac.mortars.iter() {
            add(mo.cls, mo.dead, ActorRef::Mortar(id));
        }
        for (id, x) in ac.actors.iter() {
            if !w.registry.actors.get(w.names.str(x.cls)).is_some_and(|e| e.keep_at_lab_reset) {
                add(x.cls, x.dead, ActorRef::Actor(id));
            }
        }
        for (id, ap) in ac.aim_punches.iter() {
            add(ap.cls, ap.dead, ActorRef::AimPunch(id));
        }
        for (id, a) in ac.accelerators.iter() {
            add(a.cls, a.dead, ActorRef::Accelerator(id));
        }
        // the ship's loop ends with its actor
        for (id, sh) in ac.ships.iter() {
            add(sh.cls, sh.dead, ActorRef::Ship(id));
        }
        if seq.is_empty() {
            finished = true;
            break;
        }
        let mut rank: Vec<Name> = Vec::new();
        for e in &seq {
            if !rank.contains(&e.0) {
                rank.push(e.0);
            }
        }
        seq.sort_by_key(|e| rank.iter().position(|&c| c == e.0)); // stable: creation order within a class
        for (_, a) in seq {
            end_on_reset(w, a);
        }
    }
    if !finished {
        crate::fighter::common::warn_text(
            w,
            "Lab reset: end paths still spawning actors after 8 rounds (World.destroyAllSpawned)",
        );
    }
    w.scratch.reset_pass = false;
}

/// `destroyAllSpawned`'s end of one actor.
fn end_on_reset(w: &mut World, a: ActorRef) {
    match a {
        ActorRef::Projectile(pr) => {
            if w.actors.projectiles.get(pr).is_some_and(|p| !p.dead) {
                spawning::destroy_projectile(w, pr, false);
            }
            spawning::projectile_end(w, pr); // the end the frame's list pass runs for a dead one
        }
        ActorRef::Item(it) => {
            if w.actors.items.get(it).is_some_and(|i| !i.dead) {
                items::destroy_item(w, it);
            }
        }
        ActorRef::Marker(m) => {
            if w.actors.markers.get(m).is_some_and(|x| !x.dead) {
                crate::chars::morty::destroy_marker(w, m);
            }
        }
        ActorRef::Mortar(mo) => {
            if let Some(x) = w.actors.mortars.get_mut(mo) {
                x.dead = true;
            }
        }
        ActorRef::Actor(id) => {
            let Some(x) = w.actors.actors.get_mut(id) else { return };
            x.dead = true;
            let cls = x.cls;
            crate::combat::elem::end_actor_children(w, a); // as the frame's list pass runs it
            let end = w.registry.actors.get(w.names.str(cls)).and_then(|e| match e.hooks {
                ActorHooks::World(h) => h.end,
                _ => None,
            });
            if let Some(end) = end {
                end(w, id);
            }
            // (its SpawnedPlatform parts, a fighter's base among them, go with it when it leaves the arena: a gone
            // actor's part reads as dead)
        }
        ActorRef::AimPunch(ap) => {
            if let Some(x) = w.actors.aim_punches.get_mut(ap) {
                x.dead = true;
            }
        }
        ActorRef::Accelerator(ac) => {
            if w.actors.accelerators.get(ac).is_some_and(|x| !x.dead) {
                crate::chars::morty::destroy_accelerator(w, ac);
            }
        }
        ActorRef::Ship(sh) => {
            let Some(s) = w.actors.ships.get(sh) else { return };
            if !s.dead {
                let fighter = w.fighter(s.owner).id;
                let ev = crate::name!("sfx_cha_C019_spaceship_loop_stop");
                w.events.push(Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: None });
            }
            if let Some(s) = w.actors.ships.get_mut(sh) {
                s.dead = true;
            }
        }
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => {}
    }
}

/// `newPawn`: a new pawn's spawn at its point and what its components and BeginPlay give a new pawn (`reset` for
/// the players, `spawnMinion` for a minion).
pub fn new_pawn(w: &mut World, f: FighterId, x: Real, y: Real, facing: Real) {
    // The new pawn's isPushingDown, DownStickStarted and FastFallTriggered (class defaults false), ahead of the
    // stop below: the old pawn's isAttackingFlag ends there must not crouch the new one (AttackEnded @133382).
    {
        let fr = w.fighter_mut(f);
        fr.crouch.pushing_down = false;
        fr.crouch.down_stick = false;
        fr.fast_fall.ff_triggered = false;
        // Deviation: and its BranchManager counts no active attack ahead of the stop, so the old pawn's attack ends
        // there fire no AttackEnded (the pawn's hook, IsAttackingEnd) on what stands for the new pawn: a destroyed
        // pawn's montage ends nothing on the new one. The
        // count goes below 0 at such an end and `reset_match_state` puts it back to 0.
        fr.charge.attack_is_active = 0;
    }
    match_side::spawn(w, f, x, y, facing);
    moves::move_commands_reset(w, f); // a new pawn's Comp_Actor_MoveCommandManager
    {
        let c = &mut w.fighter_mut(f).clock;
        c.creation_time = c.time; // a reset spawns new pawns
    }
    crate::actors::firewall::fire_wall_reset(w, f); // a new pawn's Comp_Pawn_FireWall: an empty FireWallArray
    // ... its Comp_Pawn_HitBoxHandler (`dodgeFxReset`): an empty DodgingFighters and its
    // gates open (the DODGED text's, the dodge burst's, DodgeQuest_Update's: the component's Temp_bool_IsClosed
    // variables).
    if let Some(d) = &mut w.fighter_mut(f).evade.dodge {
        *d = control::DodgeState::default();
    }
    // ... and a new AnimInstance: its run machine at its entry state. A respawn keeps the pawn and its AnimInstance
    // (ResetFighterLogic, BaseFighter @0-1253, touches neither), so `spawn` leaves it.
    w.fighter_mut(f).sm.run_sub = None;
    moves::friction_reset(w, f); // a new pawn's FrictionModifiers and CMC friction values
    moves::fly_reset(w, f); // ... its Comp_Pawn_Fly and the CMC values it writes
    moves::max_accel_reset(w, f); // ... CMC MaxAcceleration 4096
    w.fighter_mut(f).moves.speed_override = None; // ... its Comp_State_Manager: UsingSpeedOverride false
    moves::folds_reset(w, f); // ... the class defaults of the values the buff folds write
    {
        let fr = w.fighter_mut(f);
        fr.armor.timed_armor = None; // ... its Comp_Armor: an empty TimedArmorPack, no ArmorTimer
        fr.react.hit_rotation_off = None; // ... its Base_AnimBP: HitRotationOn true
        // ... and OnEdge false in it and in the pawn, its edge players' times gone
        fr.platforms.on_edge = None;
        fr.sm.edge_times = None;
        // ... the pawn CDO's AllowWallPlay?, DisableDash and WallBounceForceVertMin / WallBounceForcedVertMinAmount
        // (written only by their BPI calls; Comp_JakeEat's BeginPlay writes AllowWallPlay? again in Jake's
        // matchReset below)
        fr.wall.allow_wall_play = None;
        fr.moves.disable_dash = None;
        fr.hit.wall_bounce_force_vert_min = None;
        fr.hit.wall_bounce_forced_vert_min_amount = None;
        // ... its Comp_Pawn_HitBoxHandler: MostRecentHitActor unset
        fr.decay.recent_hit_actor = None;
    }
    moves::input_override_reset(w, f); // ... no input override, the disable counters 0, bCanWalkOffLedges true
    buffs::buff_decay_reset(w, f); // ... an empty BuffDurationDecays and its own 1 s loop
    let abilities = fighter::new_ability_containers(&w.char_data(f).def);
    let pre_spawn = math::to_int32(latent_frames(1.0));
    let rules = w.setup.rules.is_some();
    {
        let fr = w.fighter_mut(f);
        fr.moves.last_jump_v = 0.0; // BaseFighter LastJumpVelocity of a new pawn
        // a new pawn's DamageManager: both DoOnce gates open (@957-982 / @4793)
        fr.charge.charge_hit_small_at = 0.0;
        fr.charge.charge_hit_large_at = 0.0;
        fr.stats.temp_health.clear(); // a new pawn's Comp_Armor has no TempHealthPack
        // A new pawn's Comp_Pawn_AbilityManager: new containers, and CreateAbilities' Delay(1.0) before
        // HandlePreSpawnCooldowns again. The old pawn's cooldowns end with it.
        fr.abilities.abilities = abilities;
        fr.life.pre_spawn_delay = pre_spawn;
        // Its hurt shapes as their components start: none turned off by an UpdateHurtBoxes of the old pawn (a
        // character whose construction turns one off does it again in its matchReset, as Taz's fight cloud).
        fr.hit.hurt_off.clear();
        // A new pawn's Comp_Spawner: ProjectileAimVector zero and no ResetAimTimer (Default__Comp_Spawner_C). The
        // old aimer's EndPlay (spawn's resetAim) acted on the old pawn's component.
        let a = &mut fr.items.proj_aim;
        a.y = 0.0;
        a.z = 0.0;
        a.reset = 0.0;
        a.t = 0.0;
        a.at = 0.0;
        // A new pawn's Comp_Actor_VO: MostRecentVOLineIndex 0, IsCurrentlySpeaking false, PreviousAkEventIDs empty,
        // CurrentAkEventID -1 (the members' defaults; Comp_Actor_VO). The old pawn's line is not taken out of
        // PandaGameState.ActiveVOSpeakers: the game state is the same actor over the Lab's reset (no graph writes the
        // count but BPI_AddVOSpeaker / BPI_RemoveVOSpeaker @14387-14541), the only BPI_RemoveVOSpeaker is
        // VODonePlaying's, and that callback is not run for a destroyed pawn (native:
        // UAkComponent::OnComponentDestroyed cancels the callback package, and a delegate on a pending-kill object is
        // refused). So a pawn reset while its counted line plays leaves the speaker count one high for the rest of
        // the Lab session, as the build does. vo_seq runs on: it only makes the port's sound keys.
        fr.vo.vo_recent = 0.0;
        fr.vo.vo_speaking = false;
        fr.vo.vo_lines.clear();
        fr.vo.vo_current = None;
        // and an empty VO_TagContainer, no RetriggerableDelay pending (the old component's die with it)
        fr.vo.vo_tags.clear();
        fr.vo.vo_enemy_delay = 0.0;
        fr.vo.vo_friend_delay = 0.0;
        fr.stats.damage = fr.stats.starting_damage; // CurrentDamage is a float32
        fr.stats.kos = 0;
        fr.stats.ring_outs = 0;
        // ResetFighterCombatStatTracking zeroes RingOuts, DamageDealt and Assists; OffPlatformRingouts is zeroed with
        // them here (a new match has new player states in the port, a queue set's rematch too).
        // Unverified: whether the build's rematch keeps OffPlatformRingouts across the set's games.
        fr.stats.assists = 0;
        fr.stats.off_platform_ring_outs = 0;
        fr.stats.damage_dealt = 0.0;
        fr.invuln.damage_disabled = false;
        fr.life.dead = 0.0;
        fr.life.abducted = false;
        fr.life.cam_focus = false;
        // ReadyForIntro (a match with a countdown): KeepOnCamera.Unregister, AddInvisibility
        fr.vis.invisible = rules;
        fr.vis.on_camera = !rules;
    }
    control::clear_invuln(w, f);
    match_side::reset_match_state(w, f);
    // Buff_ShaggyPassiveSandwich equips the sandwich at match start; a mode World's pawns get no passive buff (no
    // SetTeamIndexOnSpawn: Comp_Pawn_PassiveBuffManager @738-1527).
    // Unverified: as read, no sandwich on a tutorial Shaggy; not seen in the live game.
    if matches!(w.fighter(f).cs, CharState::Shaggy(_)) {
        let rock = w.char_data(f).def.rock && w.setup.mode.is_none();
        crate::chars::shaggy::state::set_rock(w, f, rock);
        if let CharState::Shaggy(sh) = &mut w.fighter_mut(f).cs {
            sh.hold_pose = false; // ... without BPI_HoldingItem (a new match has a new AnimBP)
        }
    }
    // A new HarleyQuinn_AnimBP instance: the linked layer's Anim_Idle is its default (SetIdleAnim has not run).
    if let Some(hq) = crate::chars::harley::harley_mut(w, f) {
        hq.hold_idle = false;
    }
    crate::combat::hits::hold_charge_begin_play(w, f); // a new pawn's Comp_HoldCharge, ahead of the pawn's BeginPlay
    control::charge_attack_begin_play(w, f); // ... and its Comp_Pawn_ChargeAttack's ChargeMultiplier 1
    if let Some(h) = w.fighter(f).hooks.char.match_reset {
        h(w, f); // CharHooks: the character's own state of a new pawn
    }
}

/// `reset`.
pub fn reset(w: &mut World) {
    destroy_all_spawned(w);
    crate::actors::item::clear_item_queues(w); // the old items' ItemThrowHit / ActivateItem calls go with them
    pawns::drop_pawns(w);
    // The stage solids' boxes are walking boxes for the spawn's floor (`match_side::spawn`): the list ahead of the
    // first frame's (`World::step` sets it each frame).
    if w.stage_run.as_ref().is_some_and(|r| !r.boxes.is_empty()) || w.runtime_solids.is_some() {
        spawning::platforms_of_frame(w);
        let ps = w.scratch.platforms.clone();
        for f in w.players.clone() {
            w.fighter_mut(f).platforms.platforms = ps.clone();
        }
    }
    for (i, f) in w.players.clone().into_iter().enumerate() {
        let s = w.stage.spawns[i];
        new_pawn(w, f, s.x, s.y, s.facing);
    }
    // New pawns: each one's SetTeamIndexOnSpawn in spawn order, with the fighters there by then (the later ones
    // through the bindings, `reset_passives`): every fighter ends with the passives its team and side call for. Then
    // its possession: Comp_Pawn_PerkManager.Initialize -> Add.
    let players = w.players.clone();
    for &f in &players {
        passive_buffs(w, f, &players);
        perks::add_perks(w, f, true);
        mutators::apply_mutators_to_fighter(w, f); // SpawnCharacter @3834, after the possession
        crate::world::wheel::wheel_fighter_spawned(w, f); // the port's own Chaos Wheel: the window's mutator
    }
    let n = usize::from(S::team_count(w.setup.team_style));
    w.flow.team_knockouts = vec![0.0; n];
    w.flow.team_damage = vec![0.0; n];
    w.flow.tied_ffa_teams.clear();
    flow::start_stocks(w);
    w.flow.match_started = false;
    w.flow.match_over = false;
    w.flow.timers.clear();
    w.flow.step_ring_outs = 0; // (the anchors' timers went with the others)
    w.flow.blast_shrink_tick = -1;
    w.flow.blast_shrink_time = None;
    w.flow.blast_shrink_by = None;
    if w.flow.blast_shrink != 0.0 {
        blast::set_blast_shrink(w, 0.0);
    }
    // New pawns and a new NonOverlappingActors: a pawn's capsule begins its overlaps where it spawns.
    for &f in &players {
        let b = blast::blast_overlaps(w, f);
        let l = &mut w.fighter_mut(f).life;
        l.blast_listed = false;
        l.blast_in = b;
    }
    // FFAGameMode SpawnAndInitPlayer (@1038..1346): every pawn's first spawn gives it Buff_FFABonusObjectiveTrackers
    // and ActivateTrackers (@4180), whose SurvivalTimer is a one-shot of SurvivalTime. OnSurvivalTimerComplete
    // (@4490): BPI_ScoreBonusPoint(team, SurvivalPoints, no multiplier), which has no MatchOver gate (PandaGameMode
    // @24841). The timer goes with the buff actor.
    // Unverified: the spawn is taken as the world's first tick.
    let ffa = w.setup.team_style == TeamStyle::FreeForAll && w.setup.rules.is_some_and(|r| !r.stocks);
    w.flow.bonus_trackers =
        players.iter().map(|_| flow::BonusTracker { alive: ffa, spree: 0, spree_done: false }).collect();
    if ffa {
        for &f in &players {
            flow::later(w, S::BONUS_SURVIVAL_TIME, TimerCmd::BonusSurvived { fighter: f }, None);
        }
    }
    crate::world::catch_up::reset(w);
    w.flow.victory_vo = false;
    if w.flow.announcer_until >= 0 {
        w.flow.vo_shared.speakers = (w.flow.vo_shared.speakers - 1).max(0);
    }
    w.flow.announcer_until = -1;
    w.flow.winning_team = -1;
    w.flow.ko_point_multiplier = 1.0;
    // The actors the reset destroyed (`destroy_all_spawned`, at the top) leave the lists, and one an end path spawned
    // goes with them: an id still held (a bot's node memory) reads an invalid actor from then on, as a reference to a
    // destroyed actor does in the build. A new match is a new level; the Lab's reset destroys every
    // SpawnedActorParent_C whatever its 'Die With Instigator' (the projectiles, the items, the ship, the markers,
    // mortar aims, aim punches and accelerators), not with the old pawns' Comp_Spawner ReceiveEndPlay a second later.
    // An actor of `actors` whose kind is no SpawnedActorParent_C child (`keep_at_lab_reset`) stays.
    let (reg, names) = (w.registry, w.names.clone());
    let ac = &mut w.actors;
    ac.actors.retain(|_, x| !x.dead && reg.actors.get(names.str(x.cls)).is_some_and(|e| e.keep_at_lab_reset));
    ac.projectiles.clear();
    ac.items.clear();
    ac.ships.clear();
    ac.markers.clear();
    ac.mortars.clear();
    ac.aim_punches.clear();
    ac.accelerators.clear();
    for &f in &players {
        let fr = w.fighter_mut(f);
        fr.items.spawned_actors.clear();
        fr.shapes.carried = None; // the old pawns go with the reset too
    }
    w.actors.reach = players.iter().map(|_| items::Reach::default()).collect();
    for &f in &players {
        let it = &mut w.fighter_mut(f).items;
        it.item_reachable = false;
        it.pickup_request = false;
        it.block_pick_up = false;
        it.reach_recheck = false;
    }
    if w.stage_run.is_some() {
        stage::stage_reset(w); // StageHooks.reset
    }
    if S::items_on(&w.setup) {
        w.item_area = Some(items::new_item_area(w));
    }
    // InitializeMutators' WorldMutatorHandler and its world mutators (at the game mode's Init), then
    // AllPlayersSpawned.
    mutators::begin_world_mutators(w);
    crate::world::wheel::wheel_reset(w); // the port's own Chaos Wheel: its item window on the new area
}
