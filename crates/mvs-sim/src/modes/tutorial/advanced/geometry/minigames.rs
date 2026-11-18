//! The geometry family's minigame actors: TutorialMinigame_SpawnWall,
//! _SpawnPlatform, _KBISide and _KBIUp.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::ai::pad::Btn as BotBtn;
use crate::fighter::FighterId;
use crate::fighter::match_side::TutorialEvent;
use crate::input::Dir;
use crate::name;
use crate::world::World;
use crate::world::flow::{DelegateCall, DelegateList, PawnList};

use super::super::super::TutorialObj;
use super::super::super::classes::{self, Call, Fired, RunList, minigame_mut};
use super::super::super::demo::{Place, callable, teleport};
use super::super::super::latent::{self, MoveTarget};
use super::super::super::minigame::{self, MinigameState};
use super::super::super::pawn::{bot_press, place_of_loc, set_current_damage, set_facing_logic, sound_at, tagged};
use super::solids::{self, Held, SolidClass, spec_of};
use super::{CAST_SOUND, emitter_at};

/// TutorialMinigame_SpawnWall: ReceiveBeginPlay (@826) binds TutorialStepCleanup to CleanUpSpawnedActor, then a
/// TutorialWall at (0, -1000, 1480) and the cast sound at it (@15). CleanUpSpawnedActor (@1056): with the wall valid,
/// the wall destroyed and then the actor itself (an invalid wall leaves both).
#[derive(State, Debug, Default)]
pub struct SpawnWall {
    pub wall: Option<Held>,
}

/// TutorialMinigame_SpawnPlatform: ReceiveBeginPlay (@813) binds TutorialStepCleanup, then (@15) a TutorialPlatform at
/// the transform of the first actor tagged Platform (no sound). CleanUpSpawnedActor (@1099) as SpawnWall's.
#[derive(State, Debug, Default)]
pub struct SpawnPlatform {
    pub platform: Option<Held>,
}

/// The two KBI minigames' shared part: the player pawn (GetPlayerPawn(PlayerIndex): the step's managed pawn), the
/// KnockedBackIncludeSelf handler bound per pawn (its Attackee), PlayerSuccessfullyAttacked and TutorialComplete.
/// Not ported: Comp_Pawn_DamageManager DI_ShowVisualOverride = true (the port draws no DI visual to force).
#[derive(State, Debug, Default)]
pub struct KbiMinigame {
    pub player: Option<FighterId>,
    pub attacked: bool,
    pub tutorial_complete: bool,
    /// The last OnKnockedBack's Attackee (the Delay's resume reads the event's parameter as it is then).
    pub attackee: Option<FighterId>,
    pub wall: Option<Held>,
}

/// TutorialMinigame_KBISide (PlatformCurve: Z +1200 over 1.5 s, 0 over 0.5 s, Z -1200 over 1.5 s, 0 over 0.5 s).
/// ReceiveBeginPlay (@2767): TutorialStepComplete to CompleteTutorialStep (TutorialComplete = true), TutorialStepCleanup
/// to CleanupTutorialStep, the game mode's PlayerRespawned to OnPlayerRespawn, then OnPlayerRespawn(0).
/// OnPlayerRespawn (@4258 -> @1808): the player's head (damage 100); TutorialComplete: nothing more; no valid wall:
/// SpawnWall; Delay 2.0 (@27) then AttackPlayer.
/// AttackPlayer (@3242): not complete: the bag's NormalAttackPressed(1 Down), Delay 0.5 (@1056): attacked: the flag
/// cleared; else AttackPlayer again.
/// OnKnockedBack (@3769): DotProduct2D(KnockBackDirection, (1, 0)) < 0 (toward -world Y): DestroyWall, Delay 0.8 (@42):
/// the emitter at the pawn, the pawn teleported to the first PlayerCenterLeft (rotation zero), the emitter there,
/// OnPlayerRespawn(0); else the "attacked" state and PlayerSuccessfullyAttacked.
/// Unverified: K2_TeleportTo's rotation (0, 0, 0) (@364) is not applied: the teleport keeps the pawn's facing.
/// SpawnWall (@4387): a TutorialWallShort at the transform of KBIWall[WallCount % 2], the cast sound at it; WallCount 3:
/// CurveIndex 0 and MoveWall(its place, 0); WallCount + 1.
/// MoveWall (@5657): the wall's root K2_SetWorldLocation(StartLocation, teleport), CurveIndex, then (@2418)
/// MoveComponentTo(root, StartLocation + PlatformCurve[CurveIndex], its duration, no ease); its Completed (@676):
/// CurveIndex + 1 mod 4, with the wall valid MoveWall(the wall's place, CurveIndex).
/// DestroyWall (@5184): with the wall valid: ProcessTutorialEvent(86 KBILeft), sfx_map_shared_rockwall_destruct at it,
/// PS_WallDestruction at it, the wall destroyed, Wall = None.
/// CleanupTutorialStep (@2979): DestroyWall, PlayerRespawned unbound, the player's CurrentDamage 0. The actor is not
/// destroyed (its Delays and handlers run on to the end of the tutorial).
/// Not ported: PS_WallDestruction (not among the bound particle systems).
/// Unverified: a wall destroyed mid-move ends its MoveComponentTo with no Completed (`latent.rs`); MoveWall then
/// stops, and a later wall stands still.
#[derive(State, Debug, Default)]
pub struct KbiSideMinigame {
    pub k: KbiMinigame,
    pub wall_count: i32,
    pub curve_index: i32,
}

/// TutorialMinigame_KBIUp (RespawnTag PunchingBagCenter). ReceiveBeginPlay (@2663): TutorialStepComplete
/// (TutorialComplete = true) and TutorialStepCleanup bound, PlayerRespawned to OnPlayerRespawn, OnPlayerRespawn(0),
/// SpawnWall (@4449: a TutorialWallShort at the transform of the first KBICeilLeft, K2_SetActorRotation((0, 0, 90)),
/// the cast sound at it, @1800), then (@30) TutorialActor.PunchingBag moved by (0, 100, 0) (K2_SetActorLocation, no
/// sweep).
/// OnPlayerRespawn (@3811): the player's head (damage 115), Delay 1.0 (@15) then AttackPlayer (no complete gate).
/// AttackPlayer (@2919): not complete: the bag's NormalAttackPressed(0 Up), Delay 1.5 (@1759): attacked: the flag
/// cleared; else AttackPlayer again.
/// OnKnockedBack (@3446): PlayerSuccessfullyAttacked, the "attacked" state, Delay 2.0 (@559): with the Attackee valid:
/// ProcessTutorialEvent(88 KBIUp), BPI_DisableInput, the emitter at it, K2_TeleportTo(the first RespawnTag actor, its
/// rotation), the emitter there, MoveComponentTo(its root, that place, its rotation, 0.2 s, no ease) whose Completed
/// (@1739) is OnPlayerRespawn(0).
/// CleanupTutorialStep (@2882): the wall destroyed. PlayerRespawned stays bound, the actor is not destroyed.
/// Unverified: the count of 88 rests on the pawn alone being valid 2.0 s after the knockback; no test of the ceiling's
/// hit is made, as decoded.
#[derive(State, Debug, Default)]
pub struct KbiUpMinigame {
    pub k: KbiMinigame,
}

// ---- the classes' tables

/// The state of the class `$v` of minigame `$id`.
macro_rules! ms {
    ($w:expr, $id:expr, $v:ident) => {
        match &mut minigame_mut($w, $id).expect("a live minigame").state {
            MinigameState::$v(s) => s,
            _ => unreachable!(),
        }
    };
}

/// A call the class has no handler for: a node of another class's, which no entry of this one makes.
fn unhandled(w: &World, id: u32, c: Call<'_>) {
    let cls = classes::minigame_of(w, id).map_or("(gone)", |m| m.cls);
    panic!("tutorial: {cls} has no handler for {c:?}");
}

fn delay(w: &mut World, id: u32, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Minigame(id), node, s);
}

/// A call on one of this family's minigame actors.
pub fn call(w: &mut World, id: u32, c: Call<'_>) {
    use MinigameState as M;
    // (none of the family's classes has an own ReceiveEndPlay)
    if matches!(c, Call::Destroyed) {
        return;
    }
    let Some(m) = classes::minigame_of(w, id) else { return };
    match &m.state {
        M::SpawnWall(_) => spawn_wall_call(w, id, c),
        M::SpawnPlatform(_) => spawn_platform_call(w, id, c),
        M::KbiSide(_) => kbi_call(w, id, c, false),
        M::KbiUp(_) => kbi_call(w, id, c, true),
        _ => unreachable!("a minigame of another family"),
    }
}

// ---- SpawnWall, SpawnPlatform

fn spawn_wall_call(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::BeginPlay => {
            minigame::bind_steps(w, id, false, true);
            let wall = Held::spawn(w, SolidClass::Wall, spec_of(id, 0), &[0.0, -1000.0, 1480.0], &[]);
            if let Some(h) = &wall {
                let (x, y) = h.at(w);
                sound_at(w, CAST_SOUND, x, y);
            }
            ms!(w, id, SpawnWall).wall = wall;
        }
        Call::Delegate { handler: "onStepCleanup", .. } => {
            if !solids::valid(&ms!(w, id, SpawnWall).wall) {
                return;
            }
            let mut wall = ms!(w, id, SpawnWall).wall.take();
            solids::destroy(w, &mut wall);
            ms!(w, id, SpawnWall).wall = wall;
            minigame::destroy(w, id);
        }
        _ => unhandled(w, id, c),
    }
}

fn spawn_platform_call(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::BeginPlay => {
            minigame::bind_steps(w, id, false, true);
            let t = tagged(w, "Platform", 0);
            let p = t.and_then(|t| Held::spawn(w, SolidClass::Platform, spec_of(id, 0), &t.loc, &t.rot));
            ms!(w, id, SpawnPlatform).platform = p;
        }
        Call::Delegate { handler: "onStepCleanup", .. } => {
            if !solids::valid(&ms!(w, id, SpawnPlatform).platform) {
                return;
            }
            let mut p = ms!(w, id, SpawnPlatform).platform.take();
            solids::destroy(w, &mut p);
            ms!(w, id, SpawnPlatform).platform = p;
            minigame::destroy(w, id);
        }
        _ => unhandled(w, id, c),
    }
}

// ---- the KBI minigames

/// The shared fields of the KBI minigame `id`.
fn k(w: &mut World, id: u32) -> &mut KbiMinigame {
    match &mut minigame_mut(w, id).expect("a live minigame").state {
        MinigameState::KbiSide(s) => &mut s.k,
        MinigameState::KbiUp(s) => &mut s.k,
        _ => unreachable!(),
    }
}

fn side(w: &mut World, id: u32) -> &mut KbiSideMinigame {
    ms!(w, id, KbiSide)
}

fn kbi_call(w: &mut World, id: u32, c: Call<'_>, up: bool) {
    let me = TutorialObj::Minigame(id);
    match c {
        Call::BeginPlay => {
            minigame::bind_steps(w, id, true, true);
            classes::bind_run(w, RunList::PlayerRespawned, me, RunList::PlayerRespawned.base_handler());
            player_respawn(w, id, up);
            if up {
                let t = tagged(w, "KBICeilLeft", 0);
                let wall =
                    t.and_then(|t| Held::spawn(w, SolidClass::WallShort, spec_of(id, 0), &t.loc, &[0.0, 0.0, 90.0]));
                if let Some(h) = &wall {
                    let (x, y) = h.at(w);
                    sound_at(w, CAST_SOUND, x, y);
                }
                k(w, id).wall = wall;
                let bag = w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).and_then(|a| a.bag);
                if let (true, Some(bag)) = (callable(w, bag), bag) {
                    let (x, y) = (w.fighter(bag).body.x, crate::fighter::actor_y(w, bag));
                    crate::fighter::moves::recall_move(w, bag, x - 100.0, y);
                    crate::combat::phases::root_moved(w, bag);
                }
            }
        }
        Call::Delegate { handler: "onStepComplete", .. } => k(w, id).tutorial_complete = true,
        Call::Delegate { handler: "onStepCleanup", .. } => {
            if up {
                let mut wall = k(w, id).wall.take();
                solids::destroy(w, &mut wall);
                k(w, id).wall = wall;
            } else {
                destroy_wall(w, id);
                classes::unbind_run(w, RunList::PlayerRespawned, me, Some(RunList::PlayerRespawned.base_handler()));
                let p = k(w, id).player;
                if let Some(p) = p
                    && w.fighter(p).life.dead != Real::INFINITY
                {
                    set_current_damage(w, p, 0.0);
                }
            }
        }
        Call::Delegate { handler: "playerRespawned", call: DelegateCall::PlayerRespawned { .. }, .. } => {
            player_respawn(w, id, up);
        }
        Call::Delegate {
            handler: "knockedBack",
            src: Some(ActorRef::Fighter(f)),
            call: DelegateCall::KnockedBackIncludeSelf { dir_x, .. },
        } => {
            if up {
                knocked_back_up(w, id, f);
            } else {
                knocked_back_side(w, id, f, *dir_x);
            }
        }
        Call::Fired(Fired::Delay("@27")) if !up => attack_player(w, id, false),
        Call::Fired(Fired::Delay("@15")) if up => attack_player(w, id, true),
        Call::Fired(Fired::Delay("@1056")) if !up => attack_done(w, id, false),
        Call::Fired(Fired::Delay("@1759")) if up => attack_done(w, id, true),
        Call::Fired(Fired::Delay("@42")) if !up => {
            let g = k(w, id).attackee;
            if callable(w, g) {
                let g = g.expect("callable");
                let (x, y) = (w.fighter(g).body.x, crate::fighter::actor_y(w, g));
                fx(w, up, x, y);
            }
            if let Some(t) = tagged(w, "PlayerCenterLeft", 0) {
                let (x, y) = place_of_loc(&t.loc);
                teleport(w, g, Some(Place { x, y }));
                fx(w, up, x, y);
            }
            player_respawn(w, id, false);
        }
        Call::Fired(Fired::Delay("@559")) if up => {
            let g = k(w, id).attackee;
            if !callable(w, g) {
                return;
            }
            let g = g.expect("callable");
            minigame::process_event(w, TutorialEvent::KBIUp);
            crate::fighter::moves::disable_input(w, g);
            let (gx, gy) = (w.fighter(g).body.x, crate::fighter::actor_y(w, g));
            fx(w, true, gx, gy);
            let Some(t) = tagged(w, "PunchingBagCenter", 0) else { return };
            let (x, y) = place_of_loc(&t.loc);
            teleport(w, Some(g), Some(Place { x, y }));
            fx(w, true, x, y);
            latent::move_to(w, me, "@1646", MoveTarget::Fighter(g), x, y, 0.2, false, false);
        }
        Call::Fired(Fired::Move("@1646")) if up => player_respawn(w, id, true),
        Call::Fired(Fired::Move("@2647")) if !up => {
            let s = side(w, id);
            s.curve_index = (s.curve_index + 1) % 4;
            let i = s.curve_index;
            if let Some((x, y)) = wall_at(w, id) {
                move_wall(w, id, x, y, i);
            }
        }
        _ => unhandled(w, id, c),
    }
}

/// SpawnEmitterAtLocation(PS_MeeseeksSpawn) with Team 1.0.
fn fx(w: &mut World, up: bool, x: Real, y: Real) {
    let cls = if up { name!("TutorialMinigame_KBIUp_C") } else { name!("TutorialMinigame_KBISide_C") };
    emitter_at(w, cls, x, y);
}

/// OnPlayerRespawn's head: the cast of GetPlayerPawn(index), Player, BPI_SetFacingLogic(false), BPI_DisableInput,
/// CurrentDamage, the KnockedBackIncludeSelf bind (AddUnique: once per pawn). False where the cast fails.
fn take_player(w: &mut World, id: u32, damage: Real) -> bool {
    let managed = w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).and_then(|a| a.managed);
    let (true, Some(f)) = (callable(w, managed), managed) else { return false };
    k(w, id).player = Some(f);
    set_facing_logic(w, f, false);
    crate::fighter::moves::disable_input(w, f);
    set_current_damage(w, f, damage);
    classes::bind(w, DelegateList::Pawn(f, PawnList::KnockedBackIncludeSelf), TutorialObj::Minigame(id), "knockedBack");
    true
}

/// OnPlayerRespawn (index ignored): KBISide (@4258 -> @1808), KBIUp (@3811).
fn player_respawn(w: &mut World, id: u32, up: bool) {
    if !take_player(w, id, if up { 115.0 } else { 100.0 }) {
        return;
    }
    if up {
        delay(w, id, "@15", 1.0);
        return;
    }
    if k(w, id).tutorial_complete {
        return;
    }
    if !solids::valid(&k(w, id).wall) {
        spawn_wall(w, id);
    }
    delay(w, id, "@27", 2.0);
}

/// AttackPlayer: not complete: the bag's NormalAttackPressed(dir), then the Delay that checks it landed.
fn attack_player(w: &mut World, id: u32, up: bool) {
    if k(w, id).tutorial_complete {
        return;
    }
    // TutorialActor.PunchingBag's BPI_NormalAttackPressed(dir)
    let bag = w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).and_then(|a| a.bag);
    if let (true, Some(bag)) = (callable(w, bag), bag) {
        bot_press(w, bag, BotBtn::Attack, Some(if up { Dir::Up } else { Dir::Down }), false);
    }
    if up {
        delay(w, id, "@1759", 1.5);
    } else {
        delay(w, id, "@1056", 0.5);
    }
}

fn attack_done(w: &mut World, id: u32, up: bool) {
    if k(w, id).attacked {
        k(w, id).attacked = false;
    } else {
        attack_player(w, id, up);
    }
}

/// The "attacked" state of the player's knockback: BPI_EnableInput, BPI_DisableAttack, BPI_DisableEvade,
/// BPI_DisableJump.
fn free_to_influence(w: &mut World, f: FighterId) {
    crate::fighter::moves::enable_input(w, f);
    crate::fighter::moves::disable_attack(w, f);
    crate::fighter::moves::disable_evade(w, f);
    crate::fighter::moves::disable_jump(w, f);
}

fn knocked_back_side(w: &mut World, id: u32, f: FighterId, dir_x: Real) {
    k(w, id).attackee = Some(f);
    if dir_x < 0.0 {
        destroy_wall(w, id);
        delay(w, id, "@42", 0.8);
        return;
    }
    free_to_influence(w, f);
    k(w, id).attacked = true;
}

fn knocked_back_up(w: &mut World, id: u32, f: FighterId) {
    let s = k(w, id);
    s.attackee = Some(f);
    s.attacked = true;
    free_to_influence(w, f);
    delay(w, id, "@559", 2.0);
}

/// SpawnWall (@4387): KBISide's.
fn spawn_wall(w: &mut World, id: u32) {
    let count = side(w, id).wall_count;
    let t = tagged(w, "KBIWall", usize::try_from(count % 2).expect("a wall slot"));
    let wall = t.and_then(|t| Held::spawn(w, SolidClass::WallShort, spec_of(id, 0), &t.loc, &t.rot));
    let at = wall.as_ref().map(|h| h.at(w));
    if let Some((x, y)) = at {
        sound_at(w, CAST_SOUND, x, y);
    }
    k(w, id).wall = wall;
    if let (3, Some((x, y))) = (count, at) {
        side(w, id).curve_index = 0;
        move_wall(w, id, x, y, 0);
    }
    side(w, id).wall_count += 1;
}

/// The place of the wall while it is valid.
fn wall_at(w: &mut World, id: u32) -> Option<(Real, Real)> {
    let wall = k(w, id).wall.take();
    let at = wall.as_ref().filter(|h| h.valid()).map(|h| h.at(w));
    k(w, id).wall = wall;
    at
}

/// MoveWall (@5657).
fn move_wall(w: &mut World, id: u32, x: Real, y: Real, i: i32) {
    let Some(spec) = k(w, id).wall.as_ref().filter(|h| h.valid()).map(|h| h.spec) else { return };
    crate::world::stage::runtime_solid_place(w, spec, x, y);
    side(w, id).curve_index = i;
    // PlatformCurve: (dz, seconds)
    const CURVE: [(Real, Real); 4] = [(1200.0, 1.5), (0.0, 0.5), (-1200.0, 1.5), (0.0, 0.5)];
    let (dz, s) = CURVE[usize::try_from(i).expect("a curve index")];
    latent::move_to(w, TutorialObj::Minigame(id), "@2647", MoveTarget::Solid(spec), x, y + dz, s, false, false);
}

/// DestroyWall (@5184).
fn destroy_wall(w: &mut World, id: u32) {
    let Some((x, y)) = wall_at(w, id) else { return };
    minigame::process_event(w, TutorialEvent::KBILeft);
    sound_at(w, name!("sfx_map_shared_rockwall_destruct"), x, y);
    let mut wall = k(w, id).wall.take();
    solids::destroy(w, &mut wall);
    // (Wall = None)
}
