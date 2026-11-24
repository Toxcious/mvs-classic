//! Velma's mark: VelmaMarkProjectile ([`Mark`]: stuck to the ground or a fighter,
//! the target her neutral attack homes on; its step and ReceiveAnyDamage overrides are `actors::projectile`'s arms),
//! VelmaAimProjectile ([`AimProjectile`]), VelmamSassFollowUpProjectile ([`FollowUp`]),
//! Buff_VelmaSpawnHomingTarget and the MarkProjectileVelma / NoMarkProjectileVelma conditions. No world tick
//! caches the World for the buff's begin: the Rust hooks are given it, so it has
//! no
//! part here.
//!
//! VelmaMarkProjectile (offsets are its ubergraph's). Thrown by Velma_Attack_MarkProjectile(_Air)_Montage and the
//! neutral attacks' NoMark spawner, or spawned by the buff below. HitBoxDamage (Damage 1, no knockback, HitReaction 6,
//! DestroyProjectiles, HitEachActorOnlyOnce), HP 1 with DamageDisabled, DestroyOnBlocked, DestroyOnExitedArena?, the
//! Capsule ProjectileNavPlatform, homing 100000.
//! - ReceiveBeginPlay @3444: the LAST ally is the homing target (CheckForHomingTarget @9337), ActivationConditions[1] =
//!   true @3978, the whoosh @481, then the chain: Delay(0.5) -> Velocity x 0.75, Delay(0.01), Delay(0.075) -> Velocity
//!   x 0.75, then AttachedToAlly ? Delay(9) : Delay(6) -> TetherBreakWarning = true, Delay(1) -> K2_DestroyActor @461.
//! - HitEnemyPlayer @6335 and HitAlly @6825 (no parent call): the hit sound, "Spawn Attach Point"(the target) unless
//!   Attached, K2_AttachToActor(the target, SnapToTarget) and the common DoOnce @819-879; an ally also gets
//!   Buff_SpeedUp_C (2.5 s, 1 stack, from the owner) @7103.
//! - The Capsule's ComponentHit @6637: nothing when Attached; else "Spawn Attach Point"(None) (StopSimulating),
//!   AttachToComponent(the hit component, KeepWorld), the hit sound.
//! - ReceiveAnyDamage @4208: the audio gate only (and DamageDisabled keeps it from being called).
//! - Dead in the build, not ported: OnAttachedParentKnockedBack (nothing writes WoundedBuffApplied true), SwapPosition
//!   (no caller), DisableProjectile (C023B classes alone).
//!
//! Buff_VelmaSpawnHomingTarget (duration 5, MaxStacks 1), BeginPlay @584, authority: the Instigator's
//! BPI_CheckConditions([MarkProjectileVelmaCondition]) @743; true -> DestroyPreviousMarkers @911 (every mark of the
//! buff's Instigator, K2_DestroyActor); then CompSpawnActor(VelmaMarkProjectile_C, the Instigator, the OWNER's actor
//! location) @1348 with AfterMarkCreated bound (HitBoxOn? false), Get Spawned Actor(the class, Last), the same team ?
//! mark.HitAlly(owner) @2157 : mark.HitEnemyPlayer(owner) @2373, then K2_DestroyActor @2203.
//!
//! VelmaAimProjectile (the neutral attacks' Mark spawner): HitBoxDamage (5, BKB 1000, KBS 12, HitReaction 4),
//! DestroyOnHitFighter, InitialLifeSpan 2, homing 20000. BeginPlay @3466: the last mark of the owner is AimAtActor;
//! Delay(0.033) -> AimDownProjectileMovement; Delay(0.2) -> homing 13000; the cast sound by ability 0's ammo; Delay(0.6)
//! + Delay(1.0) -> homing off. HandleFighterOverlap @6045, Reflected @4684.
//!
//! VelmamSassFollowUpProjectile (the aim projectile's death spawn after an ally's overlap): BeginPlay @2042 -> @1782:
//! the loop sound, Delay(0.0) -> @15 the nearest enemy pawn to the owner under LocalMinDistance is the homing target,
//! speed 1650, Delay(1.0) -> K2_DestroyActor. HitEnemyPlayer @2057: the VO and the loop's stop. HitAlly @2825: homing
//! off.
//!
//! Model. The mark is a Projectile of the world's list, in the owner's spawned actors. On a fighter it rides that
//! fighter's actor location (SnapToTarget at the root) as Arya's MarkTeleportProjectile does
//! (`chars::arya::knife`), with its blast box check (DestroyOnExitedArena? stays true). Its blocking move is swept
//! (`receive_hit`) against the stage block and the nav blockers.
// Deviation: on a fighter waiting for its respawn the mark stays where the fighter last stood (the build's attach
// parent is the pawn that left the arena; whether it is destroyed or hidden there was not read), and its blast box
// check ends it there.
// Deviation: AttachToComponent(the hit component) on a moving stage piece is not ported: a mark on a platform stays
// where it hit.
// Deviation: the Delay(0.0) of "Spawn Attach Point Visuals" before the VelmaTargeted UI state is not kept (the state
// is added in the tick of the attach).
// Deviation: the aim projectile's follow-up overlap's Pawn list is the match's pawns less those waiting for their
// respawn, in the sim's order (equal distances: the first wins, the build's overlap order was not read).
// Unverified: the Delays follow the convention of the other kinds (a BeginPlay Delay's link runs in the tick where
// age = latentFrames(seconds); a Delay started in a link counts from the next frame); no game read.
// Unverified: the begin overlap of the aim projectile's hit box with a fighter is taken as the hurtbox test of the
// hit, once per begin (a set per projectile), as Shaggy's sandwich.

use super::state::velma_mut;
use crate::Real;
use crate::State;
use crate::actors::projectile::{
    StepEnd, class_def, class_p, expiry, hit_collision, hit_shapes, set_actor_scale, set_hitbox, set_instance_vars,
    stop_simulating,
};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileExt, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{end_buff, spawn_buff};
use crate::combat::elem::{clear_homing, set_homing};
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::{BuffClass, BuffRef, phases};
use crate::events::{Event, SoundKey, UiStateOp, UiStateParams};
use crate::fighter::common::{latent_frames, peers};
use crate::fighter::match_side::post_vo;
use crate::fighter::{FighterId, actor_y, has_equipped};
use crate::math::to_int32;
use crate::math::ue::size2d;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorHooks, BuffHooks, ProjectileHooks, ProjectileReceiveHit, ReflectHooks};
use crate::registry::props::{num, obj};
use crate::registry::{ActorEntry, BuffEntry, Condition, Registry, RegistryError};
use crate::world::World;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::{comp_spawn_projectile, destroy_projectile};
use indexmap::IndexSet;
use mvs_data::value::Value;

pub const MARK: &str = "VelmaMarkProjectile_C";
pub const AIM_PROJECTILE: &str = "VelmaAimProjectile_C";
pub const SASS_FOLLOW_UP: &str = "VelmamSassFollowUpProjectile_C";
pub const SPAWN_HOMING_TARGET: &str = "Buff_VelmaSpawnHomingTarget_C";

const MARK_N: Name = name!("VelmaMarkProjectile_C");
const SASS_FOLLOW_UP_N: &str = SASS_FOLLOW_UP;
/// The UI state's id (@2201, @9202).
const TARGETED: Name = name!("VelmaTargeted");
const SPEED_UP: Name = name!("Buff_SpeedUp_C");
const MARK_HIT: Name = name!("sfx_cha_velma_attack_markprojectile_hit");
const WHOOSH: Name = name!("sfx_cha_velma_attack_normal_neutral_whoosh");
const REFLECT_SOUND: Name = name!("sfx_cha_velma_attack_normal_neutral_hit_tier03");
const LOOP_TAG: Name = name!("velmasass");
const LOOP_SOUND: Name = name!("sfx_cha_velma_throw_book_loop");
const LOOP_STOP: Name = name!("sfx_cha_velma_throw_book_loop_stop");
const FOLLOW_SPEED: Real = 1650.0;

// The BeginPlay chain's ages (the tick each link runs in).
fn slow1() -> Real {
    latent_frames(0.5)
}
fn slow2() -> Real {
    slow1() + latent_frames(0.01) + latent_frames(0.075)
}
/// Delay(WoundedBuffApplied ? 5 : 1): never wounded.
fn tail() -> Real {
    latent_frames(1.0)
}

/// `Mark` (VelmaMarkProjectile_C).
#[derive(State, Debug)]
pub struct Mark {
    /// ActivationConditions (class default [true, false]; BeginPlay @3978 and the DoOnce write entry 1 true).
    pub conds: Vec<bool>,
    /// The template's ProjectileMovement.HomingAccelerationMagnitude.
    pub home_mag: Real,
    /// Attached: "Spawn Attach Point"(a fighter).
    pub stuck: bool,
    /// The attach parent (K2_AttachToActor); AttachedEnemy is it from the DoOnce on.
    pub target: Option<FighterId>,
    /// AttachedToAlly.
    pub to_ally: bool,
    /// AttachedToEnemy.
    pub to_enemy: bool,
    /// Stopped by the Capsule's hit @6637 (Attached stays false).
    pub ground: bool,
    /// The common DoOnce @819.
    pub gate_done: bool,
    /// TetherBreakWarning @307.
    pub warning: bool,
    /// The age the Delay(9 | 6) ends at (set at the second slow).
    pub life_at: Real,
    /// The age K2_DestroyActor @461 runs at.
    pub die_at: Real,
}

impl Default for Mark {
    /// The field initial values.
    fn default() -> Self {
        Mark {
            conds: Vec::new(),
            home_mag: 0.0,
            stuck: false,
            target: None,
            to_ally: false,
            to_enemy: false,
            ground: false,
            gate_done: false,
            warning: false,
            life_at: -1.0,
            die_at: -1.0,
        }
    }
}

/// `AimProjectile` (VelmaAimProjectile_C).
#[derive(State, Debug)]
pub struct AimProjectile {
    pub home_mag: Real,
    /// AimAtActor: the owner's last mark.
    pub aim_at: Option<ActorRef>,
    /// BigHit.
    pub big_hit: bool,
    /// HitLogic's DoOnce.
    pub hit_once: bool,
    /// The overlap's Delay(0.1).
    pub destroy_at: Real,
    /// ReflectedMulticast's Delay(0.066).
    pub reflect_at: Real,
    /// HasBeenReflected.
    pub reflected: bool,
    /// InitialLifeSpan, in ticks.
    pub life_at: Real,
    /// The fighters (ids) the hit box overlaps (the begin overlap's edge).
    pub inside: IndexSet<i32>,
}

impl Default for AimProjectile {
    /// The field initial values.
    fn default() -> Self {
        AimProjectile {
            home_mag: 0.0,
            aim_at: None,
            big_hit: false,
            hit_once: false,
            destroy_at: -1.0,
            reflect_at: -1.0,
            reflected: false,
            life_at: 0.0,
            inside: IndexSet::new(),
        }
    }
}

/// `FollowUp` (VelmamSassFollowUpProjectile_C).
#[derive(State, Debug, Default)]
pub struct FollowUp {
    pub home_mag: Real,
    /// Delay(0.0) -> @15.
    pub search_at: Real,
    /// Delay(1.0) -> K2_DestroyActor.
    pub die_at: Real,
    /// The loop sound is playing.
    pub loop_on: bool,
    /// LocalMinDistance (written as the search goes).
    pub min_dist: Real,
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn mk(w: &World, id: ProjectileId) -> &Mark {
    match &pr(w, id).ext {
        ProjectileExt::Mark(m) => m,
        e => panic!("VelmaMarkProjectile_C without its state: {e:?}"),
    }
}

fn mk_mut(w: &mut World, id: ProjectileId) -> &mut Mark {
    match &mut pm(w, id).ext {
        ProjectileExt::Mark(m) => m,
        e => panic!("VelmaMarkProjectile_C without its state: {e:?}"),
    }
}

fn aim(w: &World, id: ProjectileId) -> &AimProjectile {
    match &pr(w, id).ext {
        ProjectileExt::AimProjectile(m) => m,
        e => panic!("VelmaAimProjectile_C without its state: {e:?}"),
    }
}

fn aim_mut(w: &mut World, id: ProjectileId) -> &mut AimProjectile {
    match &mut pm(w, id).ext {
        ProjectileExt::AimProjectile(m) => m,
        e => panic!("VelmaAimProjectile_C without its state: {e:?}"),
    }
}

fn fol(w: &World, id: ProjectileId) -> &FollowUp {
    match &pr(w, id).ext {
        ProjectileExt::FollowUp(m) => m,
        e => panic!("VelmamSassFollowUpProjectile_C without its state: {e:?}"),
    }
}

fn fol_mut(w: &mut World, id: ProjectileId) -> &mut FollowUp {
    match &mut pm(w, id).ext {
        ProjectileExt::FollowUp(m) => m,
        e => panic!("VelmamSassFollowUpProjectile_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a Velma projectile has an owner")
}

/// The template's ProjectileMovement.HomingAccelerationMagnitude (`homingOf`).
fn homing_of(w: &World, p: &Projectile) -> Real {
    class_def(w, p)
        .and_then(|d| obj(&d.components, "ProjectileMovement"))
        .and_then(|c| num(c, "HomingAccelerationMagnitude"))
        .unwrap_or(0.0)
}

/// `sound`: an event of the owner at the projectile.
fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let p = pr(w, id);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

/// The mark's `attachedTo` getter (`world_actor::attached_to`).
pub fn attached_to(_p: &Projectile, m: &Mark) -> Option<FighterId> {
    m.target
}

/// The mark's step override (`Mark.step`): on a fighter it rides his actor location and its timers run on. `None`
/// when the parent's step runs (no target, or dead), else the override's result.
pub fn step_override(w: &mut World, id: ProjectileId) -> Option<Option<StepEnd>> {
    let t = mk(w, id).target?;
    if pr(w, id).dead {
        return None;
    }
    let alive = w.fighter(t).life.dead <= 0.0;
    let (tx, ty) = (w.fighter(t).body.x, actor_y(w, t));
    let p = pm(w, id);
    p.prev_x = p.x;
    p.prev_y = p.y;
    p.age += 1;
    if alive {
        p.x = tx;
        p.y = ty;
    }
    Some(expiry(w, id))
}

/// The mark's ReceiveAnyDamage override (@4208, no parent call; DamageDisabled keeps it from being called): false.
pub fn damaged() -> bool {
    false
}

/// The owner's first live mark (Comp_Spawner.SpawnedActors filtered by the class, entry 0: a destroyed actor leaves
/// the list at its EndPlay), less `but`.
fn first_mark(w: &World, f: FighterId, but: Option<ProjectileId>) -> Option<ProjectileId> {
    w.fighter(f).items.spawned_actors.iter().find_map(|a| match *a {
        ActorRef::Projectile(id) if Some(id) != but => w
            .actors
            .projectiles
            .get(id)
            .filter(|p| p.cls == MARK_N && !p.dead && matches!(p.ext, ProjectileExt::Mark(_)))
            .map(|_| id),
        _ => None,
    })
}

/// Get Spawned Actor(the class, Last): the last live one.
fn last_mark(w: &World, f: FighterId) -> Option<ProjectileId> {
    w.fighter(f).items.spawned_actors.iter().rev().find_map(|a| match *a {
        ActorRef::Projectile(id) => w
            .actors
            .projectiles
            .get(id)
            .filter(|p| p.cls == MARK_N && !p.dead && matches!(p.ext, ProjectileExt::Mark(_)))
            .map(|_| id),
        _ => None,
    })
}

/// `syncMark`: `VelmaState.mark` is the first live mark of the owner.
fn sync_mark(w: &mut World, f: FighterId, but: Option<ProjectileId>) {
    let m = first_mark(w, f, but);
    if let Some(s) = velma_mut(w, f) {
        s.mark = m;
    }
}

/// "Spawn Attach Point"(t) @4327: StopSimulating; a valid target sets Attached.
fn spawn_attach_point(w: &mut World, id: ProjectileId, t: Option<FighterId>) {
    stop_simulating(pm(w, id));
    if t.is_some() {
        mk_mut(w, id).stuck = true;
    }
    // Spawn Attach Point Visuals @4662 -> @1420-2580: the bubble, then a Delay(0.0) and a DoOnce @2102 that adds the UI
    // state VelmaTargeted to the target (@2128-2526). A wall stop (None) closes that DoOnce with no broker: no later
    // target gets it. CleanUpUI removes it (end).
    if let Some(t) = t
        && !mk(w, id).ground
    {
        w.fighter_mut(t).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
            id: TARGETED,
            state_type: 1,
            display_type: 0,
            current_value: -1.0,
            max_value: -1.0,
            timer: -1.0,
            icon_material: name!("MI_State_Targeted"),
            active: true,
            pop_text: Name::NONE,
            auto_remove_time: -1.0,
            subicon: None,
        }));
    }
}

/// K2_AttachToActor(t, None, SnapToTarget location, KeepWorld rotation and scale, weld).
fn attach_to(w: &mut World, id: ProjectileId, t: FighterId) {
    let (tx, ty) = (w.fighter(t).body.x, actor_y(w, t));
    let m = mk_mut(w, id);
    m.target = Some(t);
    m.ground = false;
    let p = pm(w, id);
    p.x = tx;
    p.y = ty;
}

/// The common DoOnce @819-879.
fn attach_gate(w: &mut World, id: ProjectileId) {
    if mk(w, id).gate_done {
        return;
    }
    let m = mk_mut(w, id);
    m.gate_done = true;
    m.stuck = true;
    if m.conds.len() < 2 {
        m.conds.resize(2, false);
    }
    m.conds[1] = true;
    // SetActorEnableCollision(false): no hit box, no hurt box, no capsule (the ride moves it).
    let p = pm(w, id);
    set_hitbox(p, false);
    p.hurt_radius = 0.0;
}

/// HitEnemyPlayer @6335.
fn hit_enemy_of(w: &mut World, id: ProjectileId, vic: FighterId) {
    sound(w, id, MARK_HIT);
    if !mk(w, id).stuck {
        spawn_attach_point(w, id, Some(vic));
    }
    attach_to(w, id, vic);
    mk_mut(w, id).to_enemy = true;
    attach_gate(w, id);
}

/// HitAlly @6825.
fn hit_ally_of(w: &mut World, id: ProjectileId, vic: FighterId) {
    sound(w, id, MARK_HIT);
    if !mk(w, id).stuck {
        mk_mut(w, id).to_ally = true;
        spawn_attach_point(w, id, Some(vic));
    }
    attach_to(w, id, vic);
    let from = pr(w, id).owner;
    spawn_buff(w, vic, SPEED_UP, 2.5, Some(1), from, true, None); // @7103
    attach_gate(w, id);
}

/// The constructor's tail: `conds` from the class's ActivationConditions, the movement's homing magnitude.
fn mark_construct(w: &World, p: &mut Projectile) {
    let conds = class_p(w, p)
        .get("ActivationConditions")
        .and_then(mvs_data::value::Value::as_arr)
        .map_or_else(Vec::new, |a| a.iter().map(crate::registry::props::truthy).collect());
    let home_mag = homing_of(w, p);
    p.ext = ProjectileExt::Mark(Mark { conds, home_mag, ..Mark::default() });
}

fn mark_begin(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    // CheckForHomingTarget: the last ally (GetAllies: the owner's team, players only, none waiting for its respawn).
    let team = w.fighter(f).team.team;
    let mut ally = None;
    for &o in peers(w) {
        let b = w.fighter(o);
        if o != f && b.team.team == team && b.life.dead <= 0.0 && !b.life.is_minion {
            ally = Some(o);
        }
    }
    if let Some(a) = ally {
        let mag = mk(w, id).home_mag;
        set_homing(w, ActorRef::Projectile(id), ActorRef::Fighter(a), mag);
    }
    let quiet = velma_mut(w, f).is_some_and(|s| s.marks.quiet_spawn);
    let m = mk_mut(w, id);
    if m.conds.len() < 2 {
        m.conds.resize(2, false);
    }
    m.conds[1] = true;
    if quiet {
        set_hitbox(pm(w, id), false); // AfterMarkCreated
    }
    sound(w, id, WHOOSH); // @481
    sync_mark(w, f, None);
}

/// The Capsule's blocking move: none once stuck, stopped or on a fighter; else the nav blockers and the polygon solids.
fn mark_boxes(w: &mut World, id: ProjectileId, out: &mut crate::world::stage_query::StageShapes) {
    out.boxes.clear();
    out.polys.clear();
    if mk(w, id).stuck || pr(w, id).stopped || mk(w, id).target.is_some() {
        return;
    }
    w.blocker_shapes_into(BlockKind::Nav, out);
}

/// The Capsule's ComponentHit @6637; HandleImpact stops the movement after it.
fn mark_hit(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, _t: Real, _pen: bool) -> bool {
    if mk(w, id).stuck {
        return false;
    }
    spawn_attach_point(w, id, None);
    mk_mut(w, id).ground = true;
    sound(w, id, MARK_HIT);
    false
}

/// The latent actions, in the order they were started.
fn mark_step(w: &mut World, id: ProjectileId) -> bool {
    let a = crate::real(pr(w, id).age);
    if a == slow1() || a == slow2() {
        let p = pm(w, id);
        p.vx *= 0.75;
        p.vy *= 0.75;
    }
    if a == slow2() {
        let n = if mk(w, id).to_ally { 9.0 } else { 6.0 };
        mk_mut(w, id).life_at = a + latent_frames(n);
    }
    if a == mk(w, id).life_at {
        let m = mk_mut(w, id);
        m.warning = true;
        m.die_at = a + tail();
    }
    a == mk(w, id).die_at
}

fn mark_hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    if !pr(w, id).dead {
        hit_enemy_of(w, id, vic);
    }
}

fn mark_hit_ally(w: &mut World, id: ProjectileId, vic: FighterId) {
    hit_ally_of(w, id, vic);
}

fn mark_end(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    sync_mark(w, f, Some(id));
    // CleanUpUI(Target) @9579 (the expiry @424, ReceiveEndPlay @2926, ReceiveDestroyed @9594): RemoveStateForPlayer
    // VelmaTargeted on each valid entry of Target, the fighter "Spawn Attach Point" was given (the attach parent).
    let m = mk(w, id);
    if let (true, Some(t)) = (m.stuck, m.target) {
        w.fighter_mut(t).controls.ui_state_ops.push(UiStateOp::Remove(TARGETED));
    }
}

/// Buff_VelmaSpawnHomingTarget's BeginPlay @584.
fn spawn_homing_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    // The Instigator's BPI_BaseFighter cast fails: the condition reads false (@2218) and @925's spawner is not valid:
    // the buff stays (pop_flow) for its duration.
    let Some(from) = from else { return };
    // DestroyPreviousMarkers @2452: every mark of the buff's Instigator.
    if mark_check(w, from) {
        let ids: Vec<ProjectileId> = w.actors.projectiles.iter().map(|(id, _)| id).collect();
        for id in ids {
            let hit = w.actors.projectiles.get(id).is_some_and(|p| p.cls == MARK_N && !p.dead && p.owner == Some(from));
            if hit {
                destroy_projectile(w, id, false);
            }
        }
    }
    if let Some(s) = velma_mut(w, from) {
        s.marks.quiet_spawn = true;
    }
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    comp_spawn_projectile(w, from, MARK_N, x, y, [Some(0.0); 3], 0.0, 0.0, None);
    if let Some(s) = velma_mut(w, from) {
        s.marks.quiet_spawn = false;
    }
    if let Some(m) = last_mark(w, from) {
        if w.fighter(from).team.team == w.fighter(f).team.team {
            hit_ally_of(w, m, f);
        } else {
            hit_enemy_of(w, m, f);
        }
    }
    end_buff(w, f, b); // @2203
}

/// MarkProjectileVelmaCondition.Check: the actor's Comp_Spawner, FilterArray(SpawnedActors, the mark)[0] a
/// BPI_SpawnedActor -> its ActivationConditions[1] && !IsEquippedComponents.contains('TeleportMarker'); else false.
fn mark_check(w: &World, f: FighterId) -> bool {
    first_mark(w, f, None).is_some_and(|m| {
        mk(w, m).conds.get(1).copied().unwrap_or(false) && !has_equipped(w, f, name!("teleportmarker"))
    })
}

/// MarkProjectileVelmaCondition_C (`markCheck`).
fn mark_condition(w: &mut World, f: FighterId) -> bool {
    mark_check(w, f)
}

/// NoMarkProjectileVelmaCondition_C (`!markCheck`).
fn no_mark_condition(w: &mut World, f: FighterId) -> bool {
    !mark_check(w, f)
}

// ---------------------------------------------------------------- VelmaAimProjectile
/// The sounds by GetCurrentAbilityData(0).CurrentAmmo (the select's default: None).
fn cast_tier(ammo: usize) -> Option<Name> {
    match ammo {
        1 => Some(name!("sfx_cha_velma_attack_normal_neutral_cast_tier04")),
        2 => Some(name!("sfx_cha_velma_attack_normal_neutral_cast_tier03")),
        3 => Some(name!("sfx_cha_velma_attack_normal_neutral_cast_tier02")),
        4 => Some(name!("sfx_cha_velma_attack_normal_neutral_cast_tier01")),
        _ => None,
    }
}

fn hit_tier(ammo: usize) -> Option<Name> {
    match ammo {
        0 => Some(name!("sfx_cha_velma_attack_normal_neutral_hit_tier04")),
        1 => Some(name!("sfx_cha_velma_attack_normal_neutral_hit_tier03")),
        2 => Some(name!("sfx_cha_velma_attack_normal_neutral_hit_tier02")),
        3 => Some(name!("sfx_cha_velma_attack_normal_neutral_hit_tier01")),
        _ => None,
    }
}

fn aim_down_at() -> Real {
    latent_frames(0.033)
}
fn accel_at() -> Real {
    latent_frames(0.2)
}
fn homing_off_at() -> Real {
    latent_frames(0.6) + latent_frames(1.0)
}

/// `f.abilities[0]?.ammo ?? 0` as a table index: a count that is no index reads the select's default.
fn ammo(w: &World, f: FighterId) -> Option<usize> {
    let a = w.fighter(f).abilities.abilities.first().map_or(0.0, |s| s.ammo);
    if a.fract() != 0.0 {
        return None;
    }
    usize::try_from(to_int32(a)).ok()
}

/// `castSound`: the cast sound by ammo on the owner, then the whoosh at the projectile.
fn cast_sound(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let ev = ammo(w, f).and_then(cast_tier);
    if let Some(ev) = ev {
        let fighter = pr(w, id).owner_id;
        w.events.push(Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: None });
    }
    sound(w, id, WHOOSH);
}

/// HitLogic @1620.
fn hit_logic(w: &mut World, id: ProjectileId) {
    if aim(w, id).hit_once {
        return;
    }
    aim_mut(w, id).hit_once = true;
    let f = owner(w, id);
    if let Some(ev) = ammo(w, f).and_then(hit_tier) {
        sound(w, id, ev);
    }
    if aim(w, id).big_hit {
        cast_sound(w, id);
    }
}

fn aim_construct(w: &World, p: &mut Projectile) {
    let home_mag = homing_of(w, p);
    let life_at = timer_ticks(num(class_p(w, p), "InitialLifeSpan"));
    p.ext = ProjectileExt::AimProjectile(AimProjectile { home_mag, life_at, ..AimProjectile::default() });
}

fn aim_begin(w: &mut World, id: ProjectileId) {
    // @4621: the last of the owner's list of the exact class.
    let f = owner(w, id);
    let at = last_mark(w, f);
    aim_mut(w, id).aim_at = at.map(ActorRef::Projectile);
    // @3891: HomingTargetComponent = its root (the template's homing on).
    if let Some(m) = at {
        let mag = aim(w, id).home_mag;
        set_homing(w, ActorRef::Projectile(id), ActorRef::Projectile(m), mag);
    }
    cast_sound(w, id); // @4224, @4403
}

fn aim_step(w: &mut World, id: ProjectileId) -> bool {
    let a = crate::real(pr(w, id).age);
    if a == aim_down_at() {
        // @310
        set_instance_vars(w, id, vec![("AimDownProjectileMovement".to_owned(), Value::Bool(true))]);
    }
    if a == accel_at()
        && let Some(h) = &mut pm(w, id).buff_home
    {
        h.mag = 13000.0; // @15
    }
    if a == homing_off_at() {
        clear_homing(w, ActorRef::Projectile(id)); // @421
    }
    // @72: the look-at rotation (the drawing's, from the velocity) and the sound.
    if a == aim(w, id).reflect_at {
        sound(w, id, REFLECT_SOUND);
    }
    if a == aim(w, id).destroy_at {
        return true;
    }
    // InitialLifeSpan, until a reflect's SetLifeSpan took its place.
    let s = aim(w, id).life_at;
    pr(w, id).reflect.is_none() && s > 0.0 && a >= s
}

/// HandleFighterOverlap @6045, at the begin of the hit box's overlap.
fn aim_overlap(w: &mut World, id: ProjectileId, vic: FighterId) {
    let mut on = hit_collision(pr(w, id))
        && w.fighter(vic).life.dead <= 0.0
        && !crate::combat::phases::no_hit_from(w, ActorRef::Projectile(id), vic);
    if on {
        on = match phases::victim_pose(w, vic) {
            Some(vp) => {
                let mut sp: Vec<WorldSphere> = Vec::new();
                hit_shapes(w, pr(w, id), &mut sp);
                let met = crate::combat::hit::hurt::hurt_hit(w, vic, &sp, &vp).is_some();
                phases::give_vbuf(w, vp);
                met
            }
            None => false,
        };
    }
    let vid = w.fighter(vic).id;
    if !on {
        aim_mut(w, id).inside.shift_remove(&vid);
        return;
    }
    if !aim_mut(w, id).inside.insert(vid) {
        return;
    }
    let f = owner(w, id);
    if w.fighter(vic).team.team != w.fighter(f).team.team {
        aim_mut(w, id).big_hit = false;
    } else if vic != f {
        aim_mut(w, id).big_hit = true;
        // BoomMulticast(true): the explosion particle (cosmetic). The death spawn: World.destroyProjectile.
        set_instance_vars(
            w,
            id,
            vec![
                ("SpawnActorAtDeath".to_owned(), Value::Bool(true)),
                ("ActorClassSpawnedAtDeath".to_owned(), Value::Str(SASS_FOLLOW_UP_N.to_owned())),
            ],
        );
    } else {
        return;
    }
    hit_logic(w, id);
    if aim(w, id).destroy_at < 0.0 {
        // a Delay already counting ignores the call
        let age = crate::real(pr(w, id).age);
        aim_mut(w, id).destroy_at = age + latent_frames(0.1);
    }
}

/// Reflected @4684 and ReflectedMulticast @4748.
fn aim_reflected(w: &mut World, a: ActorRef) {
    let ActorRef::Projectile(id) = a else { return };
    set_instance_vars(w, id, vec![("AimDownProjectileMovement".to_owned(), Value::Bool(false))]);
    clear_homing(w, a);
    aim_mut(w, id).reflected = true;
    set_actor_scale(pm(w, id), 1.5);
    if aim(w, id).reflect_at < 0.0 {
        let age = crate::real(pr(w, id).age);
        aim_mut(w, id).reflect_at = age + latent_frames(0.066);
    }
}

// ---------------------------------------------------------------- VelmamSassFollowUpProjectile
fn follow_construct(w: &World, p: &mut Projectile) {
    let home_mag = homing_of(w, p);
    let min_dist = num(class_p(w, p), "LocalMinDistance").unwrap_or(0.0);
    p.ext = ProjectileExt::FollowUp(FollowUp {
        home_mag,
        search_at: latent_frames(0.0),
        die_at: -1.0,
        loop_on: false,
        min_dist,
    });
}

fn loop_key(w: &World, id: ProjectileId) -> SoundKey {
    SoundKey { tag: LOOP_TAG, n: pr(w, id).seq }
}

fn follow_begin(w: &mut World, id: ProjectileId) {
    fol_mut(w, id).loop_on = true;
    let (fighter, key) = (pr(w, id).owner_id, loop_key(w, id));
    w.events.push(Event::SoundStart { fighter, ev: LOOP_SOUND, key });
}

/// @15: the nearest pawn of another team to the owner, then CheckForHomingTarget and the speeds.
fn follow_search(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let (fx, fy, team) = (w.fighter(f).body.x, actor_y(w, f), w.fighter(f).team.team);
    let mut best = None;
    let mut min = fol(w, id).min_dist;
    for &o in peers(w) {
        let b = w.fighter(o);
        if b.team.team == team || b.life.dead > 0.0 {
            continue;
        }
        let d = size2d([b.body.x - fx, actor_y(w, o) - fy]);
        if d < min {
            min = d;
            best = Some(o);
        }
    }
    fol_mut(w, id).min_dist = min;
    if let Some(o) = best {
        let mag = fol(w, id).home_mag;
        set_homing(w, ActorRef::Projectile(id), ActorRef::Fighter(o), mag);
    }
    pm(w, id).max_speed = FOLLOW_SPEED; // InitialSpeed is read at the spawn alone
    let age = crate::real(pr(w, id).age);
    fol_mut(w, id).die_at = age + latent_frames(1.0);
}

fn follow_step(w: &mut World, id: ProjectileId) -> bool {
    let a = crate::real(pr(w, id).age);
    if a == fol(w, id).search_at {
        follow_search(w, id);
    }
    a == fol(w, id).die_at
}

fn follow_hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    let f = owner(w, id);
    post_vo(w, f, name!("ability_generic"), Some(vic), 0.1);
    if fol(w, id).loop_on {
        fol_mut(w, id).loop_on = false;
        let (fighter, key) = (pr(w, id).owner_id, loop_key(w, id));
        w.events.push(Event::SoundStop { fighter, key, fade: None, ev: Some(LOOP_STOP), curve: None, cut: false });
    }
}

fn follow_hit_ally(w: &mut World, id: ProjectileId, _vic: FighterId) {
    clear_homing(w, ActorRef::Projectile(id)); // @1734
}

/// The loop's event is posted with bStopWhenAttachedToDestroyed (Lib_Audio PF_Post_Event): it ends with the actor.
fn follow_end(w: &mut World, id: ProjectileId) {
    if !fol(w, id).loop_on {
        return;
    }
    fol_mut(w, id).loop_on = false;
    let (fighter, key) = (pr(w, id).owner_id, loop_key(w, id));
    w.events.push(Event::SoundStop { fighter, key, fade: None, ev: None, curve: None, cut: false });
}

/// This file's registry entries: Buff_VelmaSpawnHomingTarget_C, MarkProjectileVelmaCondition_C,
/// NoMarkProjectileVelmaCondition_C, VelmaMarkProjectile_C, VelmaAimProjectile_C, VelmamSassFollowUpProjectile_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        SPAWN_HOMING_TARGET,
        BuffEntry::new(
            BuffClass::BuffVelmaSpawnHomingTarget,
            BuffHooks { begin: Some(spawn_homing_begin), ..BuffHooks::default() },
        ),
    )?;
    r.conditions.enter("MarkProjectileVelmaCondition_C", Condition::Fn(mark_condition))?;
    r.conditions.enter("NoMarkProjectileVelmaCondition_C", Condition::Fn(no_mark_condition))?;
    r.actors.enter(
        MARK,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::VelmaMarkProjectile),
            ActorHooks::Projectile(ProjectileHooks {
                construct: Some(mark_construct),
                begin: Some(mark_begin),
                receive_hit: Some(ProjectileReceiveHit { boxes: mark_boxes, hit: mark_hit }),
                step: Some(mark_step),
                hit_enemy: Some(mark_hit_enemy),
                hit_ally: Some(mark_hit_ally),
                end: Some(mark_end),
                ..ProjectileHooks::default()
            }),
        ),
    )?;
    r.actors.enter(
        AIM_PROJECTILE,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::VelmaAimProjectile),
            ActorHooks::Projectile(ProjectileHooks {
                construct: Some(aim_construct),
                begin: Some(aim_begin),
                step: Some(aim_step),
                overlap: Some(aim_overlap),
                reflect: ReflectHooks { reflected: Some(aim_reflected), ..ReflectHooks::default() },
                ..ProjectileHooks::default()
            }),
        ),
    )?;
    r.actors.enter(
        SASS_FOLLOW_UP,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::VelmamSassFollowUpProjectile),
            ActorHooks::Projectile(ProjectileHooks {
                construct: Some(follow_construct),
                begin: Some(follow_begin),
                step: Some(follow_step),
                hit_enemy: Some(follow_hit_enemy),
                hit_ally: Some(follow_hit_ally),
                end: Some(follow_end),
                ..ProjectileHooks::default()
            }),
        ),
    )
}
