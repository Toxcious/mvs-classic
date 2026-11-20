//! The pawns past the players: the minions and a mode's pawns, their controllers and their first ticks
//! (the pawn and minion code).
//!
//! A minion: a fighter of a given character spawned mid-match by another one (UAIBlueprintHelperLibrary::
//! SpawnAIFromClass, Pawn_StevenUniverse @3572), outside the players' list. A mode's pawn: a fighter a game mode
//! spawns itself (the tutorial's player fighter, its bags, its demo fighters), apart from the player slots: a slot is
//! a player data (`MatchSetup::players`, by index), a pawn a fighter the mode spawns, binds to a slot, possesses and
//! destroys while the match runs. A match whose set-up has a `mode` starts with no fighters and spawns every pawn
//! through these calls.

use crate::Real;
use crate::ai::bt::BtBot;
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::{Fighter, FighterId, Hooks, tick};
use crate::input::{InputState, NO_INPUT, canon_input, held};
use crate::names::Name;
use crate::rng::Rng;
use crate::setup::InputPrefs;
use crate::world::World;
use indexmap::IndexMap;

/// `ModeHooks`: what a mode's own code does at the World's points: `round_end` at a slot pawn's
/// blast exit; `latent` in the frame's leftover latent pass, ahead of the stage's: the mode's actors' and
/// controllers' Delays, latent moves and timers.
#[derive(Clone, Copy, Debug)]
pub struct ModeHooks {
    pub round_end: fn(&mut World, FighterId),
    pub latent: Option<fn(&mut World)>,
}

/// `pawnCtl` and `newPawns`: the controllers of the pawns past the players (a bot's tree is no sim state, as a
/// player's bot) and the pawns spawned in this step that have not ticked yet.
#[derive(Clone, Debug, Default)]
pub struct PawnControllers {
    /// `pawnCtl`: by the pawn, in the order they were set.
    pub ctl: IndexMap<FighterId, PawnCtl>,
    /// `newPawns`: empty between steps.
    pub new_pawns: Vec<NewPawn>,
}

/// `PawnCtl`: the controller that possesses a pawn, off the snapshot like a minion's: the local
/// player's (PlayerController 0: it reads the frame's first input), a bot running a tree by name
/// (Idle_AIController, TutorialRecoverOnly_AIController, Base_AIController, TargetDummyController), a demo
/// controller's graph, or none (SpawnDefaultController with no AIControllerClass spawns nothing; a minion whose
/// stock controller has no tree holds one).
#[derive(Clone, Debug)]
pub enum PawnCtl {
    /// A player's slot (`{ kind: 'player', input: 0 }`).
    Player,
    Bot(Box<BotCtl>),
    Demo(DemoCtl),
    None,
}

/// `BotCtl`: a behaviour tree's controller.
#[derive(Clone, Debug)]
pub struct BotCtl {
    /// `bot`: the tree's running bot (BtBot).
    pub bot: BtBot,
    /// The input it last gave (what the pawn's next `beginFrame` reads).
    pub last: InputState,
    /// The predicted line's held copy of it, `None` after a confirmed think (a predicted step writes only this one).
    pub shown: Option<InputState>,
    /// The controller's ReceivePossess.
    pub possessed: Option<fn(&mut World, FighterId)>,
    /// ReceiveUnPossess, with `destroyed` when the controller goes with the pawn's K2_DestroyActor
    /// (AController::PawnPendingDestroy: a controller with no PlayerState).
    pub unpossessed: Option<fn(&mut World, FighterId, bool)>,
}

/// `DemoCtl`: a Blueprint AIController that drives its pawn from its own graph (the tutorial's TutorialController_*
/// classes): its calls on the pawn (JumpPressed, NormalAttackPressed, BPI_SetMoveAxisX ...) are the pawn's input of
/// its next frame; `input` gives it once per frame.
#[derive(Clone, Debug)]
pub struct DemoCtl {
    /// The demo controller's id (`modes::tutorial::TutorialObj::Demo`): its state in `TutorialRun::demos`.
    pub id: u32,
    pub input: fn(&mut World, FighterId) -> InputState,
    pub possessed: Option<fn(&mut World, FighterId)>,
    pub unpossessed: Option<fn(&mut World, FighterId, bool)>,
}

/// A `newPawns` entry: the pawn and the call that gives it its first tick (`MinionAt`).
#[derive(Clone, Copy, Debug)]
pub struct NewPawn {
    pub m: FighterId,
    pub at: FirstTicks,
}

/// `MinionAt` / `PawnAt`: where in the spawn frame the pawn ticks for the first time.
/// `OwnerTick` when the spawn comes in the owner's actor tick: its controller, CMC, pawn, mesh, components and tree
/// run once after every fighter's `duringPhysicsTick`; `Leftover` when it comes in the leftover latent pass: they run
/// at the frame's end (TG_LastDemotable), after the timers. From the next frame on it ticks after the players in
/// every phase. A spawn outside `World::step` ticks first in the next frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FirstTicks {
    /// `'ownerTick'`, after the fighters' `duringPhysicsTick`.
    OwnerTick,
    /// `'leftover'`, after the ring-outs.
    Leftover,
}

/// `PawnSpawn`: BeginDeferredActorSpawnFromClass + FinishSpawningActor of a fighter class: the
/// data key, the actor location on the sim plane, its facing and where it first ticks. The pawn has no controller,
/// no slot, team 0, no passive buffs and no perks until the mode's own calls give them.
#[derive(Clone, Copy, Debug)]
pub struct PawnSpawn {
    pub character: Name,
    pub x: Real,
    pub y: Real,
    pub facing_left: bool,
    pub at: FirstTicks,
}

/// `MinionSpawn`.
#[derive(Clone, Copy, Debug)]
pub struct MinionSpawn {
    /// The data key: a roster character (the owner's own or another's) or a data-only key the owner's match loads
    /// with it (MATCH_NEEDS, DATA_ONLY: velma -> velmapolice).
    pub character: Name,
    /// GetOwner() of the spawn (the set-up's MinionCreator).
    pub owner: FighterId,
    pub x: Real,
    /// The actor location, sim plane.
    pub y: Real,
    pub facing: Real,
    /// The tree its controller runs, by name; `None`: a controller with no tree.
    pub tree: Option<Name>,
    pub at: FirstTicks,
}

/// `MINION_ID_BASE`: the minions' ids from 100 (a player's is its index, 0..7; the World's
/// hit pair keys and hit sets take a fighter id below 1000), one per spawn of the match in order, taken again after
/// `MINION_IDS` spawns.
pub const MINION_ID_BASE: i32 = 100;
/// `MINION_IDS`.
pub const MINION_IDS: i32 = 900;

/// The part of every mid-match fighter spawn up to its BeginPlay (`bornPawn`): a new id from the pawns' counter
/// (minions and mode pawns share it and its range), an Rng seeded from the id, the spawn count and the tick, the
/// pawn appended to `pawns` (made from the players at the first spawn), the world's clock, `newPawn` at the place,
/// KeepOnCamera's Register, and the run-time spawn's begin overlaps after BeginPlay.
#[allow(clippy::too_many_arguments, reason = "the function keeps the arguments of its counterpart")]
fn born_pawn(
    w: &mut World,
    fn_name: &str,
    character: Name,
    x: Real,
    y: Real,
    facing: Real,
    at: FirstTicks,
    clock: Option<FighterId>,
    platforms: Vec<crate::fighter::PlatformRef>,
    reuse: Option<FighterId>,
) -> FighterId {
    let key = w.names.str(character).to_owned();
    if !w.data.chars.contains_key(&key) {
        panic!("{fn_name}: no data for the character {key}");
    }
    if at == FirstTicks::OwnerTick && w.scratch.step_at >= 2 {
        panic!("{fn_name}: an 'ownerTick' spawn after the fighters' duringPhysicsTick");
    }
    let in_use = |w: &World, id: i32| w.pawns.as_ref().is_some_and(|ps| ps.iter().any(|&p| w.fighter(p).id == id));
    let mut seq = w.pawn_seq.unwrap_or(0);
    // (the port's own Waves mode spawns without end while its player keeps the first id all run: its spawns pass
    // over the ids still in use, `modes::waves`)
    if crate::modes::waves::on(w) {
        let mut tries = 0;
        while in_use(w, MINION_ID_BASE + seq % MINION_IDS) && tries < MINION_IDS {
            seq += 1;
            tries += 1;
        }
    }
    let id = MINION_ID_BASE + seq % MINION_IDS;
    if in_use(w, id) {
        panic!("{fn_name}: the id {id} is still in use");
    }
    w.pawn_seq = Some(seq + 1);
    let hooks = Hooks::bind(w.registry, &key, fn_name).unwrap_or_else(|e| panic!("{fn_name}: {e:?}"));
    let seed = crate::math::dbl::to_int32(
        f64::from(id) * 104_729.0 + f64::from(seq) * 7919.0 + f64::from(w.flow.tick) * 31337.0,
    );
    let fighter = Fighter::new(id, character, hooks, Rng::new(seed));
    let m = match reuse {
        Some(r) => {
            if w.fighter(r).life.dead != Real::INFINITY || w.pawns.as_ref().is_some_and(|ps| ps.contains(&r)) {
                panic!("{fn_name}: the reused fighter is still in the match");
            }
            w.fighters.replace(r, fighter);
            r
        }
        None => w.fighters.insert(fighter),
    };
    crate::fighter::set_up(w, m);
    // (`sharePeers`, `peers`, `stagePlatforms`, `comboReset`, `voShared`: the port's fighters read the World's)
    if w.pawns.is_none() {
        w.pawns = Some(w.players.clone());
    }
    w.pawns.as_mut().expect("made above").push(m);
    w.fighter_mut(m).platforms.platforms = platforms;
    // `m.stagePlatforms = w.stagePlatforms`: the stage's platforms, as the players' (`reset`).
    let n_plat = u32::try_from(w.stage.stage_platforms.len()).expect("a platform count");
    w.fighter_mut(m).platforms.stage_platforms = (0..n_plat).collect();
    // Its clock is the world's (GetTimeSeconds): its first beginFrame takes it from the frame's start again.
    match clock {
        Some(c) => {
            let (t, prev, time) = {
                let c = &w.fighter(c).clock;
                (c.tick, c.prev_time, c.time)
            };
            let cl = &mut w.fighter_mut(m).clock;
            cl.tick = t;
            cl.prev_time = prev;
            cl.time = time;
        }
        None => world_clock(w, m),
    }
    crate::world::reset::new_pawn(w, m, x, y, facing);
    let blast_in = crate::world::blast::blast_overlaps(w, m);
    {
        let fr = w.fighter_mut(m);
        fr.vis.invisible = false;
        // Comp_Actor_KeepOnCamera BeginPlay: Register.
        fr.vis.on_camera = true;
        fr.life.blast_listed = false;
        fr.life.blast_in = blast_in;
    }
    crate::combat::phases::overlaps(w, m, false);
    m
}

/// The fighters' clock at the world's tick, for a spawn with no fighter in the match to take it from: each
/// `beginFrame` adds DELTA_SECONDS in float32 from 0, and inside a step the frame has begun.
fn world_clock(w: &mut World, m: FighterId) {
    let (mut t, mut prev) = (0.0, 0.0);
    for _ in 0..w.flow.tick {
        prev = t;
        t += DELTA_SECONDS;
    }
    let tick = w.flow.tick;
    let cl = &mut w.fighter_mut(m).clock;
    cl.tick = tick;
    cl.prev_time = prev;
    cl.time = t;
}

/// `spawnPawn`: BeginDeferredActorSpawnFromClass + FinishSpawningActor of a fighter class by a game mode's own code
/// (the tutorial's spawn recipe): the pawn's BeginPlay runs inside FinishSpawningActor with no controller
/// (APawn::PostInitializeComponents 142c79790), its Comp_Actor_KeepOnCamera registers it on the camera, and it has
/// no slot, team 0, no passive buffs (no SetTeamIndexOnSpawn: Comp_Pawn_PassiveBuffManager @738-1527) and no perks
/// (PerkManager.Initialize runs in a player controller's possession). FacingLeft is written ahead of
/// FinishSpawningActor: facing +Y, the port's -1. Its first tick is the spawn frame's (`o.at`, as a minion's); a
/// spawn outside the step ticks from the next frame on. Its Comp_ItemManager list is its own (`itemReach`; the
/// World's `reach` is the players').
/// Not ported: Comp_Pawn_TauntManager's slots (a player's taunts, the World's construction) are not given to it.
pub fn spawn_pawn(w: &mut World, o: PawnSpawn) -> FighterId {
    spawn_pawn_into(w, o, None)
}

/// [`spawn_pawn`] into the arena slot of `reuse`, a pawn [`despawn_pawn`] took out of the match, where one is given:
/// the new pawn takes that slot and its [`FighterId`] (an id still held to the old pawn reads the new one), so a mode
/// that spawns without end keeps the World's lists bounded (the port's own Waves mode, `modes::waves`).
pub fn spawn_pawn_into(w: &mut World, o: PawnSpawn, reuse: Option<FighterId>) -> FighterId {
    let any = w.pawns.as_ref().unwrap_or(&w.players).first().copied();
    let platforms = match any {
        Some(a) => w.fighter(a).platforms.platforms.clone(),
        None => {
            crate::world::spawning::platforms_of_frame(w);
            w.scratch.platforms.clone()
        }
    };
    let facing = if o.facing_left { -1.0 } else { 1.0 };
    let m = born_pawn(w, "spawnPawn", o.character, o.x, o.y, facing, o.at, any, platforms, reuse);
    let fr = w.fighter_mut(m);
    fr.items.item_reach = Some(crate::fighter::match_side::ItemReach::default());
    // No controller: MovementMode stays None (the CMC ctor 1429164d0 leaves it; ACharacter::
    // PostInitializeComponents 1429052a0 sets the default mode only with bRunPhysicsWithNoController, which no
    // BaseFighter sets) and the CMC moves nothing until a possession's Restart.
    fr.moves.move_none = true;
    fr.life.uncontrolled = true;
    if w.scratch.step_at != 0 {
        w.pawn_ctl.new_pawns.push(NewPawn { m, at: o.at });
    }
    m
}

/// `bindSlot`: PlayerDataReadyServer(slot) on a mode pawn: MatchPlayerData (its player index), then
/// SetFacingDirection, which offline sets the rotation from FacingLeft (BaseFighter @158520 -> @119234 ->
/// @61228-61332): the port keeps the two as one facing, so nothing moves. Bootstrap UI attaches the pawn's own
/// widgets to the slot's broker (`slotBound`).
/// Not ported: ApplyNewSkin(null) and BroadcastPlayerValues are not read for a mode pawn.
pub fn bind_slot(w: &mut World, f: FighterId, slot: i32) {
    if !mode_pawn(w, f) {
        panic!("bindSlot: not a mode pawn of this match");
    }
    w.fighter_mut(f).life.slot = Some(slot);
    let fighter = w.fighter(f).id;
    w.events.push(Event::SlotBound { fighter, slot });
}

/// `isSame`: whether two controllers are one. PlayerController 0 is one controller (every `Player` is it); a demo
/// controller is one by its id (the tutorial possesses a pawn with one twice), and every caller hands `possess` a bot
/// controller it has just made, so one of those is never one already held.
fn same_ctl(a: Option<&PawnCtl>, b: &PawnCtl) -> bool {
    match (a, b) {
        (Some(PawnCtl::Player), PawnCtl::Player) => true,
        (Some(PawnCtl::Demo(x)), PawnCtl::Demo(y)) => x.id == y.id,
        _ => false,
    }
}

/// `possess`: Possess(f) by a controller (SpawnDefaultController's, a spawned controller's, PlayerController 0's),
/// as Controller::Possess 1429e6a50 and OnPossess 1429e6760 run it: the controller's current other pawn is
/// unpossessed (OnUnPossess +0x6e8: that pawn's UnPossessed, no ReceiveUnPossess), then the pawn's own controller
/// (UnPossess 1429f6f40: its ReceiveUnPossess), the caller too when it already holds the pawn; then PossessedBy,
/// whose ReceivePossessed runs whenever the pawn's controller changed, which that UnPossess makes it do every time (a
/// demo pawn's SpawnDefaultController then Possess runs it twice); then ReceivePossess on the controller only when
/// its pawn changed (none the second time). A `None` controller is SpawnDefaultController with no AIControllerClass:
/// nothing is spawned and nothing possessed (APawn::SpawnDefaultController 142c7ed40). ReceivePossessed's
/// SetupPlayerUI sets the pawn's slot active on its broker and puts the pawn on the camera's priority stack
/// (AddFighterToPriorityStack removes before it adds: once); the input settings follow the controller (a bot's are
/// the defaults).
/// Unverified: a pawn bound to no slot is taken to set no slot active (SetupPlayerUI with no MatchPlayerData not
/// read).
/// Not ported: BaseFighter ReceivePossessed's PerkManager.Initialize for PlayerController 0 (player data 0's perks in
/// a tutorial are not read).
pub fn possess(w: &mut World, f: FighterId, ctl: PawnCtl) {
    if matches!(ctl, PawnCtl::None) {
        return;
    }
    if !mode_pawn(w, f) {
        panic!("possess: not a mode pawn of this match");
    }
    let old = w.pawn_ctl.ctl.get(&f).cloned();
    let same = same_ctl(old.as_ref(), &ctl);
    if !same {
        let others: Vec<FighterId> =
            w.pawn_ctl.ctl.iter().filter(|&(&p, c)| p != f && same_ctl(Some(c), &ctl)).map(|(&p, _)| p).collect();
        for p in others {
            w.pawn_ctl.ctl.shift_remove(&p);
            w.fighter_mut(p).life.uncontrolled = true;
        }
    }
    if let Some(old) = old {
        unpossess(w, f, &old, false);
    }
    // PossessedBy, then ACharacter::Restart: SetDefaultMovementMode, Walking on a base, else Falling.
    // Unverified: the default mode is taken from the pawn's grounded as it stands (a knockback or flying mode it
    // was left in is not cleared), and Restart's UnCrouch and jump reset are not run.
    let prefs = match ctl {
        PawnCtl::Player => w.setup.players.first().map_or_else(InputPrefs::default, |p| p.input),
        _ => InputPrefs::default(),
    };
    let (slot, fighter) = {
        let fr = w.fighter_mut(f);
        fr.life.uncontrolled = false;
        fr.moves.move_none = false;
        fr.controls.prefs = prefs;
        (fr.life.slot, fr.id)
    };
    if let Some(slot) = slot {
        w.events.push(Event::SlotActive { slot, on: true });
    }
    w.events.push(Event::PriorityAdd { fighter });
    let possessed = match &ctl {
        PawnCtl::Bot(b) => b.possessed,
        PawnCtl::Demo(d) => d.possessed,
        _ => None,
    };
    w.pawn_ctl.ctl.insert(f, ctl);
    if !same && let Some(h) = possessed {
        h(w, f);
    }
}

/// `unpossess`: UnPossess (1429f6f40) of f's controller: OnUnPossess (the pawn's UnPossessed, SetPawn(None)), then
/// the controller's ReceiveUnPossess; the controller leaves `pawn_ctl`.
fn unpossess(w: &mut World, f: FighterId, ctl: &PawnCtl, destroyed: bool) {
    w.pawn_ctl.ctl.shift_remove(&f);
    w.fighter_mut(f).life.uncontrolled = true;
    let h = match ctl {
        PawnCtl::Bot(b) => b.unpossessed,
        PawnCtl::Demo(d) => d.unpossessed,
        _ => None,
    };
    if let Some(h) = h {
        h(w, f, destroyed);
    }
}

/// `despawnPawn`: the removal of a mode pawn. With `teardown`, the controller part of the tutorial's whole teardown
/// (TutorialActor ControlTutorialPawn @213-490): PlayerState K2_DestroyActor (None for an AI-held pawn),
/// SetLifeSpan(0) (AActor 14280b290: no lifespan, nothing destroyed), the brain's K2_DestroyComponent (no tree runs:
/// the bot leaves with the entry) and UnPossess (its ReceiveUnPossess); the controller itself lives on with no pawn
/// (its own timers are the mode's state). Then K2_DestroyActor: APawn::Destroyed's
/// DetachFromControllerPendingDestroy, which for a pawn still held unpossesses it and destroys a controller with no
/// PlayerState (AController::PawnPendingDestroy 1429e69d0), then RouteEndPlay: AFighterCharacter::EndPlay 1410bc5c0
/// skips the parent, so no component's EndPlay runs, now or at GC (UActorComponent::EndPlay 14292a5c0):
/// Comp_Spawner's DestroyActorsOnEndPlay never comes (the pawn's Die With Instigator actors outlive it, owned by the
/// removed pawn), nor KeepOnCamera's Unregister. Then ReceiveDestroyed (BaseFighter @133174):
/// Comp_Actor_Parent.DestroyChildren (its buffs, each one's EndPlay in turn), the slot's broker
/// SetActiveForPlayer(index, false). The pawn leaves the match's fighters; what it still had queued goes out with the
/// frame's events.
/// Unverified: the UnPossess inside PawnPendingDestroy (a pawn removed while still possessed) is the 4.26 source's,
/// not read in the exe; the order of the buff children's destroys is taken as the port's list order.
/// Deviation: the camera's RegisteredOnscreenActors entry the build keeps for the removed pawn (the box then takes
/// the world origin) is not kept: the port's camera frames the match's pawns alone.
pub fn despawn_pawn(w: &mut World, f: FighterId, teardown: bool) {
    if w.pawns.is_none() || !mode_pawn(w, f) {
        panic!("despawnPawn: not a mode pawn of this match");
    }
    if let Some(ctl) = w.pawn_ctl.ctl.get(&f).cloned() {
        let destroyed = !teardown && !matches!(ctl, PawnCtl::Player);
        unpossess(w, f, &ctl, destroyed);
    }
    // `for (const b of [...f.buffs])`: the entries as they stand, each ended in turn.
    let serials: Vec<u32> = w.fighter(f).buffs.buffs.iter().map(|b| b.serial).collect();
    for s in serials {
        let Some(b) = w.fighter_mut(f).buffs.buffs.iter_mut().find(|b| b.serial == s) else { continue };
        if b.frames <= 0.0 {
            continue;
        }
        b.frames = 0.0;
        crate::combat::buffs::tick_buffs(w, f, true);
    }
    flush_events(w, f);
    let slot = w.fighter(f).life.slot;
    if let Some(slot) = slot {
        w.events.push(Event::SlotActive { slot, on: false });
    }
    w.fighter_mut(f).life.dead = Real::INFINITY;
    // (the unpossess hook and the buffs' ends may have removed other pawns)
    if let Some(ps) = w.pawns.as_mut()
        && let Some(i) = ps.iter().position(|&p| p == f)
    {
        ps.remove(i);
    }
    w.pawn_ctl.new_pawns.retain(|n| n.m != f);
}

/// `modePawn`: a pawn past the players that is no minion (a mode's).
fn mode_pawn(w: &World, f: FighterId) -> bool {
    !w.fighter(f).life.is_minion
        && w.pawns.as_ref().and_then(|ps| ps.iter().position(|&p| p == f)).is_some_and(|i| i >= w.players.len())
}

/// `spawnMinion`: UAIBlueprintHelperLibrary::SpawnAIFromClass as Pawn_StevenUniverse calls it (@3572), then the
/// shared part of its set-up (MulticastWatermelonStevenSetUp @3639). In this order, inside the call: the pawn
/// (construction, its BeginPlay: no controller, no Instigator, not yet a minion, team 0, KeepOnCamera registered;
/// then the run-time spawn's begin overlaps after BeginPlay), its stock controller and tree (its first search at its
/// first tick), then the set-up's raw writes NotPlayerCharacter @4588, TeamIndex @4769 (a field write: no
/// TeamIndexSetOnSpawn, so no passive buffs on it or from it, Comp_Pawn_PassiveBuffManager @738-1527), IsMinion
/// @4921, MinionCreator @4954 and KeepOnCamera.Unregister @5121 (off the camera; the app's camera and HUD read the
/// players alone, so it has no plate either: DestroyUI @5955). The caller writes the rest of its set-up behind this
/// call (scale, weight, skin, buffs); the OnBFDefeated binding @5448 is the creator's `minion_defeated` hook. No
/// perks (no PerkManager possession), no PlayerState: `bot_controlled` stays false (unverified: IsBotControlled not
/// read in the exe for a pawn without a PlayerState). Its first tick is the spawn frame's (`o.at`); a spawn outside
/// the step ticks from the next frame on. `o.character` need not be the owner's: any key the data holds, a data-only
/// one too (Velma's police car, Pawn_VelmaAIBotPolice, SpawnAIFromClass @5963).
pub fn spawn_minion(w: &mut World, o: MinionSpawn) -> FighterId {
    let platforms = w.fighter(o.owner).platforms.platforms.clone();
    let m = born_pawn(w, "spawnMinion", o.character, o.x, o.y, o.facing, o.at, Some(o.owner), platforms, None);
    let seq = w.pawn_seq.expect("set by born_pawn") - 1;
    let id = w.fighter(m).id;
    let seed = crate::math::dbl::to_int32(f64::from(id) * 7919.0 + f64::from(seq) * 104_729.0 + 12345.0);
    let ctl = make_minion_ctl(w, m, o.tree, seed, w.scratch.step_at != 0);
    w.pawn_ctl.ctl.insert(m, ctl);
    let team = w.fighter(o.owner).team.team;
    {
        let fr = w.fighter_mut(m);
        fr.life.not_player_character = true;
        fr.team.team = team;
        fr.life.is_minion = true;
        fr.life.minion_creator = Some(o.owner);
        fr.vis.on_camera = false;
    }
    if w.scratch.step_at != 0 {
        w.pawn_ctl.new_pawns.push(NewPawn { m, at: o.at });
    }
    m
}

/// `makeMinionCtl`: the controller of a minion: its stock bot, or `None` for a controller with
/// no tree. The controller is a stock AAIController (AIControllerClass is the engine's, the set-up's write @4690
/// comes after the possession): no Base_AIController graph, no PlayerState, its tree run by name with every
/// cast<Base_AIController_C> failing. Like a player's bot, it lives off the snapshot: it thinks only on a confirmed
/// step, and a predicted one holds its last input, so a predicted step writes only its `shown`.
fn make_minion_ctl(w: &World, m: FighterId, tree: Option<Name>, seed: i32, in_step: bool) -> PawnCtl {
    let Some(tree) = tree else { return PawnCtl::None };
    let Some(trees) = w.bot_trees.as_ref() else {
        panic!("spawnMinion: the tree {} needs World::bot_trees", w.names.str(tree));
    };
    let cl = &w.fighter(m).clock;
    // GetTimeSeconds is the world's: the first think reads the time of the frame it runs in (think adds a frame):
    // the spawn frame's for a spawn in the step (behind the minion's beginFrame), the next frame's for one outside
    // it; GetGameTimeSinceCreation counts from the spawn.
    let bot = BtBot::minion(seed, trees, tree, if in_step { cl.prev_time } else { cl.time }, cl.creation_time);
    PawnCtl::Bot(Box::new(BotCtl { bot, last: NO_INPUT, shown: None, possessed: None, unpossessed: None }))
}

/// `botCtl`: a bot controller of a tree that has not thought yet.
pub fn bot_ctl(
    bot: BtBot,
    possessed: Option<fn(&mut World, FighterId)>,
    unpossessed: Option<fn(&mut World, FighterId, bool)>,
) -> BotCtl {
    BotCtl { bot, last: NO_INPUT, shown: None, possessed, unpossessed }
}

/// `destroyMinion`: K2_DestroyActor of a minion (its creator's handler, the `minion_defeated` hook): Comp_Spawner
/// ReceiveEndPlay destroys the actors it spawned with 'Die With Instigator' (as the respawn's
/// DestroyActorsOnEndPlay), and it leaves the match's fighters; what it still had queued goes out with the frame's
/// events.
/// Not ported: the other components' EndPlay (a hold it was in, a buff it gave another fighter).
/// Deviation: AFighterCharacter::EndPlay 1410bc5c0 skips the parent, so the build runs no component EndPlay at a
/// BaseFighter's destroy and Comp_Spawner's ReceiveEndPlay never comes; the minion's Die With Instigator actors are
/// still taken to go with it here.
pub fn destroy_minion(w: &mut World, m: FighterId) {
    let i = w.pawns.as_ref().and_then(|ps| ps.iter().position(|&p| p == m));
    if !i.is_some_and(|i| i >= w.players.len()) {
        panic!("destroyMinion: not a minion of this match");
    }
    crate::world::spawning::destroy_with_instigator(w, m);
    w.fighter_mut(m).life.dead = Real::INFINITY;
    // (the destroys may have changed the list: the index taken first is removed)
    if let (Some(ps), Some(i)) = (w.pawns.as_mut(), i) {
        ps.remove(i);
    }
    w.pawn_ctl.new_pawns.retain(|n| n.m != m);
    if !w.predicted {
        w.pawn_ctl.ctl.shift_remove(&m);
    }
    flush_events(w, m);
}

/// `minionBot`: GetAIController(minion).Blackboard: the minion's stock bot, `None` for one with no tree or not a
/// minion of this match. Read by a pawn graph that reads its own blackboard (Pawn_VelmaAIBotPolice @492
/// GetValueAsObject(EnemyActor)).
pub fn minion_bot(w: &World, m: FighterId) -> Option<&BtBot> {
    match w.pawn_ctl.ctl.get(&m) {
        Some(PawnCtl::Bot(b)) => Some(&b.bot),
        _ => None,
    }
}

/// `pawnThink`: the BTComponent's tick (TG_DuringPhysics, behind the pawn's components): the
/// input of its next frame. On a predicted step it holds its last one.
fn pawn_think(w: &mut World, f: FighterId) {
    let Some(PawnCtl::Bot(ctl)) = w.pawn_ctl.ctl.get_mut(&f) else { return };
    if w.predicted {
        ctl.shown = Some(held(ctl.shown.as_ref().unwrap_or(&ctl.last)));
        return;
    }
    let mut bot = std::mem::take(&mut ctl.bot);
    let last = bot.think(w, f);
    if let Some(PawnCtl::Bot(ctl)) = w.pawn_ctl.ctl.get_mut(&f) {
        ctl.bot = bot;
        ctl.last = last;
        ctl.shown = None;
    }
}

/// `World.step`: a controller whose pawn has left the match: destroyed on a confirmed step, or spawned on a
/// predicted one that the restore took back (a confirmed step is never rolled back; a predicted one may still bring
/// back a minion it destroyed, so the entries are kept until then).
pub fn prune_controllers(w: &mut World) {
    if w.predicted || w.pawn_ctl.ctl.is_empty() {
        return;
    }
    let fs = &w.scratch.fs;
    w.pawn_ctl.ctl.retain(|k, _| fs.contains(k));
}

/// `pawnInput`: a pawn's input for the frame past the players (the step gives `inputs[i]` to the player `i`):
/// PlayerController 0's reads the frame's `inputs[0]`; a bot's is its last think (the BTComponent's tick of the
/// frame before), on the predicted line its held copy once a predicted step has made one; a demo controller's the
/// calls its graph made on the pawn since its last frame; none without a controller.
pub fn pawn_input(w: &mut World, f: FighterId, closed: bool, inputs: &[InputState]) -> InputState {
    let Some(ctl) = w.pawn_ctl.ctl.get(&f) else { return NO_INPUT };
    if closed {
        return NO_INPUT;
    }
    match ctl {
        PawnCtl::Player => inputs.first().copied().unwrap_or(NO_INPUT),
        PawnCtl::Demo(d) => {
            let input = d.input;
            canon_input(&input(w, f))
        }
        PawnCtl::Bot(b) => canon_input(if w.predicted { b.shown.as_ref().unwrap_or(&b.last) } else { &b.last }),
        PawnCtl::None => canon_input(&NO_INPUT),
    }
}

/// `controllerThink`: a pawn's controller think behind its components (the step's TG_DuringPhysics, `firstTicks`):
/// a bot's alone.
pub fn controller_think(w: &mut World, f: FighterId) {
    if matches!(w.pawn_ctl.ctl.get(&f), Some(PawnCtl::Bot(_))) {
        pawn_think(w, f);
    }
}

/// `Pawns.firstTicks`: the first tick of the pawns spawned in this step at `at`: controller, CMC, pawn, mesh, then
/// its components and its tree, each one whole in spawn order.
pub fn first_ticks(w: &mut World, at: FirstTicks) {
    if w.pawn_ctl.new_pawns.is_empty() {
        return;
    }
    let now: Vec<FighterId> = w.pawn_ctl.new_pawns.iter().filter(|n| n.at == at).map(|n| n.m).collect();
    if now.is_empty() {
        return;
    }
    w.pawn_ctl.new_pawns.retain(|n| n.at != at);
    for m in now {
        if w.fighter(m).life.dead == Real::INFINITY && !w.pawns.as_ref().is_some_and(|ps| ps.contains(&m)) {
            continue;
        }
        {
            // beginFrame counts the frame again
            let cl = &mut w.fighter_mut(m).clock;
            cl.tick -= 1;
            cl.time = cl.prev_time;
        }
        crate::world::flow::bind_ffa_damage(w, m);
        let inputs = std::mem::take(&mut w.scratch.inputs);
        let inp = pawn_input(w, m, w.scratch.closed, &inputs);
        w.scratch.inputs = inputs;
        tick::begin_frame_input(w, m, inp);
        tick::set_on_turn(w, m);
        tick::controller_tick(w, m);
        tick::cmc_tick_own(w, m);
        let (moved, held_item) = {
            let fr = w.fighter(m);
            (fr.frame.moved, fr.items.held_item.is_some())
        };
        if moved {
            crate::combat::phases::overlaps(w, m, false);
        }
        if moved && held_item {
            tick::follow_holder(w, m);
        }
        tick::pawn_tick_own(w, m, true);
        tick::clear_on_turn(w, m);
        tick::mesh_tick_own(w, m);
        crate::combat::phases::overlaps(w, m, true);
        tick::during_physics_tick_own(w, m);
        tick::fly_tick(w, m);
        controller_think(w, m);
    }
}

/// `Pawns.flushEvents`: a fighter's queued events and UI ops into the frame's.
pub fn flush_events(w: &mut World, f: FighterId) {
    let World { fighters, events, .. } = w;
    let fr = fighters.get_mut(f).expect("a live fighter id");
    events.append(&mut fr.events);
    let fighter = fr.id;
    events.extend(fr.controls.ui_state_ops.drain(..).map(|op| Event::UiState { fighter, op }));
}

// ---- the call surface of the pawns

/// `matchFighters`: the fighters a fighter loop of the port meets: the players (by player index), then a mode's
/// pawns in spawn order; no minion (the loops written over the players never met one, and a minion is
/// NotPlayerCharacter). The players' own list where the match has no pawns past them.
pub fn match_fighters(w: &World) -> Vec<FighterId> {
    match &w.pawns {
        Some(ps) => ps.iter().copied().filter(|&f| !w.fighter(f).life.is_minion).collect(),
        None => w.players.clone(),
    }
}

/// An empty fighter list of the World's pool ([`put_fighters`] hands it back; a walk inside a walk takes another).
pub fn take_fighters(w: &mut World) -> Vec<FighterId> {
    let mut v = w.scratch.fighter_lists.pop().unwrap_or_default();
    v.clear();
    v
}

/// `match_fighters` in a list of the World's pool, for a walk whose body changes the World ([`put_fighters`] hands
/// it back; a walk inside a walk takes another).
pub fn match_fighters_list(w: &mut World) -> Vec<FighterId> {
    let mut v = take_fighters(w);
    match &w.pawns {
        Some(ps) => v.extend(ps.iter().copied().filter(|&f| !w.fighter(f).life.is_minion)),
        None => v.extend_from_slice(&w.players),
    }
    v
}

/// `live` in a list of the World's pool ([`put_fighters`] hands it back).
pub fn live_list(w: &mut World) -> Vec<FighterId> {
    let mut v = take_fighters(w);
    match &w.pawns {
        Some(ps) => {
            let new = &w.pawn_ctl.new_pawns;
            v.extend(ps.iter().copied().filter(|&f| !new.iter().any(|n| n.m == f)));
        }
        None => v.extend_from_slice(&w.players),
    }
    v
}

/// `World.all` (`World::all_ids`) in a list of the World's pool ([`put_fighters`] hands it back).
pub fn all_list(w: &mut World) -> Vec<FighterId> {
    let mut v = take_fighters(w);
    v.extend_from_slice(w.pawns.as_deref().unwrap_or(&w.players));
    v
}

/// A list of [`match_fighters_list`], [`live_list`] or [`all_list`] back to the World's pool.
pub fn put_fighters(w: &mut World, v: Vec<FighterId>) {
    w.scratch.fighter_lists.push(v);
}

/// `match_fighters` walked in place, for a reader that changes nothing.
pub fn match_fighters_iter(w: &World) -> impl Iterator<Item = FighterId> + '_ {
    let (ps, minions) = match &w.pawns {
        Some(ps) => (ps.as_slice(), true),
        None => (w.players.as_slice(), false),
    };
    ps.iter().copied().filter(move |&f| !minions || !w.fighter(f).life.is_minion)
}

/// `isMatchFighter`: `f` is one of `match_fighters` (a BaseFighter class filter on an overlap's actor).
pub fn is_match_fighter(w: &World, f: FighterId) -> bool {
    if w.players.contains(&f) {
        return true;
    }
    w.pawns.as_ref().is_some_and(|ps| ps.contains(&f)) && !w.fighter(f).life.is_minion
}

/// `slotOf`: the player index a pawn stands for: a mode pawn's bound slot, a player's index, -1 for a minion or an
/// unbound mode pawn (mode pawns take their ids from the minions' counter and range). Every per-player read (the
/// ring-out asset, the HUD's target, the item reach) goes through it.
pub fn slot_of(w: &World, f: FighterId) -> i32 {
    let fr = w.fighter(f);
    fr.life.slot.unwrap_or(if fr.life.is_minion || fr.id >= MINION_ID_BASE { -1 } else { fr.id })
}

// ---- the call surface of the match's pawns

/// `live`: the players' list, or every fighter of the match but the pawns of this step
/// that have not ticked yet.
pub fn live(w: &World) -> Vec<FighterId> {
    let Some(ps) = &w.pawns else { return w.players.clone() };
    let new = &w.pawn_ctl.new_pawns;
    ps.iter().copied().filter(|&f| !new.iter().any(|n| n.m == f)).collect()
}

/// `dropPawns`: the Lab's reset (`World::reset`).
/// Deviation: TrainGameMode Destroy All Players abducts its SpawnedFighters alone, so a minion outlives the reset in
/// the build; the port's reset takes it away with the players' pawns.
pub fn drop_pawns(w: &mut World) {
    let Some(ps) = w.pawns.take() else { return };
    let np = w.players.len();
    for &m in &ps[np.min(ps.len())..] {
        w.fighter_mut(m).life.dead = Real::INFINITY;
    }
    // (`f.peers = w.fighters`: the port's fighters read the World's lists)
    w.pawn_seq = None;
    w.pawn_ctl.ctl.clear();
    w.pawn_ctl.new_pawns.clear();
}
