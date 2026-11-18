//! Arya's thrown dagger and its three conditions: MarkTeleportProjectile_C ([`Knife`], a
//! projectile's `ext`, a ParentProjectilePhysics child) and MarkTeleportToEnemyNotOutCondition,
//! MarkTeleportToEnemyAttachedCondition, MarkTeleportNotAttachedCondition.
//!
//! MarkTeleportProjectile_C (MarkTeleportProjectile, whose ubergraph the offsets below are of; a
//! ParentProjectilePhysics child: ParentProjectilePhysics, SpawnedActorParent). The resume offsets of its Delays:
//! 7480 -> 1003, 1116 -> 1171, 1216 -> 1271, 1384 -> 1439, 1453 -> 937, 948 -> 15, 29 -> 117, 9539 -> 5438,
//! 5383 -> 5127, 7250 -> 2631.
//!
//! Thrown by Arya_Attack_Special_Down_Montage (InitialSpeed / MaxSpeed 2900) and its air twin (3200) from socket
//! HitCollision01; the spawn triggers ability slot 2 (TriggerAbilityOnSpawnIndex, the world's actorSpawnAbility).
//! Class data: Capsule radius = half height 80 (a sphere; profile ProjectileNavNoPlatform, Platform ignored),
//! HitBox sphere 64, HurtBox 100, ProjectileGravityScale 0.5, no bShouldBounce, no MaxLifetime, HP 1,
//! Die With Instigator, DestroyOnExitedArena?, HitBoxDamage (Damage 1, no knockback, HitReaction 6, ArmorPierce,
//! DestroyProjectiles, HitEachActorOnlyOnce), DestroyOnHitProjectile, CanProjectileClank, Dir (0, 0, 100).
//!
//! ReceiveBeginPlay @10933 (the parent's, then @7535): 'Facing Left At Throw' = the instigator's FacingLeft @7693,
//! ActivationConditions[1] = true @7745, InitialVisuals (the loop sound), an EndPoint TargetPoint (nothing reads it
//! but the cleanup), then the chain of Delays: 0.375 -> Velocity x 0.75 @1003, 0.01 -> ProjectileGravityScale 5.0
//! @1171, 0.075 -> Velocity x 0.75 @1271, 3.0 -> @1439: with AttachedToEnemy Delay(1.0) -> TetherBreakWarning @937
//! and Delay(1.0) -> @15, else @15 at once. @15: while isTeleporting Delay(TeleportTime) first, then
//! K2_DestroyActor @88. A second branch @9539: Delay(0.0) -> AbilityTriggeredOnParent = true @5438.
//! HitEnemyPlayer @10519 (a BP_FighterCharacter; no parent call: the class has no DestroyOnHitFighter either):
//! "Spawn Attach Point"(true) @8294 (ProjectileMovement.StopSimulating, Attached = true, the debuff emitter),
//! K2_AttachToActor(the target, None, SnapToTarget location), DestroyOnExitedArena? = false, the target's
//! OnExitedArena bound to K2_DestroyActor @13155, ReturnCooldownServer(0.25) @10737, the impact sound, the loop's
//! stop, then @1508: AttachedToEnemy = true, DestroyOnConsume = true, DestroyOnBlocked = false,
//! SetActorEnableCollision(false), ActivationConditions[1] = true, AttachedEnemy = the attach parent, and with
//! Buff_Arya_Wounded on the owner and another team's target: SpawnBuffActor(Buff_FearStack, the target, the owner,
//! 1, 5.0).
//! HitAlly @11101 (a team mate, never the owner: NotifyAllyOfHit): the same attach when not Attached; with
//! Buff_Arya_EnrageAllies on the instigator SpawnBuffActor(Buff_ScoobySnack, the ally, the instigator,
//! EnragePerkAllyStacks, 14.0) and ReturnCooldownServer(0.25), else ReturnCooldownServer(0.5) @11725; then the same
//! sounds and @1508 (AttachedToEnemy is true for an ally too).
//! The Capsule's hit @10938 (a blocking move): nothing when Attached, else "Spawn Attach Point"(false)
//! (StopSimulating; Attached stays false), K2_AttachToComponent(the hit component, KeepWorld), the impact sound,
//! the loop's stop. No cooldown comes back.
//! ReturnCooldownServer(pct) @12701: a DoOnce nothing opens again; @5127 waits (Delay(0.0) -> @5127) until
//! AbilityTriggeredOnParent, then the instigator's UpdateAbilityCooldownServer(TriggerAbilityIndex, false, 0.0, pct).
//! ActivateObject @9594 -> @5450 (AnimNotify_ActivateSpawned of Arya_MoveToEnemy_montage and
//! Arya_MoveKnifeNotAttached_montage, all knives): TeleportVisuals; with a valid AttachedEnemy the cast fighter and
//! its BPI_IsFacingLeft are kept in the graph's frame @5537-5695; EndRotation; TeleportTravelLeft = the unit vector
//! owner -> knife has Y < 0 @6062; ProjectileMovement.Velocity = 0 @6081; isTeleporting = true; TeleportTime =
//! FClamp(GetDistanceTo x 0.0001, 0.275, 0.5) @6328; SpawnBuffActor(Buff_MoveToPosition, owner, owner, 0, that
//! time) and on it MulticastBeginMove((0, the knife's Y, Z), false, false, the time) @7034; Delay(the time) -> @2631.
//! @2631: the cast fighter's LaunchCharacter(Dir, true, true); with the owner not in hitstun @4674 its
//! K2_TeleportTo((0, knife Y + (kept facing left ? -35 : 35), knife Z), EndRotation); with AttachedToEnemy @2808 the
//! instigator's BPI_SetFacingLogic(the kept facing) and, for a fighter of the knife's own team with
//! Buff_Arya_EnrageAllies on the instigator, SpawnBuffActor(Buff_ScoobySnack, instigator, instigator,
//! EnragePerkSelfStacks, 14.0); without it @4084 BPI_SetFacingLogic(!TeleportTravelLeft). Then @3909 the
//! instigator's LaunchCharacter(Dir, true, true) and K2_DestroyActor.
//! ReceiveAnyDamage @9609 (no parent call): a DamageCauser that is a BaseFighter of another team takes Damage off
//! HP; at 0 K2_DestroyActor. Dead in the build: the class's Comp_Actor_Destructible has DamageDisabled, which
//! nothing writes for it, so Comp_Pawn_DamageManager DamageAndLaunch's CanBeHitBy @659 fails and ApplyDamage @2591
//! is never reached (@718 -> @8047, the HitByAlly broadcast). Not ported ([`damaged`]).
//! BPI_CanMarkFighter (BPI_Marker): !Attached. Read by its handler's pfgDoDamage @7637 on an invulnerable enemy: a
//! dodge through a flying knife is credited although its hit has no knockback.
//! ReceiveEndPlay @12670 -> @7850: while isTeleporting, with Buff_Arya_Wounded on the owner, AttachedEnemy's
//! Buff_FearStack actor is destroyed; the loop's stop.
//! The two perk buffs (Buff_Arya_Wounded, Buff_Arya_EnrageAllies) are given by perks, read here by class.
//! Not ported, by the graph: OnAttachedParentKnockedBack (Buff_Weakened at WoundedHitsForStack) and SwapPosition
//! have no caller; DisableProjectile is called by C023B classes alone.
//!
//! Model. The knife is a Projectile of the world's list, so it is in the owner's spawned actors with x, y, vx, vy
//! (AnimNotify_FaceTowards target 5, Arya_CanTeleport_BTD's reads). Its place while on a fighter is that fighter's
//! actor location (no socket: the root). `Knife`'s method overrides are called from the shared
//! projectile code: `step` while on a fighter ([`ride`]) and `receiveDamage` ([`damaged`]); its getters `isAttached`
//! / `attachedTo` ([`attached_to`]) and `destroyOnConsume` ([`destroy_on_consume`]) through `actors::world_actor`.
// Unverified: the frame each Delay ends in follows the convention of the other kinds (a BeginPlay Delay's link runs
// in the tick where age = latentFrames(seconds); one started by the owner's notify counts the frame it is started
// in); no game read.
// Unverified: a blocking move of a ProjectileMovement without bShouldBounce stops it where the sweep ends (stock 4.26
// HandleImpact -> StopSimulating; the graph's own handler stops it as well), and a move that starts inside the stage
// block is such a hit (as the projectile step's DestroyOnCollideWithEnvironment rule); the sweep is sampled every
// 10 u and bisected, against the stage block alone (the Capsule ignores Platform).
// K2_TeleportTo @4912 is AActor::TeleportTo (`fighter_teleport_to`: FindTeleportSpot, no move when no spot is clear);
// its return is not read.
// Deviation: Buff_MoveToPosition is the force move of Comp_Pawn_TeleportToAlly's port (not in the buff list, its
// Delay and RootMotionID in the ally state); a spawn refused by a state group would leave isTeleporting set without
// a move.
// Deviation: GetDistanceTo and the unit vector are taken in the play plane (the knife's depth offset from its socket
// is not kept; the time clamp's floor 0.275 holds below 2750 u).

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_def, class_p, set_gravity_scale, set_hitbox, yaw_x};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{buff_of, end_buff, spawn_buff};
use crate::combat::{hits, phases};
use crate::events::{Event, KnifeKind, SoundKey};
use crate::fighter::common::latent_frames;
use crate::fighter::control::{data_name, update_ability_cooldown};
use crate::fighter::match_side::end_move_buff;
use crate::fighter::moves::{Impulse, force_move_to_point};
use crate::fighter::{FighterId, actor_y, facing, has_equipped, in_hitstun, invulnerable};
use crate::math::ue::size;
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, Condition, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::spawn_spot::fighter_teleport_to;
use indexmap::IndexSet;

const KNIFE: Name = name!("MarkTeleportProjectile_C");
const WOUNDED: Name = name!("Buff_Arya_Wounded_C");
const ENRAGE: Name = name!("Buff_Arya_EnrageAllies_C");
const FEAR_STACK: Name = name!("Buff_FearStack_C");
const SNACK: Name = name!("Buff_ScoobySnack_C");
const IMPACT: Name = name!("sfx_cha_arya_knife_impact");
/// The knife's loop sound key (`'aryaknife' + id`).
const LOOP_TAG: Name = name!("aryaknife");

// The BeginPlay chain's ages (the tick each link runs in).
fn slow1() -> Real {
    latent_frames(0.375)
}
fn gravity() -> Real {
    slow1() + latent_frames(0.01)
}
fn slow2() -> Real {
    gravity() + latent_frames(0.075)
}
fn life() -> Real {
    slow2() + latent_frames(3.0)
}

/// `Knife extends Projectile`: what the class adds.
#[derive(State, Debug)]
pub struct Knife {
    /// ActivationConditions (class default [true, false]; BeginPlay @7745 and @1508 write entry 1 true).
    pub conds: Vec<bool>,
    /// Attached: "Spawn Attach Point"(true), on a fighter.
    pub stuck: bool,
    /// AttachedToEnemy @1508 (an enemy or an ally).
    pub to_enemy: bool,
    /// AttachedEnemy, the attach parent.
    pub target: Option<FighterId>,
    /// Attached to the world: the Capsule's hit @10938 (Attached stays false).
    pub ground: bool,
    /// 'Facing Left At Throw'.
    pub facing_left_at_throw: bool,
    /// What "Spawn Attach Point Visuals" @8754-8882 reads at its first run: the root component's rotation, under
    /// which the emitter's offset (100, +-15, -20) is taken (the drawing reads these, never the knife's state at a
    /// later tick). The pitch, and the yaw in quarter turns (-1: -90, 1: 90, 0: a velocity straight up or down), as
    /// the port has the actor's rotation with the velocity of that call. The root does not turn in the stage, so a
    /// second run (visuals) reads the same.
    /// In degrees, as the projectile's pitch.
    pub root_pitch: Real,
    pub root_yaw: i32,
    /// The runs of that function: each spawns a PS_AryaSPF_Knife_Debuff @8882 (auto destroy: it lives on with the
    /// actor). 2 for a knife that lay in the stage and was then taken by a fighter: HitAlly @11176 -> @11749 and
    /// HitEnemyPlayer @10594, which asks nothing first. Never more: on a fighter its collision is off.
    pub visuals: i32,
    /// The facing of the fighter it is on at its K2_AttachToActor @10609 / @11190 (KeepWorld rotation: the knife
    /// turns with that fighter's actor from there); 0: on none yet.
    pub face_at: Real,
    /// The loop sound is playing.
    pub loop_on: bool,
    /// AbilityTriggeredOnParent.
    pub triggered: bool,
    /// ReturnCooldownServer's DoOnce.
    pub refund_closed: bool,
    /// Its percent while @5127 waits; -1: none pending.
    pub refund_pct: Real,
    /// TetherBreakWarning @937.
    pub warning: bool,
    /// The ages of the chain's tail from @1439: the warning, @15, and @15's own Delay(TeleportTime).
    pub warn_at: Real,
    pub end_at: Real,
    pub die_at: Real,
    /// isTeleporting.
    pub teleporting: bool,
    /// TeleportTime.
    pub teleport_time: Real,
    /// TeleportTravelLeft.
    pub travel_left: bool,
    /// The frame's cast of AttachedEnemy at the last ActivateObject @5537.
    pub cast: Option<FighterId>,
    /// Its BPI_IsFacingLeft at that call @5695.
    pub cast_left: bool,
    /// The age the dash's Delay @7250 ends at.
    pub done_at: Real,
    /// The team mates its hit box is over, by fighter id (the overlap's begin is the one pfgDoDamage call: the
    /// kind's `overlap`). Its own set: the world's allyOver is kept only while a buff listens.
    pub ally_in: IndexSet<i32>,
    pub capsule_r: Real,
}

impl Default for Knife {
    /// The field initial values.
    fn default() -> Self {
        Knife {
            conds: Vec::new(),
            stuck: false,
            to_enemy: false,
            target: None,
            ground: false,
            facing_left_at_throw: false,
            root_pitch: 0.0,
            root_yaw: 0,
            visuals: 0,
            face_at: 0.0,
            loop_on: false,
            triggered: false,
            refund_closed: false,
            refund_pct: -1.0,
            warning: false,
            warn_at: -1.0,
            end_at: -1.0,
            die_at: -1.0,
            teleporting: false,
            teleport_time: 0.0,
            travel_left: false,
            cast: None,
            cast_left: false,
            done_at: -1.0,
            ally_in: IndexSet::new(),
            capsule_r: 0.0,
        }
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn knife(w: &World, id: ProjectileId) -> &Knife {
    match &pr(w, id).ext {
        ProjectileExt::Knife(k) => k,
        e => panic!("MarkTeleportProjectile_C without its state: {e:?}"),
    }
}

fn knife_mut(w: &mut World, id: ProjectileId) -> &mut Knife {
    match &mut pm(w, id).ext {
        ProjectileExt::Knife(k) => k,
        e => panic!("MarkTeleportProjectile_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a knife has an owner")
}

/// `Knife.attachedTo` (GetAttachParentActor): the fighter it is on, `None` for none. Called by
/// `world_actor::attached_to` for a knife.
pub fn attached_to(p: &Projectile) -> Option<FighterId> {
    match &p.ext {
        ProjectileExt::Knife(k) => k.target,
        _ => None,
    }
}

/// `Knife.destroyOnConsume`: true from the attach on (@1508; its data's false before), read by
/// Buff_HeldByEnemy's DestroyAttachedConsumableActors, which looks only at an attached actor. Called by
/// `world_actor::destroy_on_consume` for a knife.
pub fn destroy_on_consume(p: &Projectile) -> bool {
    attached_to(p).is_some()
}

/// `Knife.step`'s override, ahead of the projectile's own step: on a fighter the movement is stopped and the actor
/// rides its parent: nothing of the parent's tick is left but the age (the class has no MaxLifetime, and
/// DestroyOnExitedArena? is false from the attach on). True: the step is done. Called by `projectile::step`.
pub fn ride(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let ProjectileExt::Knife(k) = &p.ext else { return false };
    let Some(t) = k.target else { return false };
    if p.dead {
        return false;
    }
    let (tx, ty) = (w.fighter(t).body.x, actor_y(w, t));
    let p = pm(w, id);
    p.prev_x = p.x;
    p.prev_y = p.y;
    p.age += 1;
    p.x = tx;
    p.y = ty;
    true
}

/// `Knife.receiveDamage`: ReceiveAnyDamage @9609 has no parent call and is never reached (DamageDisabled, the
/// header): no damage from a clank either, where the causer is a spawned actor (the graph wants a BaseFighter).
/// Called by `projectile::receive_damage` for a knife; never destroyed.
pub fn damaged() -> bool {
    false
}

fn loop_key(id: ProjectileId, w: &World) -> SoundKey {
    SoundKey { tag: LOOP_TAG, n: pr(w, id).seq }
}

fn loop_stop(w: &mut World, id: ProjectileId) {
    if !knife(w, id).loop_on {
        return;
    }
    knife_mut(w, id).loop_on = false;
    // Audio_Loop_Stop @12177: PF_Post_Event(sfx_cha_arya_knife_loop_stop, self, self). The event is one Stop action
    // of the loop's sound, scope game object, no fade: on the knife's own object, which the port has as the key.
    let e = Event::SoundStop {
        fighter: pr(w, id).owner_id,
        key: loop_key(id, w),
        fade: None,
        ev: Some(name!("sfx_cha_arya_knife_loop_stop")),
        curve: None,
        cut: false,
    };
    w.events.push(e);
}

fn impact(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let e = Event::Sound {
        fighter: p.owner_id,
        ev: IMPACT,
        x: Some(p.x),
        y: Some(p.y),
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
    loop_stop(w, id);
}

/// ReturnCooldownServer @12701 -> @5127.
fn refund(w: &mut World, id: ProjectileId, pct: Real) {
    if knife(w, id).refund_closed {
        return;
    }
    knife_mut(w, id).refund_closed = true;
    if knife(w, id).triggered {
        let (f, slot) = (owner(w, id), trigger_slot(w, id));
        update_ability_cooldown(w, f, slot, false, 0.0, pct);
    } else {
        knife_mut(w, id).refund_pct = pct;
    }
}

/// `pr.p.TriggerAbilityIndex ?? 0`.
fn trigger_slot(w: &World, id: ProjectileId) -> i32 {
    to_int32(num(class_p(w, pr(w, id)), "TriggerAbilityIndex").unwrap_or(0.0))
}

/// "Spawn Attach Point Visuals" @8532 (from "Spawn Attach Point" @8508, Attached or not).
/// In the graph the root is read with the movement already stopped: "Spawn Attach Point" calls StopSimulating @8438
/// ahead of the call @8508, and the function stops it again @8709 before it reads the root @8754. The port's order is
/// the other way round (the callers run this ahead of vx = vy = 0) for its own reason: the root keeps the rotation
/// it had, and the port has no stored yaw, it derives the yaw from the live vx / vy.
fn visuals(p: &mut Projectile) {
    let (pitch, vx, vy, yaw) = (p.pitch, p.vx, p.vy, yaw_x(p));
    let ProjectileExt::Knife(k) = &mut p.ext else { return };
    k.visuals += 1;
    if k.stuck || k.ground {
        return; // (the root is where the first run found it)
    }
    k.root_pitch = pitch;
    k.root_yaw = if vx == 0.0 && vy != 0.0 {
        0
    } else if yaw > 0.0 {
        -1
    } else {
        1
    };
}

/// "Spawn Attach Point"(true) and K2_AttachToActor, then @1508.
fn attach(w: &mut World, id: ProjectileId, o: FighterId) {
    visuals(pm(w, id));
    let (ox, oy, face, oid) = (w.fighter(o).body.x, actor_y(w, o), facing(w, o), w.fighter(o).id);
    let p = pm(w, id);
    p.stopped = true; // StopSimulating: the velocity goes with it
    p.vx = 0.0;
    p.vy = 0.0;
    p.x = ox;
    p.y = oy;
    let seq = p.seq;
    let k = knife_mut(w, id);
    k.stuck = true;
    k.ground = false;
    k.target = Some(o);
    k.face_at = face;
    let f = owner(w, id);
    let (fid, fx, fy) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    w.events.push(Event::AryaKnife { fighter: fid, knife: seq, kind: KnifeKind::Stick, victim: oid, x: fx, y: fy });
}

fn attached(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let o = knife(w, id).target.expect("attached to a fighter");
    knife_mut(w, id).to_enemy = true;
    // SetActorEnableCollision(false): no hit box, no hurt box, no clank.
    let p = pm(w, id);
    set_hitbox(p, false);
    p.hurt_radius = 0.0;
    let k = knife_mut(w, id);
    if k.conds.len() < 2 {
        k.conds.resize(2, false);
    }
    k.conds[1] = true;
    if buff_of(w, f, WOUNDED).is_some() && w.fighter(o).team.team != w.fighter(f).team.team {
        spawn_buff(w, o, FEAR_STACK, 5.0, Some(1), Some(f), true, None);
    }
}

/// LaunchCharacter(Dir, true, true): a direct launch (UE Y = -x).
fn launch(w: &mut World, f: FighterId, dir: Option<(Real, Real)>) {
    if w.fighter(f).life.dead > 0.0 {
        return;
    }
    let (y, z) = dir.unwrap_or((0.0, 0.0));
    let m = &mut w.fighter_mut(f).moves;
    m.impulse = Some(Impulse { vx: -y, vy: z, xy: true, z: true, kind: 0, d: None });
    m.hit_launch = true;
}

/// The class's `Dir` (Y, Z), `pr.p.Dir ?? {}`.
fn dir_of(w: &World, id: ProjectileId) -> (Real, Real) {
    let d = obj(class_p(w, pr(w, id)), "Dir");
    (d.and_then(|d| num(d, "Y")).unwrap_or(0.0), d.and_then(|d| num(d, "Z")).unwrap_or(0.0))
}

/// The Capsule's blocking hit against the stage block along this frame's move (prev -> the place the movement
/// component left it at, which does not collide for a class without bShouldBounce), and against the stage solids
/// that block PhysicsBody, its object type.
fn moved(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    if p.dead || p.stopped || knife(w, id).target.is_some() {
        return;
    }
    let gs = crate::world::stage_query::static_shapes(
        w,
        crate::consts::SOLID_NAV,
        name!("MarkTeleportProjectile_C Capsule"),
    );
    let p = pr(w, id);
    let r = knife(w, id).capsule_r * p.scale;
    let over = |x: Real, y: Real| -> bool {
        gs.boxes.iter().any(|g| {
            let qx = x - max(crate::math::narrow(g.left), min(x, crate::math::narrow(g.right)));
            let qy = y - max(crate::math::narrow(g.bottom), min(y, crate::math::narrow(g.top)));
            qx * qx + qy * qy <= r * r
        }) || gs.polys.iter().any(|q| {
            let (cx, cy) = q.closest(x, y);
            (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r
        })
    };
    let (x0, y0) = (p.prev_x, p.prev_y);
    let (dx, dy) = (p.x - x0, p.y - y0);
    let mut t = -1.0;
    if over(x0, y0) {
        t = 0.0;
    } else {
        let n = max(1.0, ((dx * dx + dy * dy).sqrt() / 10.0).ceil());
        let mut i = 1.0;
        while i <= n && t < 0.0 {
            if over(x0 + (dx * i) / n, y0 + (dy * i) / n) {
                let mut lo = (i - 1.0) / n;
                let mut hi = i / n;
                for _ in 0..12 {
                    let m = (lo + hi) / 2.0;
                    if over(x0 + dx * m, y0 + dy * m) {
                        hi = m;
                    } else {
                        lo = m;
                    }
                }
                t = lo;
            }
            i += 1.0;
        }
    }
    if t < 0.0 {
        return;
    }
    let p = pm(w, id);
    p.x = x0 + dx * t;
    p.y = y0 + dy * t;
    visuals(p);
    p.stopped = true;
    p.vx = 0.0;
    p.vy = 0.0;
    let seq = p.seq;
    knife_mut(w, id).ground = true;
    let f = owner(w, id);
    let (fid, fx, fy) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    w.events.push(Event::AryaKnife { fighter: fid, knife: seq, kind: KnifeKind::Ground, victim: -1, x: fx, y: fy });
    impact(w, id);
}

/// HitAlly's own part, behind the gates of the dealer's pfgDoDamage.
/// An already Attached knife skips the attach (@11176-11190): unreachable, the attach turns its collision off.
fn hit_ally(w: &mut World, id: ProjectileId, o: FighterId) {
    let f = owner(w, id);
    if !knife(w, id).stuck {
        attach(w, id, o);
    }
    if buff_of(w, f, ENRAGE).is_some() {
        let stacks = to_int32(num(class_p(w, pr(w, id)), "EnragePerkAllyStacks").unwrap_or(0.0));
        spawn_buff(w, o, SNACK, 14.0, Some(stacks), Some(f), true, None);
        refund(w, id, 0.25);
    } else {
        refund(w, id, 0.5);
    }
    impact(w, id);
    attached(w, id);
}

/// @2631, the end of the dash's Delay. True: K2_DestroyActor.
fn arrive(w: &mut World, id: ProjectileId) -> bool {
    let f = owner(w, id);
    let dir = dir_of(w, id);
    let (cast, cast_left, to_enemy, travel_left) = {
        let k = knife(w, id);
        (k.cast, k.cast_left, k.to_enemy, k.travel_left)
    };
    if let Some(c) = cast {
        launch(w, c, Some(dir));
    }
    if !in_hitstun(w, f) && w.fighter(f).life.dead <= 0.0 {
        let p = pr(w, id);
        let (x, y) = (p.x + if cast_left { 35.0 } else { -35.0 }, p.y);
        fighter_teleport_to(w, f, x, y); // @4912
    }
    if to_enemy {
        crate::fighter::set_facing(w, f, if cast_left { -1.0 } else { 1.0 });
        if let Some(c) = cast
            && w.fighter(c).team.team == w.fighter(f).team.team
            && buff_of(w, f, ENRAGE).is_some()
        {
            let stacks = to_int32(num(class_p(w, pr(w, id)), "EnragePerkSelfStacks").unwrap_or(0.0));
            spawn_buff(w, f, SNACK, 14.0, Some(stacks), Some(f), true, None);
        }
    } else {
        // facing left = !TeleportTravelLeft; UE Y < 0 is a greater sim x
        crate::fighter::set_facing(w, f, if travel_left { 1.0 } else { -1.0 });
    }
    launch(w, f, Some(dir));
    true
}

/// The constructor's tail: `conds` from the class's ActivationConditions, the Capsule's radius.
fn construct(w: &World, p: &mut Projectile) {
    let conds = class_p(w, p)
        .get("ActivationConditions")
        .and_then(mvs_data::value::Value::as_arr)
        .map_or_else(Vec::new, |a| a.iter().map(crate::registry::props::truthy).collect());
    let capsule_r = class_def(w, p)
        .and_then(|d| obj(&d.components, "Capsule"))
        .and_then(|c| num(c, "CapsuleRadius"))
        .unwrap_or(0.0);
    p.ext = ProjectileExt::Knife(Knife { conds, capsule_r, ..Knife::default() });
}

fn begin(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let left = facing(w, f) < 0.0;
    let ev = crate::registry::props::text(class_p(w, pr(w, id)), "Ak Projectile Loop").map(|s| data_name(w, s));
    let fid = pr(w, id).owner_id;
    let key = loop_key(id, w);
    let k = knife_mut(w, id);
    k.facing_left_at_throw = left;
    if k.conds.len() < 2 {
        k.conds.resize(2, false);
    }
    k.conds[1] = true;
    if let Some(ev) = ev {
        k.loop_on = true;
        w.events.push(Event::SoundStart { fighter: fid, ev, key });
    }
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    // The latent actions, in the order they were started.
    let age = crate::real(pr(w, id).age);
    if age == slow1() || age == slow2() {
        let p = pm(w, id);
        p.vx *= 0.75;
        p.vy *= 0.75;
    }
    if age == gravity() {
        set_gravity_scale(pm(w, id), 5.0);
    }
    knife_mut(w, id).triggered = true; // the Delay(0.0) of BeginPlay ends in the actor's first tick
    let pending = knife(w, id).refund_pct;
    if pending >= 0.0 {
        let (f, slot) = (owner(w, id), trigger_slot(w, id));
        update_ability_cooldown(w, f, slot, false, 0.0, pending);
        knife_mut(w, id).refund_pct = -1.0;
    }
    // The target's OnExitedArena @13155.
    if let Some(t) = knife(w, id).target
        && w.fighter(t).life.dead > 0.0
    {
        return true;
    }
    if age == knife(w, id).done_at && arrive(w, id) {
        return true;
    }
    if age == life() {
        let k = knife_mut(w, id);
        if k.to_enemy {
            k.warn_at = age + latent_frames(1.0);
        } else {
            k.end_at = age;
        }
    }
    if age == knife(w, id).warn_at {
        let k = knife_mut(w, id);
        k.warning = true;
        k.end_at = age + latent_frames(1.0);
    }
    if age == knife(w, id).end_at {
        let k = knife_mut(w, id);
        if !k.teleporting {
            return true;
        }
        k.die_at = age + latent_frames(k.teleport_time);
    }
    age == knife(w, id).die_at
}

/// A team mate's overlap of the hit box: NotifyAllyOfHit's calls (the ally's HitByAlly through `ally_hit`, then the
/// class's HitAlly). An enemy's hit is the world's own, with `hit_enemy` behind it.
/// pfgDoDamage runs once per begin of the overlap: an ally invulnerable at the begin (@7381: CanBeHitBy fails for
/// the team, pop) or already hit (@2883) gets nothing until a new begin.
fn overlap(w: &mut World, id: ProjectileId, vic: FighterId) {
    let f = owner(w, id);
    if vic == f || w.fighter(vic).team.team != w.fighter(f).team.team {
        return;
    }
    let vid = w.fighter(vic).id;
    let p = pr(w, id);
    let sp = [WorldSphere {
        x: p.x,
        y: p.y,
        z: if p.sphere_3d { Some(0.0) } else { None },
        r: p.hit_radius,
        ..WorldSphere::default()
    }];
    let mut on = phases::hitbox_active(w, id) && !p.dead && w.fighter(vic).life.dead <= 0.0 && !phases::no_hit(w, vic);
    if on {
        on = match phases::victim_pose(w, vic) {
            Some(vp) => {
                let met = crate::combat::hit::hurt::hurt_hit(w, vic, &sp, &vp).is_some();
                phases::give_vbuf(w, vp);
                met
            }
            None => false,
        };
    }
    if !on {
        knife_mut(w, id).ally_in.shift_remove(&vid);
        return;
    }
    if !knife_mut(w, id).ally_in.insert(vid) {
        return;
    }
    let d = hits::dealer(w, ActorRef::Projectile(id));
    if pr(w, id).hit_set.contains(&vid) || hits::projectile_blocks(w, &d, vic) || invulnerable(w, vic) {
        return;
    }
    if pr(w, id).hit.hit_each_actor_only_once.unwrap_or(false) {
        pm(w, id).hit_set.insert(vid);
    }
    let hp = pr(w, id).hit.clone();
    hits::ally_hit(w, &d, vic, &hp);
    hit_ally(w, id, vic);
}

/// BPI_CanMarkFighter: !Attached.
fn can_mark(w: &World, id: ProjectileId) -> bool {
    !knife(w, id).stuck
}

fn hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    if pr(w, id).dead {
        return;
    }
    attach(w, id, vic);
    refund(w, id, 0.25);
    impact(w, id);
    attached(w, id);
}

fn activate(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let t = knife(w, id).target;
    let (fid, fx, fy) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    let seq = pr(w, id).seq;
    let victim = t.map_or(-1, |t| w.fighter(t).id);
    w.events.push(Event::AryaKnife { fighter: fid, knife: seq, kind: KnifeKind::Dash, victim, x: fx, y: fy });
    loop_stop(w, id); // TeleportVisuals
    if let Some(t) = t {
        let left = facing(w, t) < 0.0;
        let k = knife_mut(w, id);
        k.cast = Some(t);
        k.cast_left = left;
    }
    let p = pr(w, id);
    let (dx, dy) = (p.x - fx, p.y - fy);
    let dist = size([0.0, dx, dy]);
    let travel_left = dist * dist > 1e-8 && dx > 0.0;
    let (px, py) = (p.x, p.y);
    let time = min(max(dist * 0.0001, 0.275), 0.5);
    let age = crate::real(p.age);
    let p = pm(w, id);
    p.vx = 0.0;
    p.vy = 0.0;
    let k = knife_mut(w, id);
    k.travel_left = travel_left;
    k.teleporting = true;
    k.teleport_time = time;
    end_move_buff(w, f); // CanOverride: an older Buff_MoveToPosition goes first
    let mid = force_move_to_point(w, f, px, py, time, 2.0, 0.0, 0.0, 0.0, false);
    w.fighter_mut(f).team.ally.move_id = to_int32(mid);
    // Both Delays (the buff's Delay(OverTime) and the knife's) are started here, behind the owner's tick of this
    // frame, which they count: the buff's own count is one down already, the knife's age steps later in the frame.
    w.fighter_mut(f).team.ally.buff = max(latent_frames(time) - 1.0, 1.0);
    knife_mut(w, id).done_at = age + latent_frames(time);
}

fn end(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let (teleporting, t) = {
        let k = knife(w, id);
        (k.teleporting, k.target)
    };
    // (not in the Lab reset's pass: an old pawn there)
    if !reset_pass(w)
        && teleporting
        && let Some(t) = t
        && buff_of(w, f, WOUNDED).is_some()
        && let Some(b) = buff_of(w, t, FEAR_STACK)
    {
        end_buff(w, t, b);
    }
    loop_stop(w, id);
}

fn make_kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        moved: Some(moved),
        step: Some(step),
        overlap: Some(overlap),
        can_mark: Some(can_mark),
        hit_enemy: Some(hit_enemy),
        activate: Some(activate),
        end: Some(end),
        ..ProjectileHooks::default()
    }
}

/// The owner's first live knife (Comp_Spawner.SpawnedActors filtered by the class, entry 0; a destroyed actor leaves
/// the list at its EndPlay).
fn first(w: &World, f: FighterId) -> Option<ProjectileId> {
    w.fighter(f).items.spawned_actors.iter().find_map(|a| match *a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).filter(|p| p.cls == KNIFE && !p.dead).map(|_| id),
        _ => None,
    })
}

/// `listedKnife`: Arya_CanTeleport_BTD's loop over Comp_Spawner.SpawnedActors (@156-810): the first entry whose
/// class is exactly MarkTeleportProjectile_C (@571-600 GetObjectClass ==, no cast), in list order; the graph asks
/// IsValid after the loop (@811).
/// A destroyed knife is not in the build's list for any later reader of the tick: it leaves inside its own
/// K2_DestroyActor, by SpawnedActorParent's ReceiveEndPlay @18227-18298 (RemoveActorListMulticast(self):
/// Comp_Spawner @5582 Array_RemoveItem) and again by its OnEndPlay binding -> ActorEndPlay -> @6635. The owner's
/// list drops it at the tick's end, so a dead entry is passed over here.
// Unverified: a pooled release (ReleaseToPool @6423, the cap's path with CanBePooled) is not a destroy and what takes
// such an actor off the list was not read; the knife's CanBePooled is not in its class data (the parent's default
// was not read).
pub fn listed_knife(w: &World, f: FighterId) -> Option<ProjectileId> {
    first(w, f)
}

/// ActivationConditions[1] && !IsEquippedComponents.contains('TeleportMarker'). Her montages' Equip notifies of that
/// tag have EquipAtStart false and nothing at their end, so the tag is never in the list: the read is kept as the
/// graph has it.
fn ready(w: &World, f: FighterId, id: ProjectileId) -> bool {
    knife(w, id).conds.get(1).copied().unwrap_or(false) && !has_equipped(w, f, name!("teleportmarker"))
}

/// MarkTeleportToEnemyNotOutCondition: true with no spawner or no knife @459, else !IsValid(the first) @377-435.
fn not_out(w: &mut World, f: FighterId) -> bool {
    first(w, f).is_none()
}

/// MarkTeleportToEnemyAttachedCondition @899-1052: the two reads above and the knife's Attached; false with no knife
/// @1076.
fn attached_cond(w: &mut World, f: FighterId) -> bool {
    first(w, f).is_some_and(|k| ready(w, f, k) && knife(w, k).stuck)
}

/// MarkTeleportNotAttachedCondition @765-858: the two reads alone (Attached is not read: it holds for a knife on a
/// fighter too, and the start tables ask the Attached condition first); false with no knife @882.
fn not_attached_cond(w: &mut World, f: FighterId) -> bool {
    first(w, f).is_some_and(|k| ready(w, f, k))
}

/// This file's registry entries (the knife class, the three conditions). The pawn has no hook for the dagger: its
/// end with her defeat is the world's 'Die With Instigator'.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        "MarkTeleportProjectile_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::MarkTeleportProjectile),
            ActorHooks::Projectile(make_kind()),
        ),
    )?;
    r.conditions.enter("MarkTeleportToEnemyNotOutCondition_C", Condition::Fn(not_out))?;
    r.conditions.enter("MarkTeleportToEnemyAttachedCondition_C", Condition::Fn(attached_cond))?;
    r.conditions.enter("MarkTeleportNotAttachedCondition_C", Condition::Fn(not_attached_cond))
}
