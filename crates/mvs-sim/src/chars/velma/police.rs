//! Velma's police: her side (AnimNotify_Velma_CallPolice, SpawnPoliceServer and
//! its two Delays, joined to timerTick) and the police minion itself, Pawn_VelmaAIBotPolice (the data-only
//! `velmapolice` key the store loads with her: its own character entry, [`VelmaPoliceState`]), with Buff_Arrested
//! ([`ArrestedExt`], Buff_Stun's child: the hit and the hold on the arrested enemy) and VelmaJenkinsPin
//! ([`JenkinsPin`], a world actor that plays its montages). Buff_Stun, Buff_IsPoliceCar and Buff_Light are data
//! classes with no hooks to register (`BuffClass::Data`).
//!
//! Pawn_Velma SpawnPoliceServer @5689-6037 with the Delay 1 @5634 / Delay 8 links; Pawn_VelmaAIBotPolice (the 2 s hold
//! in Tick, ArrestEnabled after its Delay 3, HandleActorCollision with TargetedEnemy, Buff_Arrested 15 s, the capture
//! montage, OnTakeDamage > 18). VelmaPolice_DetectEnemy_BTS, _InInitialDelay_BTD and _HasPerformedArrest_BTD are the
//! bot trees'. Not ported (no caller in the build): Polymorph @5691 and ChangeOffsetOnFacingChange @3162. The car's damage
//! morph (@3340-3649), SetOutline, the skin colour and the siren materials are the view's. A police that rings out stays
//! in the match dead: Velma binds no OnBFDefeated on it; its arrest ends at the victim's next buff tick (BPI_IsDead),
//! which destroys it, and else the Delay 8 does.
//
// Unverified: no match runs a police (`velma-*` matches never reach the CallPolice notify with a full count); the whole
//   minion, its arrest and the pin are ported without a recording and exercised by no trace.
// Unverified: the minion's blackboard (`EnemyActor`, the bot's tree) is the bots' (`ai::bt::BtBot`); `target_read`
//   reads it for a minion with a tree, and a minion with no tree finds no target (as a World with no
// botTrees).
// Unverified: the Mesh's component location is taken as the actor location less 180 (no animated root).
// Unverified: startMontage's other ride-alongs (attack count, section) are taken as harmless for the car.
// Unverified: Buff_Arrested's dealer reads none of its class data (`DestroyOnBlocked` ...): `instDef` is
// passed instead.
// Deviation: the spawn comes in Velma's mesh tick (the notify), taken as the 'OwnerTick' case of the minion spawn.
// Deviation: the entries of her other latent nodes (other files' Delays) are not counted in Delay 1.
// Deviation: the build fires the Delay 8 (made inside the leftover pass, so her entry stays) in her tick of its frame;
//   counted here in the same number of frames from the next, it fires in that frame's leftover pass.
// Deviation: FullBody's begin overlaps are raised once per car tick at its start, over where every body is then (the
//   engine raises them inside the moves and at the hold's teleport).
// Deviation: the build tests OnTakeDamage inside the hit; here in the frame's timer pass of the car.
// Deviation: the pin's asset load is taken as done inside BeginPlay; the Enter montage starts at the pin's first step and
//   the mesh shows one step later; its place on its target is the target's actor location (the head socket is the
//   view's).

use crate::Real;
use crate::State;
use crate::actors::actor_montage::{ActorAnim, MontageEvent, montage_play};
use crate::actors::world_actor::{WorldActor, WorldActorState};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::chars::CharState;
use crate::combat::buffs::{BuffExt, buff, buff_mut, buff_of, end_buff, inst_def_of, set_buff_values, spawn_buff};
use crate::combat::defense::{ArmorPack, add_armor_pack, remove_armor};
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hits::{apply_projectile_hit, dealer};
use crate::combat::holds::apply_stun_buffs;
use crate::combat::packet::HitPacket;
use crate::combat::{BuffClass, BuffRef};
use crate::events::{Event, NotifyRef, UiStateOp, UiStateParams};
use crate::fighter::anim::{add_invisibility, remove_invisibility, start_montage};
use crate::fighter::common::{actor_def, latent_frames, warn_text};
use crate::fighter::control::{data_name, enable_invulnerability, pawn_prop};
use crate::fighter::match_side::post_vo;
use crate::fighter::moves::{Impulse, recall_move};
use crate::fighter::moves::{V2, set_can_walk_off_ledges};
use crate::fighter::traces::{OBJ_PAWN, sphere_overlap_actors};
use crate::fighter::{FighterId, actor_y, facing, in_hitstun, invulnerable, no_hit};
use crate::math::{max, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorHooks, BuffHooks, MeshHooks, WorldActorHooks};
use crate::registry::props::{flag_or, num, obj};
use crate::registry::{ActorEntry, BuffEntry, CharHooks, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit_rounded, spawn_box};
use crate::world::pawns::{FirstTicks, MinionSpawn, destroy_minion, minion_bot, spawn_minion};
use crate::world::spawning::{add_actor, new_world_actor};
use mvs_anim::montage::PROXY_NONE;
use mvs_data::value::{Props, Value};

use super::evidence::{POLICE_EVIDENCE_N, reset_evidence};
use super::state::{VelmaPoliceState, police_of, police_w, velma_mut, velma_of, velma_w};

/// The minion's data key (`MATCH_NEEDS` velma -> velmapolice, `DATA_ONLY`: the store's character index).
pub const POLICE_DATA: &str = "velmapolice";
pub const POLICE_PAWN: Name = crate::name!("Pawn_VelmaAIBotPolice_C");
pub const POLICE_TREE: Name = crate::name!("VelmaPolice_BehaviorTree");
pub const JENKINS_PIN: &str = "VelmaJenkinsPin_C";
const JENKINS_PIN_N: Name = crate::name!("VelmaJenkinsPin_C");
pub const ARRESTED: &str = "Buff_Arrested_C";
const ARRESTED_N: Name = crate::name!("Buff_Arrested_C");
pub const STUN: Name = crate::name!("Buff_Stun_C");
pub const IS_POLICE_CAR: Name = crate::name!("Buff_IsPoliceCar_C");
const CALL_POLICE: &str = "AnimNotify_Velma_CallPolice_C";
const CAPTURE_MONTAGE: Name = crate::name!("Velma_PoliceCar_Capture_Montage");
const PIN_ENTER: Name = crate::name!("Velma_JenkinsPin_Enter_Montage");
const PIN_CAPTURED: Name = crate::name!("Velma_JenkinsPin_Captured_Montage");
const PIN_EXIT: Name = crate::name!("Velma_JenkinsPin_Exit_Montage");
const BUFF_LIGHT: Name = crate::name!("Buff_Light_C");

/// The police's Mesh (CharacterMesh0): RelativeLocation Z -180 under the root capsule (the velmapolice data), scale 0.9.
const MESH_Z: Real = -180.0;
/// FullBody: SphereComponent r80 (scale 2.5, DetectPawn: overlaps Pawn) on the Mesh's fml_un_C_spine_c socket at relative
/// Z 200, which the car's skeleton (Velma_PoliceCar_SkelMesh_Skeleton) lacks, so it sits at the Mesh's origin: world radius
/// 80 x 2.5 x 0.9 (the Mesh's scale) = 180, its centre 200 x 0.9 above the Mesh's origin (-180): the actor location.
const FULL_BODY_R: Real = 180.0;

/// `JenkinsPin` (VelmaJenkinsPin_C, a SpawnedActorParent child): spawned by the police at the origin, Instigator the
/// police (its `owner`); not in the police's SpawnedActors (a deferred spawn, no Comp_Spawner). Its Mesh plays the Enter /
/// Captured / Exit montages (`actors::actor_montage`).
#[derive(State, Debug)]
pub struct JenkinsPin {
    pub x: Real,
    pub y: Real,
    /// The fighter it is pinned to (its head pin, the view).
    pub target: Option<FighterId>,
    /// IsAttachedToCar: Captured @3121 attached it to the car's Mesh Root.
    pub attached_to_car: bool,
    /// Mesh.SetVisibility(true) @676 (hidden until then).
    pub visible: bool,
    pub entered: bool,
    /// The place on the car, in the car's facing frame (Captured @2881-3060).
    pub rel_x: Real,
    pub rel_y: Real,
    /// Ticks, the spawn frame counted (the parent's CheckBlastBoxTimer).
    pub age: i32,
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
    /// Its Mesh's montages.
    pub anim: ActorAnim,
}

impl Default for JenkinsPin {
    fn default() -> Self {
        JenkinsPin {
            x: 0.0,
            y: 0.0,
            target: None,
            attached_to_car: false,
            visible: false,
            entered: false,
            rel_x: 0.0,
            rel_y: 0.0,
            age: 0,
            blast_at: 0.1,
            blast_bounds: BlastBounds::default(),
            anim: ActorAnim::new(crate::name!("Mesh")),
        }
    }
}

/// Buff_Arrested's extension (`ArrestedExt`: `{ car }`).
#[derive(State, Debug, Default)]
pub struct ArrestedExt {
    /// PoliceCar (MulticastSetPoliceCar @4753).
    pub car: Option<FighterId>,
    /// The buff's Instigator: the dealer's `owner: from`. Not in the record.
    #[state(skip)]
    pub from: Option<FighterId>,
}

fn in_world(w: &World, m: FighterId) -> bool {
    w.players.contains(&m) || w.pawns.as_ref().is_some_and(|ps| ps.contains(&m))
}

fn pawn_var(w: &World, m: FighterId, k: &str, def: Real) -> Real {
    pawn_prop(w, m, k).unwrap_or(def)
}

fn sound(fighter: i32, ev: Name) -> Event {
    Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: None }
}

// ---------------------------------------------------------------- Velma's side

/// Pawn_Velma SpawnPoliceServer @5689-6037 and what follows MulticastBotPoliceSetup (@6060 -> @5166-5634). Caller:
/// AnimNotify_Velma_CallPolice (below).
pub fn call_police(w: &mut World, f: FighterId) {
    if !velma_of(w, f).is_some_and(|v| v.allow_police_call) {
        return; // @5689
    }
    // @5699-5908: (X, Y + SelectFloat(-250, 250, FacingLeft), Z + 100); sim x = -Y, so behind her.
    let fc = facing(w, f);
    let x = w.fighter(f).body.x - 250.0 * fc;
    let y = actor_y(w, f) + 100.0;
    // @5963 SpawnAIFromClass(Pawn_VelmaAIBotPolice_C, VelmaPolice_BehaviorTree, no collision fail). The tree is in the bot
    // trees (BL_Velma); a World with no bot trees gives the minion a controller with no tree.
    let tree =
        w.bot_trees.as_ref().is_some_and(|b| b.trees.contains_key(w.names.str(POLICE_TREE))).then_some(POLICE_TREE);
    let m = spawn_minion(
        w,
        MinionSpawn { character: name!("velmapolice"), owner: f, x, y, facing: fc, tree, at: FirstTicks::OwnerTick },
    );
    police_begin_play(w, m); // its ReceiveBeginPlay, inside the spawn
    let v = velma_w(w, f);
    v.allow_police_call = false; // @6026
    v.police = Some(m);
    v.call.last_spawn = Some(m);
    // MulticastBotPoliceSetup @3353-3911, past what the minion spawn writes: Instigator = Velma @3584, SetVisibilityMulticast(true)
    // @3806, EnableInvulnerability(false, false) @3911.
    remove_invisibility(w, m);
    enable_invulnerability(w, m, false, false);
    // @5166 InputCallPolice(false): the UI's. @5181 a Buff_PoliceEvidence is destroyed; @5369-5598 Buff_EvidenceCount's
    // ResetEvidenceCountSever.
    if let Some(pe) = buff_of(w, f, POLICE_EVIDENCE_N) {
        end_buff(w, f, pe);
    }
    reset_evidence(w, f);
    // @5634 Delay(1.0) -> @971. A second call while it is pending is ignored.
    let c = &mut velma_w(w, f).call;
    if c.delay1 <= 0 {
        let extra = if c.delay8 > 0 { 1.0 } else { 0.0 };
        c.delay1 = to_int32(latent_frames(1.0) + extra);
    }
}

/// Received_NotifyBegin: the mesh owner cast to Pawn_Velma -> SpawnPoliceServer @121.
fn call_police_notify(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if velma_of(w, f).is_some() {
        call_police(w, f);
    }
}

/// police.rs's part of the joined timerTick (`velmaTimers`: Pawn_Velma's Delay 1 @971: AllowPoliceCall = true, then
/// Delay(8.0) @982 unless it is pending; and Delay 8 @98: K2_DestroyActor on SpawnPoliceServer's last return value), after
/// evidence.rs's, before beam.rs's.
pub fn timer_tick(w: &mut World, f: FighterId) {
    let Some(v) = velma_mut(w, f) else { return };
    let c = &mut v.call;
    let mut car = None;
    if c.delay8 > 0 {
        c.delay8 -= 1;
        if c.delay8 == 0 {
            car = c.last_spawn;
        }
    }
    if let Some(car) = car {
        destroy_police(w, car);
    }
    let Some(v) = velma_mut(w, f) else { return };
    if v.call.delay1 > 0 {
        v.call.delay1 -= 1;
        if v.call.delay1 == 0 {
            v.allow_police_call = true;
            if v.call.delay8 <= 0 {
                v.call.delay8 = to_int32(latent_frames(8.0));
            }
        }
    }
}

// ---------------------------------------------------------------- the police (Pawn_VelmaAIBotPolice)

/// The minion's `state`: a new [`VelmaPoliceState`].
fn police_state(_w: &World, _f: FighterId) -> CharState {
    CharState::VelmaPolice(VelmaPoliceState::default())
}

/// ReceiveBeginPlay @2766 (after the parent's): Delay(TimeBeforeArrest) @2906, StartupVisuals @2965
/// (sfx_cha_velma_police_cast on itself), InitialLocation @3008, ActivateTargetVisuals @3036 (Delay(0) @1152),
/// Buff_IsPoliceCar @3051 (self, no stacks, no duration). Its Delays are made before its first tick (inside the spawn),
/// so its tick counts them from the spawn frame.
fn police_begin_play(w: &mut World, m: FighterId) {
    let delay3 = to_int32(latent_frames(pawn_var(w, m, "TimeBeforeArrest", 3.0)));
    let id = w.fighter(m).id;
    w.events.push(sound(id, name!("sfx_cha_velma_police_cast"))); // PF_Post_Event(self, self)
    let (x, y) = (w.fighter(m).body.x, actor_y(w, m));
    let s = police_w(w, m);
    s.delay3 = delay3;
    s.initial_x = x;
    s.initial_y = y;
    s.target_loop = true;
    spawn_buff(w, m, IS_POLICE_CAR, 0.0, Some(0), Some(m), true, None);
}

/// The minion's `tick` (`policeTick`): the police's ReceiveTick, then its latent actions.
fn police_tick(w: &mut World, f: FighterId) {
    if police_of(w, f).is_none() || !in_world(w, f) {
        return;
    }
    overlap_pass(w, f);
    // @2589: GetGameTimeSinceCreation > InitialDelay (strict) -> nothing; else StopMovementImmediately and
    // K2_SetActorLocation(InitialLocation, no sweep, teleport).
    let cl = &w.fighter(f).clock;
    let age = cl.time - cl.creation_time;
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(a > b)")]
    if !(age > pawn_var(w, f, "InitialDelay", 2.0)) {
        let fi = w.fighter_mut(f);
        fi.body.vx = 0.0;
        fi.body.vy = 0.0;
        let (x, y) = {
            let s = police_w(w, f);
            (s.initial_x, s.initial_y)
        };
        recall_move(w, f, x, y);
    }
    // The latent actions, in the order they were made: Delay(TimeBeforeArrest) -> @451 ArrestEnabled = true,
    // InitialCheckIfEnemyInsideCollision; Delay(0) -> @492 the target read.
    let s = police_w(w, f);
    if s.delay3 > 0 {
        s.delay3 -= 1;
        if s.delay3 == 0 {
            s.arrest_enabled = true;
            initial_check(w, f);
        }
    }
    if police_of(w, f).is_some_and(|s| s.target_loop) {
        target_read(w, f);
    }
}

/// FullBody's begin-overlap events (BndEvt @3916: HandleActorCollision, then ArrestedVisuals), for every Pawn body that
/// enters the sphere; the class filter BP_FighterCharacter is the cast in HandleActorCollision (a fighter).
fn overlap_pass(w: &mut World, f: FighterId) {
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    let now =
        sphere_overlap_actors(w, x, y, FULL_BODY_R, &[crate::real(OBJ_PAWN)], &[Some(ActorRef::Fighter(f))], -1.0);
    let now: Vec<FighterId> = now
        .into_iter()
        .filter_map(|a| match a {
            ActorRef::Fighter(o) => Some(o),
            _ => None,
        })
        .collect();
    police_w(w, f).inside.retain(|o| now.contains(o));
    for a in now {
        if w.fighter(a).life.dead != 0.0 || police_of(w, f).is_some_and(|s| s.inside.contains(&a)) {
            continue;
        }
        police_w(w, f).inside.push(a);
        handle_collision(w, f, a);
        arrested_visuals(w, f);
    }
}

/// InitialCheckIfEnemyInsideCollision @3954 -> @2111: for each fighter FullBody overlaps now, HandleActorCollision then
/// ArrestedVisuals.
fn initial_check(w: &mut World, f: FighterId) {
    let inside = police_w(w, f).inside.clone();
    for o in inside {
        handle_collision(w, f, o);
        arrested_visuals(w, f);
    }
}

/// HandleActorCollision: not PerformedArrest, ArrestEnabled, the fighter is TargetedEnemy and not IsInvulnerable? ->
/// PerformedArrest = true @303, Buff_Arrested on it (Instigator: the car's, Velma; 15 s) @385, MulticastSetPoliceCar(self)
/// @548, sfx_cha_velma_police_pickup on itself @585.
fn handle_collision(w: &mut World, f: FighterId, o: FighterId) {
    let inv = invulnerable(w, o);
    let s = police_w(w, f);
    if s.performed_arrest || !s.arrest_enabled || s.targeted_enemy != Some(o) || inv {
        return;
    }
    s.performed_arrest = true;
    let creator = w.fighter(f).life.minion_creator;
    let Some(b) = spawn_buff(w, o, ARRESTED_N, 15.0, Some(0), creator, true, None) else { return };
    if let Some(BuffExt::Arrested(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
        e.car = Some(f);
    }
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(sound(id, name!("sfx_cha_velma_police_pickup")));
}

/// ArrestedVisuals @5676: PerformedArrest -> DoOnce @1881: PlayAnimMontage(Capture) @1907, PF_Post_VO_Event(Instigator,
/// ability_busted) @1986, JenkinsPin.Captured() when it is valid @1207.
fn arrested_visuals(w: &mut World, f: FighterId) {
    let s = police_w(w, f);
    if !s.performed_arrest || s.visuals_closed {
        return;
    }
    s.visuals_closed = true;
    let pin = s.pin;
    start_montage(w, f, CAPTURE_MONTAGE, &PROXY_NONE, None);
    // A plain PlayAnimMontage outside the branch manager: no BeginAttack.
    w.fighter_mut(f).charge.begin_attack = false;
    if let Some(c) = w.fighter(f).life.minion_creator {
        post_vo(w, c, name!("ability_busted"), None, 1.0);
    }
    if let Some(ActorRef::Actor(p)) = pin
        && pin_alive(w, p)
    {
        captured(w, p);
    }
}

/// @492: cast<BaseFighter>(the AI controller's Blackboard EnemyActor) -> TargetedEnemy, the pin spawned @725-1114
/// (BeginDeferredActorSpawnFromClass at the origin, Instigator = self, FinishSpawningActor, BPI_UpdateHitbox: the pin has no
/// hit box); else Delay(0) again @1152 (made inside its own latent pass: the next tick).
fn target_read(w: &mut World, f: FighterId) {
    // The blackboard is the minion bot's (`ai::bt::BtBot`): a minion with no tree has none (nothing read).
    // GetValueAsObject(EnemyActor): the key's actor, `cast<BaseFighter>`.
    let enemy: Option<FighterId> = minion_bot(w, f).and_then(|b| match b.bb.obj(b.bb.key("EnemyActor")) {
        Some(ActorRef::Fighter(e)) => Some(e),
        _ => None,
    });
    let Some(e) = enemy else { return };
    let s = police_w(w, f);
    s.target_loop = false;
    s.targeted_enemy = Some(e);
    let pin = spawn_pin(w, f);
    police_w(w, f).pin = pin;
}

/// K2_DestroyActor of the police, then its ReceiveEndPlay -> DestroyVisuals @3959: PS_Velma_PoliceCar_Destroyed at the
/// Mesh's location @4142, JenkinsPin.Escaped() when it is valid @1320. Velma's VelmaState.police is dropped with it (the
/// build keeps a stale reference, which no reader of the sim asks without IsValid).
fn destroy_police(w: &mut World, car: FighterId) {
    if !in_world(w, car) {
        return; // destroyed already: K2_DestroyActor does nothing
    }
    let pin = police_of(w, car).and_then(|s| s.pin);
    destroy_minion(w, car);
    let (id, x, y) = (w.fighter(car).id, w.fighter(car).body.x, (actor_y(w, car) + MESH_Z));
    w.events.push(Event::ProjectileDeath {
        cls: POLICE_PAWN,
        fighter: id,
        vfx: Some(name!("PS_Velma_PoliceCar_Destroyed")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    if let Some(ActorRef::Actor(p)) = pin
        && pin_alive(w, p)
    {
        escaped(w, p);
    }
    if let Some(c) = w.fighter(car).life.minion_creator
        && let Some(v) = velma_mut(w, c)
    {
        if v.police == Some(car) {
            v.police = None;
        }
        if v.call.last_spawn == Some(car) {
            v.call.last_spawn = None;
        }
    }
}

/// OnTakeDamage @3340 (bound in BeginPlay to its Comp_Actor_Damage): CurrentDamage > DamageToKill (18, strict) ->
/// K2_DestroyActor @3773, then sfx_cha_velma_police_destroy at its Capsule's location @3837. The minion's `timerTick`
/// (`policeTimers`).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(a > b): NaN reads as not yet")]
fn police_timers(w: &mut World, f: FighterId) {
    if police_of(w, f).is_none() || !in_world(w, f) {
        return;
    }
    if !(w.fighter(f).stats.damage > pawn_var(w, f, "DamageToKill", 18.0)) {
        return;
    }
    let (id, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    destroy_police(w, f);
    w.events.push(Event::Sound {
        fighter: id,
        ev: name!("sfx_cha_velma_police_destroy"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

// ---------------------------------------------------------------- Buff_Stun / Buff_Arrested

/// The GetOwner Buff_Arrested's own Comp_Pawn_HitBoxHandler credits its hit to (`BuffExt::actor_owner`: the buff's
/// Instigator, the dealer's `owner: from`).
pub fn arrested_owner(e: &ArrestedExt) -> Option<FighterId> {
    e.from
}

/// Buff_Stun ReceiveBeginPlay @1380 -> @543 (authority): BuffDuration = FMax(CalculateDuration, MinimumTime), then the
/// parent's BeginPlay (@10 -> BuffParentActor's, whose LifeTimer takes BuffDuration). CalculateDuration: the owner's
/// Buff_Light CurrentStackAmount x TimePerLifeStack, else 0. MinimumTime is the class default here (Buff_Arrested writes
/// its own after this). ReleaseOnEnemyHit / ReleaseOnAllyHit bind the hit events (both false on Buff_Arrested: not ported).
fn stun_begin(w: &mut World, f: FighterId, b: BuffRef, d: &Props) {
    if crate::registry::props::flag(d, "ReleaseOnEnemyHit") || crate::registry::props::flag(d, "ReleaseOnAllyHit") {
        warn_text(w, "Buff_Stun Buff_Arrested_C: ReleaseOnEnemyHit / ReleaseOnAllyHit not ported");
    }
    let t = match buff_of(w, f, BUFF_LIGHT).and_then(|l| buff(w, l)) {
        Some(l) => l.stacks * num(d, "TimePerLifeStack").unwrap_or(0.0),
        None => 0.0,
    };
    let dur = max(t, num(d, "MinimumTime").unwrap_or(0.0));
    if let Some(x) = buff_mut(w, b) {
        x.frames = if dur > 0.0 { timer_ticks(Some(dur)) + 1.0 } else { Real::INFINITY };
    }
}

/// Buff_Arrested's Stun @2511-2822 (a MakeStruct: StunDuration MinimumTime + 1 and these).
fn arrest_stun(duration: Real) -> Value {
    let mut s = Props::new();
    s.insert("CanStruggle".to_owned(), Value::Bool(true));
    s.insert("MaxStruggleMult".to_owned(), Value::Num(0.55));
    s.insert("StruggleAmount".to_owned(), Value::Num(4.0));
    s.insert("IsGrappledEnemy".to_owned(), Value::Bool(false));
    s.insert("IsGrappledAlly".to_owned(), Value::Bool(false));
    s.insert("StruggleAudio".to_owned(), Value::Null);
    s.insert("StunnedHitReaction".to_owned(), Value::Num(6.0));
    s.insert("SuspendPositionOnGround".to_owned(), Value::Bool(false));
    s.insert("SuspendPositionInAir".to_owned(), Value::Bool(false));
    s.insert("PlayDanceMontage".to_owned(), Value::Bool(false));
    s.insert("StunDuration".to_owned(), Value::Num(f64::from(duration)));
    Value::Obj(s)
}

/// ReceiveBeginPlay @4733: Buff_Stun's (`stun_begin`), then @4594-4051 the hit on the owner through the buff's own
/// Comp_Pawn_HitBoxHandler (pfgDoDamage(null, owner): Damage 35, BaseKnockBack 0, HitPauseTime 0.2, HitStunMult 0,
/// PlayOnHitEffects, ApplyHitPauseToDamageDealer, HitEachActorOnlyOnce, KBI_Multiplier 1; the MakeStruct is the class's
/// HitInfo default but for Damage and HitPauseTime), @4174-4562 MinimumTime by the owner's CurrentDamage after it (<= 50:
/// 3.75, <= 100: 5.5, <= 135: 7.25, else 10), Stun @2822, bCanWalkOffLedges = false @2849, ArrestedText @1467 (the UI state
/// PoliceArrest), AddArmorPack(ArmorOnSpawn) @1481, SetVisibilityMulticast(false) @1548, and a KnockbackLaunchVelocity
/// that is not zero cleared @532.
fn arrested_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Arrested(ArrestedExt { car: None, from });
    }
    let d: Props = inst_def_of(w, b).map(|d| d.into_owned()).unwrap_or_default();
    stun_begin(w, f, b, &d);
    if from.is_none() {
        warn_text(w, "Buff_Arrested without an Instigator: no hit");
    } else if w.fighter(f).life.dead == 0.0 && !no_hit(w, f) {
        let mut h = HitPacket::from_props(&w.names, obj(&d, "HitInfo").unwrap_or(&Props::new()), None);
        h.damage = Some(35.0);
        h.base_knock_back = Some(0.0);
        h.hit_pause_time = Some(0.2);
        h.hit_stun_mult = Some(0.0);
        h.attack_id = Some(0);
        let mut dl = dealer(w, ActorRef::Buff(b));
        dl.attack_id = 0;
        // HitLocation of a null HitCollision: the victim's capsule.
        let at = V2 { x: w.fighter(f).body.x, y: actor_y(w, f) };
        apply_projectile_hit(w, &dl, f, &h, &[], at, None, &[], 0.0);
    }
    let dmg = w.fighter(f).stats.damage;
    let min = if dmg <= 50.0 {
        3.75
    } else if dmg <= 100.0 {
        5.5
    } else if dmg <= 135.0 {
        7.25
    } else {
        10.0
    };
    let mut v = Props::new();
    v.insert("MinimumTime".to_owned(), Value::Num(min));
    v.insert("Stun".to_owned(), arrest_stun(crate::math::narrow(min + 1.0)));
    set_buff_values(w, b, &v);
    set_can_walk_off_ledges(w, f, false);
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: name!("PoliceArrest"),
        state_type: 0,
        display_type: 2,
        current_value: -1.0,
        max_value: -1.0,
        timer: -1.0,
        icon_material: name!("MI_State_ArmorBreak"),
        active: true,
        pop_text: name!("Busted!"),
        auto_remove_time: 3.0,
        subicon: None,
    }));
    let a = obj(&d, "ArmorOnSpawn");
    let n = |k: &str, def: Real| a.and_then(|a| num(a, k)).unwrap_or(def);
    let kind =
        a.and_then(|a| a.get("ArmorType")).and_then(Value::as_str).map_or(name!("Arrested"), |s| data_name(w, s));
    let pack = ArmorPack {
        kind,
        can_override: a.is_some_and(|a| flag_or(a, "CanOverride", false)),
        amount: n("ArmorAmount", 100_000_000.0),
        max_hits: n("MaxHits", 0.0),
        threshold: n("KnockBackThreshold", 8000.0),
        kb_res: n("KnockBackResistanceMultiplier", 0.0),
        dmg_res: n("DamageResistanceMultiplier", 1.0),
        hs_res: n("HitStunResistanceMultiplier", 0.0),
    };
    add_armor_pack(w, f, &pack);
    add_invisibility(w, f);
    if let Some(l) = w.fighter_mut(f).hit.launch.as_mut().filter(|l| !l.di)
        && (l.vx != 0.0 || l.vy != 0.0)
    {
        l.vx = 0.0;
        l.vy = 0.0;
    }
}

/// ReceiveTick @4698 -> @1332: CheckHitstunActive, PoliceCar valid and not BPI_IsDead -> K2_SetActorLocation(its Mesh's
/// location + (0, 0, 200), no sweep, teleport) @1210; else (authority) K2_DestroyActor @617.
fn arrested_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let car = match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::Arrested(e)) => e.car,
        _ => None,
    };
    if in_hitstun(w, f)
        && let Some(car) = car
        && in_world(w, car)
        && w.fighter(car).life.dead == 0.0
    {
        let (x, y) = (w.fighter(car).body.x, ((actor_y(w, car) + MESH_Z) + 200.0));
        recall_move(w, f, x, y);
        return false;
    }
    true
}

/// ReceiveEndPlay @2220 (BuffParentActor's first: the stun's end), bCanWalkOffLedges = true @2338, PoliceCar.K2_DestroyActor
/// when it is valid @2470, PoliceCar = None @1451, LaunchCharacter((0, 0, 1000), true, true) on a Character with a movement
/// component @128, RemoveArmor(Arrested) @179, SetVisibilityMulticast(true) @250. The PoliceArrest UI state is left to its
/// AutoRemoveTime (3 s): the build removes nothing.
fn arrested_end(w: &mut World, f: FighterId, b: BuffRef) {
    let stunned =
        inst_def_of(w, b).and_then(|d| obj(&d, "Stun").and_then(|s| num(s, "StunDuration"))).unwrap_or(0.0) > 0.0;
    apply_stun_buffs(w, f, None, stunned);
    set_can_walk_off_ledges(w, f, true);
    let car = match buff_mut(w, b).map(|x| &mut x.ext) {
        Some(BuffExt::Arrested(e)) => e.car.take(),
        _ => None,
    };
    if let Some(car) = car {
        destroy_police(w, car);
    }
    w.fighter_mut(f).moves.impulse = Some(Impulse { vx: 0.0, vy: 1000.0, xy: true, z: true, kind: 0, d: None });
    remove_armor(w, f, name!("Arrested"));
    remove_invisibility(w, f);
}

// ---------------------------------------------------------------- VelmaJenkinsPin

fn pin_of(w: &World, a: ActorId) -> Option<&JenkinsPin> {
    match &w.actors.actors.get(a)?.state {
        WorldActorState::JenkinsPin(s) => Some(s),
        _ => None,
    }
}

fn pin_mut(w: &mut World, a: ActorId) -> Option<&mut JenkinsPin> {
    match &mut w.actors.actors.get_mut(a)?.state {
        WorldActorState::JenkinsPin(s) => Some(s),
        _ => None,
    }
}

fn pin_alive(w: &World, a: ActorId) -> bool {
    w.actors.actors.get(a).is_some_and(|x| !x.dead)
}

/// The VelmaJenkinsPin's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &JenkinsPin) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The pin, spawned by the police `car` at the origin: BeginPlay @2334: the parent's (the blast-box loop), then (authority)
/// GetTargetedCharacter @2641, AttachToTarget @3577, SetProfilePic @2608 (the Enter montage starts at its first step).
fn spawn_pin(w: &mut World, car: FighterId) -> Option<ActorRef> {
    let Some(p) = actor_def(w, ActorRef::Fighter(car), JENKINS_PIN_N).map(|a| a.p.clone()) else {
        warn_text(w, "spawned actor not exported (VelmaJenkinsPin_C)");
        return None;
    };
    let class = match w.registry.actors.get(JENKINS_PIN).map(|e| e.class) {
        Some(ActorClass::World(c)) => c,
        _ => return None,
    };
    let mut pin = JenkinsPin::default();
    let sb = spawn_box(w);
    let gone = blast_box_loop(
        &sb,
        &mut pin.x,
        &mut pin.y,
        None,
        false,
        &mut pin.blast_bounds,
        flag_or(&p, "DestroyOnExitedArena?", false),
    );
    pin.target = police_of(w, car).and_then(|s| s.targeted_enemy);
    let a = new_world_actor(w, JENKINS_PIN_N, class, car, gone, WorldActorState::JenkinsPin(pin));
    let id = w.actors.actors.insert(a);
    add_actor(w, id, None, false);
    if gone {
        return None;
    }
    follow(w, id);
    Some(ActorRef::Actor(id))
}

fn follow(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let car = a.owner;
    let Some(p) = pin_of(w, id) else { return };
    let (xy, ok) = if p.attached_to_car {
        let (rx, ry) = (p.rel_x, p.rel_y);
        let fc = facing(w, car);
        (((w.fighter(car).body.x + rx * fc), (actor_y(w, car) + ry)), true)
    } else if let Some(t) = p.target {
        ((w.fighter(t).body.x, actor_y(w, t)), true)
    } else {
        ((0.0, 0.0), false)
    };
    if ok && let Some(p) = pin_mut(w, id) {
        (p.x, p.y) = xy;
    }
}

/// Captured @2782: the Instigator's Mesh Root socket + (0, 130, 400) @2970 (sim x - 130), attached to that Mesh (keep world:
/// the place moves and turns with the car) @3060, IsAttachedToCar @3121, the Captured montage with a proxy (OnCompleted ->
/// K2_DestroyActor @1325).
fn captured(w: &mut World, id: ActorId) {
    let Some(car) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let fc = facing(w, car);
    if let Some(p) = pin_mut(w, id) {
        p.attached_to_car = true;
        p.rel_x = -130.0 * fc;
        p.rel_y = MESH_Z + 400.0;
    }
    follow(w, id);
    montage_play(w, ActorRef::Actor(id), PIN_CAPTURED, 1.0, 0.0, true);
}

/// Escaped @3639: the Exit montage with a proxy (OnCompleted -> K2_DestroyActor @1078).
fn escaped(w: &mut World, id: ActorId) {
    montage_play(w, ActorRef::Actor(id), PIN_EXIT, 1.0, 0.0, true);
}

fn pin_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    let destroy_on_exit = actor_def(w, ActorRef::Fighter(owner), JENKINS_PIN_N)
        .is_some_and(|d| flag_or(&d.p, "DestroyOnExitedArena?", false));
    let Some(p) = pin_mut(w, id) else { return };
    p.age += 1;
    let first = !p.entered;
    if first {
        p.entered = true;
    } else {
        p.visible = true;
    }
    if first {
        montage_play(w, ActorRef::Actor(id), PIN_ENTER, 1.0, 0.0, true);
    }
    follow(w, id);
    // The parent's CheckBlastBoxTimer (0.1 s, looping; the next expiry stored rounded, F(blastAt + 0.1), as police.rs).
    let Some(p) = pin_mut(w, id) else { return };
    let (x, y, frames) = (p.x, p.y, crate::real(p.age) - 1.0);
    let gone = blast_timer_exit_rounded(destroy_on_exit, &mut p.blast_at, &p.blast_bounds, frames, x, y);
    if gone && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

fn pin_montage_event(w: &mut World, a: ActorRef, ev: MontageEvent, montage: Name) {
    let ActorRef::Actor(id) = a else { return };
    if ev == MontageEvent::Completed
        && (montage == PIN_CAPTURED || montage == PIN_EXIT)
        && let Some(x) = w.actors.actors.get_mut(id)
    {
        x.dead = true;
    }
}

fn pin_end(w: &mut World, id: ActorId) {
    let Some(owner) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if let Some(s) = crate::chars::velma::state::police_mut(w, owner)
        && s.pin == Some(ActorRef::Actor(id))
    {
        s.pin = None;
    }
}

/// This file's registry entries: AnimNotify_Velma_CallPolice_C, the `velmapolice` character (`policeHooks`),
/// Buff_Arrested_C, VelmaJenkinsPin_C. Velma's own timerTick part is composed in mod.rs.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(CALL_POLICE, NotifyHandler { begin: Some(call_police_notify), ..NotifyHandler::default() })?;
    r.chars.enter(
        POLICE_DATA,
        CharHooks {
            state: Some(police_state),
            tick: Some(police_tick),
            timer_tick: Some(police_timers),
            ..CharHooks::default()
        },
    )?;
    r.buffs.enter(
        ARRESTED,
        BuffEntry::new(
            BuffClass::BuffArrested,
            BuffHooks {
                begin: Some(arrested_begin),
                tick: Some(arrested_tick),
                end: Some(arrested_end),
                ..BuffHooks::default()
            },
        ),
    )?;
    let mut pin = ActorEntry::new(
        ActorClass::World(WorldActorClass::VelmaJenkinsPin),
        ActorHooks::World(WorldActorHooks { step: Some(pin_step), end: Some(pin_end), ..WorldActorHooks::default() }),
    );
    pin.mesh = Some(MeshHooks { montage_event: Some(pin_montage_event), ..MeshHooks::default() });
    r.actors.enter(JENKINS_PIN, pin)
}
