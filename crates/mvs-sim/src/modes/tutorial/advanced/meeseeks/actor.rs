//! Tutorial_Meeseeks: the advanced tutorials' target, a Character with
//! Comp_Actor_Destructible, Comp_Pawn_DamageManager, Comp_Pawn_HitBoxHandler, Comp_HitPause and Comp_Actor_KeepOnCamera
//! (no Comp_Actor_Damage). The minigames and a demo controller spawn it; it runs no tick and never moves. Its hit path is
//! a placed stage actor's (`world/stage_actors.rs`: a hit of a fighter, a projectile or an item reaches
//! DamageAndLaunch's HitByEnemy through the entry's `hit_by_enemy`).
//! - Spawn: BeginDeferredActorSpawnFromClass with CollisionHandlingOverride 0 leaves the class's
//!   SpawnCollisionHandlingMethod, the APawn ctor's 3 (AdjustIfPossibleButDontSpawnIfColliding): PostActorConstruction
//!   runs FindTeleportSpot at the place and moves the root by the adjustment, or destroys the actor when there is none.
//!   With no controller its CharacterMovement runs nothing (no possession at a mid-match spawn, MovementMode None), so it
//!   stays where the spawn put it; GravityScale 0 (AerialCombat) changes nothing.
//! - BeginPlay: Delay 0.01, then (@70) HitByEnemy += On Hit, SpawnEmitterAtLocation(PS_MeeseeksSpawn) with Team 1.0 (the
//!   view's: `shown`), PF_Post_Event_At_Location(sfx_cha_tutorial_meeseeks_cast), OnDestroyed += CustomEvent_0
//!   (PF_Post_Event_At_Location(sfx_map_tutorial_meeseeks_poof) @1305).
//! - On Hit (@439): SpawnEmitterAtLocation(PS_MeeseeksDestroyed) Team 1.0, PopText "+1" at the actor + (0, 0, 200) (both
//!   the view's: `end` Hit), TutorialMinigame_Actor.IncrementMinigameCounter(false) (an unset or destroyed minigame:
//!   skipped), K2_DestroyActor.
//! - ConvertToFlyingMeeseeks: the jet boots and jets shown, Mesh.PlayAnimation(Meeseeks_WaveAir_Montage, true) (the
//!   view's: `flying`).
//!
//! The HitByEnemy binds of the spawners run ahead of its own On Hit (bound 0.01 s later), in bind order.

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, StageActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::{CH_PAWN, ExtraHurt};
use crate::name;
use crate::registry::entries::StageActorEntry;
use crate::world::World;
use crate::world::flow::DelegateCall;
use crate::world::spawn_spot::find_teleport_spot_at;
use crate::world::spawning::SpawnShape;
use crate::world::stage::{next_stage_actor_index, spawn_stage_actor};
use crate::world::stage_actors::{StageActor, StageActorOwn, StageHurt};
use crate::world::stage_query::Shapes;

use super::super::super::TutorialBind;
use super::super::super::TutorialObj;
use super::super::super::classes::{self, Call, Fired};
use super::super::super::latent;
use super::super::super::minigame;
use super::super::super::pawn::sound_at;

/// `MEESEEKS`.
pub const CLS: &str = "Tutorial_Meeseeks_C";
/// The CollisionCylinder's CapsuleHalfHeight.
const HALF: Real = 180.0;
/// Its CapsuleRadius.
const RADIUS: Real = 72.0;

/// How it went (the view's effects at its end): `MeeseeksEnd`.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum MeeseeksEnd {
    #[default]
    None,
    /// Its own On Hit: PS_MeeseeksDestroyed and the "+1" PopText.
    Hit,
    /// A spawner's SpawnEmitterAtLocation(PS_MeeseeksSpawn) ahead of its K2_DestroyActor.
    Poof,
    /// Destroyed with no effect of its own (the cleanups).
    Plain,
}

impl MeeseeksEnd {
    /// The numeric value of the enum `MeeseeksEnd`.
    pub fn code(self) -> u8 {
        match self {
            MeeseeksEnd::None => 0,
            MeeseeksEnd::Hit => 1,
            MeeseeksEnd::Poof => 2,
            MeeseeksEnd::Plain => 3,
        }
    }

    /// [`MeeseeksEnd::code`]'s inverse.
    pub fn of_code(c: u8) -> Option<Self> {
        Some(match c {
            0 => MeeseeksEnd::None,
            1 => MeeseeksEnd::Hit,
            2 => MeeseeksEnd::Poof,
            3 => MeeseeksEnd::Plain,
            _ => return None,
        })
    }
}

/// `MeeseeksActor` beside `StageActor`'s fields, and what its graph binds and holds of other objects (`Binds`: the
/// record writes the first four).
#[derive(State, Debug, Default)]
pub struct MeeseeksOwn {
    /// The world tick of its spawn (its mesh's single-node animation starts there).
    pub spawned: i32,
    /// ConvertToFlyingMeeseeks called (PlayAnimation in the frame of the spawn).
    pub flying: bool,
    /// The world tick of BeginPlay's Delay link (PS_MeeseeksSpawn), -1 before it.
    pub shown: i32,
    pub end: MeeseeksEnd,
    /// Comp_Pawn_DamageManager HitByEnemy (the causer): the spawners' handlers, then its own On Hit.
    pub hit_by_enemy: Vec<TutorialBind>,
    /// TutorialMinigame_Actor (a `TutorialObj::Minigame` id).
    pub minigame: Option<u32>,
    /// BeginPlay's OnDestroyed += CustomEvent_0 made.
    pub on_destroyed_bound: bool,
}

/// `TutorialObj::Stage` of the actor at `slot`.
fn obj(slot: u32) -> TutorialObj {
    TutorialObj::Stage(i32::try_from(slot).expect("a stage actor index"))
}

fn actor(w: &World, slot: u32) -> &StageActor {
    &w.stage_run.as_ref().expect("a stage run").actors[usize::try_from(slot).expect("a stage actor index")]
}

fn actor_mut(w: &mut World, slot: u32) -> &mut StageActor {
    &mut w.stage_run.as_mut().expect("a stage run").actors[usize::try_from(slot).expect("a stage actor index")]
}

/// The actor's own state.
pub fn own(w: &World, slot: u32) -> &MeeseeksOwn {
    match &actor(w, slot).own {
        StageActorOwn::Meeseeks(o) => o,
        _ => panic!("Tutorial_Meeseeks: stage actor {slot} is no Meeseeks"),
    }
}

fn own_mut(w: &mut World, slot: u32) -> &mut MeeseeksOwn {
    match &mut actor_mut(w, slot).own {
        StageActorOwn::Meeseeks(o) => o,
        _ => panic!("Tutorial_Meeseeks: stage actor {slot} is no Meeseeks"),
    }
}

/// The class's entry (`stageActors({[MEESEEKS]: ...})`).
/// Unverified: the CollisionCylinder (object type Pawn) is taken as the shape a hit meets; the mesh's physics asset
/// bodies (CharacterMesh0) were not read.
/// Unverified: SurfaceType 6 for a light hit's sound, as the other actors with no PhysMaterialOverride.
pub fn entry() -> StageActorEntry {
    StageActorEntry {
        class: StageActorClass::TutorialMeeseeks,
        hurt_type: Some(i32::from(CH_PAWN)),
        friendly_fire: false,
        can_take_damage: true,
        hit_pause_multiplier: None,
        surface: Some(6),
        create: None,
        any_damage: None,
        hit_by_enemy: Some(hit_by_enemy),
        hit_by_ally: None,
        knocked_back: None,
        hurt: Some(hurt),
        sounds: &[],
        damage_disabled: None,
        invulnerable: None,
        current_damage: None,
        victim_pause: None,
        mesh: None,
        reset: None,
        solid_hit: None,
    }
}

/// `hurt`: the capsule's segment and radius at the actor (none once dead).
fn hurt(w: &World, slot: u32, out: &mut Vec<StageHurt>) -> bool {
    let a = actor(w, slot);
    if a.dead {
        return false;
    }
    out.push(StageHurt::Up(ExtraHurt { x: a.x, y0: a.y - (HALF - RADIUS), y1: a.y + (HALF - RADIUS), r: RADIUS }));
    true
}

/// `hitByEnemy`: `bindsOf(a).hitByEnemy.broadcast(causer)`: the copy of the list in bind order; a stale entry (its binder
/// destroyed) is swapped out of the live list and not called.
fn hit_by_enemy(w: &mut World, slot: u32, causer: ActorRef, hit: &HitPacket, _dmg: Real) {
    let list = own(w, slot).hit_by_enemy.clone();
    let call = DelegateCall::HitByEnemy { hit: Some(hit), dir_x: 0.0, dir_y: 0.0, dealer: None, by: Some(causer) };
    for b in list {
        if classes::stale(w, b.owner) {
            let l = &mut own_mut(w, slot).hit_by_enemy;
            if let Some(i) = l.iter().position(|&x| x == b) {
                l.swap_remove(i);
            }
            continue;
        }
        classes::delegate(w, b, Some(ActorRef::Stage(slot)), &call);
    }
}

/// `spawnMeeseeks`: BeginDeferredActorSpawnFromClass(Tutorial_Meeseeks_C) + FinishSpawningActor at sim (x, y): `None`
/// when FindTeleportSpot finds no spot (destroyed before BeginPlay). BeginPlay's Delay is started here. The spawners all
/// write TeamIndex 1 right after FinishSpawningActor; nothing reads the team in between, so it is made with it.
/// Unverified: the blockers of the spot test are the stage block alone (the Pawn profile of its capsule against the
/// level's floor box); a fighter's capsule at the place and the encroachment skin are not counted.
pub fn spawn(w: &mut World, x: Real, y: Real) -> Option<ActorRef> {
    let at = find_teleport_spot_at(
        Shapes::boxes(&[w.stage.ground]),
        x,
        y,
        &SpawnShape::Capsule { r: RADIUS, hh: HALF, ux: 0.0, uy: 1.0 },
    );
    if !at.ok {
        return None;
    }
    let index = next_stage_actor_index(w);
    let own = MeeseeksOwn { spawned: w.flow.tick, shown: -1, ..MeeseeksOwn::default() };
    spawn_stage_actor(
        w,
        StageActor {
            cls: StageActorClass::TutorialMeeseeks,
            index,
            team: 1,
            dead: false,
            x: at.x,
            y: at.y,
            own: StageActorOwn::Meeseeks(own),
        },
    );
    latent::delay(w, obj(index), "BeginPlay", 0.01);
    Some(ActorRef::Stage(index))
}

/// `setMeeseeksMinigame`: TutorialMinigame_Actor = the minigame.
pub fn set_minigame(w: &mut World, a: ActorRef, minigame: u32) {
    own_mut(w, slot(a)).minigame = Some(minigame);
}

/// `bindMeeseeksHit`: Comp_Pawn_DamageHandler.HitByEnemy += a spawner's handler (AddUnique; the spawner's destroy makes
/// the entry stale).
pub fn bind_hit(w: &mut World, a: ActorRef, owner: TutorialObj, handler: &'static str) {
    let b = TutorialBind { owner, handler };
    let l = &mut own_mut(w, slot(a)).hit_by_enemy;
    if !l.contains(&b) {
        l.push(b);
    }
}

/// `convertToFlyingMeeseeks`.
pub fn convert_to_flying(w: &mut World, a: ActorRef) {
    own_mut(w, slot(a)).flying = true;
}

fn slot(a: ActorRef) -> u32 {
    match a {
        ActorRef::Stage(i) => i,
        _ => panic!("not a stage actor: {a:?}"),
    }
}

/// `destroyMeeseeks`: K2_DestroyActor: out of the hit tests, its Delay dropped, OnDestroyed (the poof, once BeginPlay
/// bound it). `end`: the effect the caller spawned ahead of the destroy (the view draws it).
/// Not ported: OnDestroyed's broadcast to a binder (nothing of the tutorials binds a Meeseeks' OnDestroyed).
pub fn destroy(w: &mut World, a: ActorRef, end: MeeseeksEnd) {
    let i = slot(a);
    if actor(w, i).dead {
        return;
    }
    actor_mut(w, i).dead = true;
    let bound = {
        let o = own_mut(w, i);
        o.end = end;
        o.on_destroyed_bound
    };
    latent::drop_owner(w, obj(i));
    if bound {
        let (x, y) = (actor(w, i).x, actor(w, i).y);
        sound_at(w, name!("sfx_map_tutorial_meeseeks_poof"), x, y);
    }
}

/// `liveMeeseeks`: GetAllActorsOfClass(Tutorial_Meeseeks_C): the live ones, in spawn order.
pub fn live(w: &World) -> Vec<ActorRef> {
    let Some(r) = w.stage_run.as_ref() else { return Vec::new() };
    let mut out = Vec::new();
    for (i, a) in r.actors.iter().enumerate() {
        if a.cls == StageActorClass::TutorialMeeseeks && !a.dead {
            out.push(ActorRef::Stage(u32::try_from(i).expect("a stage actor index")));
        }
    }
    out
}

/// `meeseeksValid`: IsValid(a Meeseeks reference).
pub fn valid(w: &World, a: Option<ActorRef>) -> bool {
    a.is_some_and(|a| !actor(w, slot(a)).dead)
}

/// A call on the Tutorial_Meeseeks stage actor `i` (`TutorialObj::Stage`).
pub fn call(w: &mut World, i: i32, c: Call<'_>) {
    let slot = u32::try_from(i).expect("a stage actor index");
    match c {
        Call::Fired(Fired::Delay("BeginPlay")) => begin_play(w, slot),
        Call::Delegate { handler: "onHit", .. } => on_hit(w, slot),
        _ => panic!("tutorial: Tutorial_Meeseeks has no handler for {c:?}"),
    }
}

/// BeginPlay's Delay link (@70).
fn begin_play(w: &mut World, slot: u32) {
    let b = TutorialBind { owner: obj(slot), handler: "onHit" };
    let tick = w.flow.tick;
    let (x, y) = (actor(w, slot).x, actor(w, slot).y);
    let o = own_mut(w, slot);
    if !o.hit_by_enemy.contains(&b) {
        o.hit_by_enemy.push(b);
    }
    o.shown = tick;
    sound_at(w, name!("sfx_cha_tutorial_meeseeks_cast"), x, y);
    own_mut(w, slot).on_destroyed_bound = true;
}

/// On Hit (@439).
fn on_hit(w: &mut World, slot: u32) {
    if let Some(m) = own(w, slot).minigame
        && classes::minigame_of(w, m).is_some_and(|m| !m.destroyed)
    {
        minigame::increment_counter(w, m, false);
    }
    destroy(w, ActorRef::Stage(slot), MeeseeksEnd::Hit);
}

/// `stage_destroyed`: whether the actor `i` is destroyed (its binds are stale).
pub fn destroyed(w: &World, i: i32) -> bool {
    let Ok(slot) = usize::try_from(i) else { return true };
    w.stage_run.as_ref().and_then(|r| r.actors.get(slot)).is_none_or(|a| a.dead)
}
