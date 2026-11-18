//! Marvin's homing: Buff_C018HomingVictim_C (the flag's mark on a fighter, its
//! ext [`HomingVictimExt`]) and Buff_C018HomingProjectile_C (an actor buff that turns a spawned actor into a homing one,
//! its ext [`HomingProjExt`]); perks.rs's Buff_C018PerkHomingProjectile_C enters the same actor hooks
//! ([`projectile_actor`]). No pawn hook and no pawn state.
//!
//! Buff_C018HomingVictim (BuffParentActor child; BuffDuration 0, CanRefresh, AlwaysRefresh, IsDebuff,
//! DestroyBuffWhenOwnerDefeated; its CheckForProjectiles sphere: radius 800, object type HitBox, overlap to
//! ProjectileDestructible and ItemDestructible): ReceiveBeginPlay @5094 -> @963: the Instigator's Comp_Actor_Destructible
//! valid -> Team Index @1390, MulticastUI(BuffDuration > 0 ? BuffDuration : -1, false) @1524. RefreshBuff @5745: the
//! parent's, then @823 MulticastUI(.., true) @938. CheckForProjectiles' begin overlap @5109 -> @15: a SpawnedActorParent
//! of the Team Index whose Instigator is a Marvin that finds it viable -> ApplyHomingBuff @371; another team or not
//! viable -> an item joins QueuedItems @663; TeamIndex -1 -> a "for at most 5" Delay(0) retry @395-533. Its end overlap
//! @5774 -> @2446: out of QueuedItems. ReceiveTick @5779: QueuedItems backwards @4134-5038. OwnerKnockedBack @2127: a
//! SpawnedActorParent attacker of attack type 0 or 1 destroys it. ReceiveEndPlay @5055 -> @2689: the overlapped
//! actors' homing buffs on this owner end; RemoveStateForPlayer @4001. ApplyHomingBuff @0-919: the hit set test,
//! SpawnBuffActor(Buff_C018HomingProjectile_C) @626 -> ServerHomingInfo(the victim) @874.
//!
//! Buff_C018HomingProjectile (MagnitudeVelocityMultiplier 3, InitialVelocityMultiplier 0.4, MinSpeedOnLockOnStart 1500,
//! CanOverride, BlockerBuffs [Buff_C018PerkHomingProjectile_C]): ReceiveBeginPlay @888, ServerHomingInfo @4840 -> @15,
//! MulticastHomingInfo @4010, ReceiveTick @2659 -> @2346, OnHitAnything @4845, ReceiveEndPlay @3020.
//
// Deviation: CheckForProjectiles' overlaps are tested once a frame in the buff's world timer (`BuffHooks::timer`, the
// frame's timer pass): a sphere query at the owner's root for the bodies of ProjectileDestructible (a projectile's
// HurtBox) and ItemDestructible (an item's Sphere) with the sphere's HitBox type as responder. The bodies of the types
// the sphere's default Block answers (a projectile's HitBox: the port's queries gather no HitBox body) are not met, nor
// a World.actors kind. Its end and begin edges, the Delay(0) retry and the queue run in that pass in that order; the
// build dispatches the overlaps inside the moves that make them.
// Unverified: a Delay(0) made at a begin overlap fires in the same frame's pass, one made in its own link a frame
// later.
// Unverified: ApplyHomingBuff's CheckIfActorHasBeenHit(victim, AttackTag) is read as the actor's hit set (the
// fighters its hit box hit while it was on), whatever the tag.
// Deviation: the UI timer of a refresh is the duration the refreshing spawn passed (`spawn_homing_victim`); a refresh
// that came another way takes the buff's frames left as seconds.
// Unverified: the originals the HomingProjectile writes back to an Item: its own bIsHomingProjectile is taken as off
// when no buff homing was there (`Item::write_homing(false)`); a Projectile's own homing is never touched (`set_homing`
// does not write it), so deleting buff_home gives it back.
// The pop text is the player's text, read by the HUD.

use super::pawn::is_viable_for_projectile_tweaks;
use super::state::marvin_of;
use crate::Real;
use crate::State;
use crate::actors::world_actor::{actor_cls, actor_dead, actor_elem};
use crate::actors::{ActorRef, projectile};
use crate::combat::buffs::{BuffExt, buff, buff_mut, buff_of, spawn_buff};
use crate::combat::elem::{
    ElemExt, actor_inst_def, clear_homing, elem_buff, elem_buff_mut, end_actor_buff, set_homing, spawn_actor_buff,
};
use crate::combat::{BuffClass, BuffRef};
use crate::events::{UiStateOp, UiStateParams};
use crate::fighter::common::{actor_def, owner_team};
use crate::fighter::traces::{OBJ_ITEM_DESTRUCTIBLE, OBJ_PROJECTILE_DESTRUCTIBLE, sphere_overlap_actors};
use crate::fighter::{FighterId, actor_y};
use crate::math::max;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks};
use crate::registry::props::num;
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal, BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// Buff_C018HomingVictim's class.
pub const HOMING_VICTIM: &str = "Buff_C018HomingVictim_C";

/// Buff_C018HomingProjectile's class.
pub const HOMING_PROJECTILE: &str = "Buff_C018HomingProjectile_C";

/// CheckForProjectiles.SphereRadius (Buff_C018HomingVictim's template).
const CHECK_RADIUS: Real = 800.0;

/// The HitBox object type (`OBJ.HitBox`), the sphere's own type: the responder of the query.
const OBJ_HIT_BOX: Real = 7.0;

/// Buff_C018HomingVictim_C's extension (`HomingVictimExt`, `BuffExt::C018HomingVictim`).
#[derive(State, Debug, Default)]
pub struct HomingVictimExt {
    /// The Marvin whose flag marked the victim (the buff's Instigator).
    pub marker: Option<FighterId>,
    /// Team Index (the Instigator's TeamIndex at the BeginPlay).
    pub team: i32,
    /// The SpawnedActorParents CheckForProjectiles overlapped at the last test.
    pub over: Vec<ActorRef>,
    /// QueuedItems.
    pub queued: Vec<ActorRef>,
    /// The "for at most 5" counter of the TeamIndex -1 retry.
    pub retries: i32,
    /// The begin overlap's cast temp (K2Node_DynamicCast_AsSpawned_Actor_Parent).
    pub last_cast: Option<ActorRef>,
    /// The id (`seq`) of `last_cast`'s actor, kept for the record: the arena's reference no longer names it once it is
    /// dead and
    /// out of its list (Rust-only, never read by the sim).
    pub last_cast_seq: Option<i32>,
    /// The world tick the pending Delay(0) @533 fires at, -1 none.
    pub retry_at: i32,
}

/// Buff_C018HomingProjectile_C's (and Buff_C018PerkHomingProjectile_C's) extension on the actor (`HomingProjectileExt`,
/// `ElemExt::C018HomingProjectile`). `ElemExt` is Copy, so `oHome` (BuffHome) is held as its target and
/// magnitude.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct HomingProjExt {
    /// Projectile Movement Component valid (the actor's class has a ProjectileMovement).
    pub pm: bool,
    /// Original Is Homing Projectile / Homing Target Component / Homing Acceleration Magnitude: the actor's BuffHome as
    /// (to, mag) when it had one.
    pub o_home: Option<(ActorRef, Real)>,
    /// Original Projectile Gravity Scale.
    pub o_grav: Real,
    /// ValuesAssigned.
    pub assigned: bool,
    /// Cancelled.
    pub cancelled: bool,
    /// The Tick's DoOnce closed.
    pub closed: bool,
    /// New Homing Component's owner (None until MulticastHomingInfo with a movement).
    pub target: Option<FighterId>,
    /// ServerHomingInfo's Delay(0) @833 pending.
    pub waiting: bool,
    /// Its Fighter argument (the event's, as the last call left it).
    pub pending: Option<FighterId>,
}

// ---- the actor: place, velocity, team, gravity (a Projectile or an Item, `Spawned`)

fn is_spawned(a: ActorRef) -> bool {
    matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_))
}

/// x, y, vx, vy.
fn state_of(w: &World, a: ActorRef) -> Option<(Real, Real, Real, Real)> {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|p| (p.x, p.y, p.vx, p.vy)),
        ActorRef::Item(id) => w.actors.items.get(id).map(|p| (p.x, p.y, p.vx, p.vy)),
        _ => None,
    }
}

fn set_v(w: &mut World, a: ActorRef, vx: Real, vy: Real) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                (p.vx, p.vy) = (vx, vy);
            }
        }
        ActorRef::Item(id) => {
            if let Some(p) = w.actors.items.get_mut(id) {
                (p.vx, p.vy) = (vx, vy);
            }
        }
        _ => {}
    }
}

fn gravity_of(w: &World, a: ActorRef) -> Real {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(0.0, projectile::gravity_scale),
        ActorRef::Item(id) => w.actors.items.get(id).map_or(0.0, |i| i.gravity_scale()),
        _ => 0.0,
    }
}

fn set_gravity(w: &mut World, a: ActorRef, s: Real) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                projectile::set_gravity_scale(p, s);
            }
        }
        ActorRef::Item(id) => {
            if let Some(p) = w.actors.items.get_mut(id) {
                p.set_gravity_scale(s);
            }
        }
        _ => {}
    }
}

fn buff_home_of(w: &World, a: ActorRef) -> Option<(ActorRef, Real)> {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id)?.buff_home.as_ref().map(|h| (h.to, h.mag)),
        ActorRef::Item(id) => w.actors.items.get(id)?.buff_home.as_ref().map(|h| (h.to, h.mag)),
        _ => None,
    }
}

/// `teamOf`: an item's destructible team, a projectile's owner's team (a projectile the world spawned has none: -1).
fn team_of(w: &World, a: ActorRef) -> i32 {
    match a {
        ActorRef::Item(id) => crate::actors::item::item_destructible_team(w, id),
        _ => owner_team(w, a),
    }
}

/// The Instigator implements BPI_C018 and BPI_IsViableForProjectileTweaks(a).
fn viable(w: &World, inst: Option<FighterId>, a: ActorRef) -> bool {
    inst.is_some_and(|i| marvin_of(w, i).is_some()) && is_viable_for_projectile_tweaks(w, a)
}

/// `hasMovement`: the actor's class has a ProjectileMovement component (through its owner's data).
fn has_movement(w: &World, a: ActorRef) -> bool {
    let owner = match a {
        ActorRef::Item(id) => w.actors.items.get(id).and_then(|i| i.owner.or(i.spawner)),
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).and_then(|p| p.owner),
        _ => None,
    };
    let (Some(o), Some(c)) = (owner, actor_cls(w, a)) else { return false };
    actor_def(w, ActorRef::Fighter(o), c)
        .is_some_and(|d| d.components.get("ProjectileMovement").is_some_and(|v| v.as_obj().is_some()))
}

// ---- the victim's mark

fn vext(w: &World, b: BuffRef) -> Option<&HomingVictimExt> {
    match &buff(w, b)?.ext {
        BuffExt::C018HomingVictim(e) => Some(e),
        _ => None,
    }
}

/// The extension taken out of the buff (put back with [`put_vext`]).
fn take_vext(w: &mut World, b: BuffRef) -> Option<HomingVictimExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::C018HomingVictim(e) => Some(std::mem::take(e)),
        _ => None,
    }
}

fn put_vext(w: &mut World, b: BuffRef, e: HomingVictimExt) {
    if let Some(x) = buff_mut(w, b)
        && let BuffExt::C018HomingVictim(slot) = &mut x.ext
    {
        *slot = e;
    }
}

/// `spawnHomingVictim`: SpawnBuffActor(Buff_C018HomingVictim_C, victim, inst, 0, duration, true, false) through
/// `caller`'s Comp_State_Manager (the victim's own @6692 / @8318; the flag's @669 / @1463: none). `duration <= 0` (the
/// mark's -1 @669 / @6692): no life timer. Called by flag.rs.
pub fn spawn_homing_victim(
    w: &mut World,
    victim: FighterId,
    inst: FighterId,
    duration: Real,
    caller: Option<FighterId>,
) {
    w.scratch.homing_refresh_dur = Some(duration);
    spawn_buff(w, victim, name!("Buff_C018HomingVictim_C"), duration, Some(0), Some(inst), true, caller);
    w.scratch.homing_refresh_dur = None;
}

/// MulticastUI(Timer, IsRefresh) @5213 (not on a dedicated server).
fn multicast_ui(w: &mut World, f: FighterId, timer: Real, refresh: bool) {
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: name!("HomingVictim"),
        state_type: 1,
        display_type: 0,
        current_value: -1.0,
        max_value: -1.0,
        timer,
        icon_material: name!("MI_State_MarvinFlag"),
        active: true,
        pop_text: if refresh { Name::NONE } else { name!("Lock On!") },
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

/// ApplyHomingBuff(Projectile) @0-919 on the victim `v`'s mark.
fn apply_homing_buff(w: &mut World, v: FighterId, inst: Option<FighterId>, a: ActorRef) {
    let once = match a {
        ActorRef::Projectile(id) => w
            .actors
            .projectiles
            .get(id)
            .is_some_and(|p| p.hit.hit_each_actor_only_once == Some(true) && p.hit_set.contains(&w.fighter(v).id)),
        ActorRef::Item(id) => w
            .actors
            .items
            .get(id)
            .is_some_and(|p| p.hit.hit_each_actor_only_once == Some(true) && p.hit_set.contains(&w.fighter(v).id)),
        _ => false,
    };
    if once {
        return; // @397-592
    }
    if let Some(s) = spawn_actor_buff(w, a, name!("Buff_C018HomingProjectile_C"), 0, 1.0, true, inst) {
        // @626
        if let Some(i) = crate::combat::elem::buff_place(w, a, s) {
            server_homing_info(w, a, i, Some(v)); // @874
        }
    }
}

/// @15: the begin overlap's flow with `a` the cast's temp. `link`: run from the Delay(0)'s link (its next Delay counts
/// from the next frame).
fn on_actor(w: &mut World, f: FighterId, e: &mut HomingVictimExt, a: ActorRef, link: bool) {
    let t = team_of(w, a);
    if t != -1 {
        if t == e.team && viable(w, e.marker, a) {
            apply_homing_buff(w, f, e.marker, a); // @371
            return;
        }
        if matches!(a, ActorRef::Item(_)) && !e.queued.contains(&a) {
            e.queued.push(a); // @588-663
        }
        return;
    }
    // @395: at most 5 retries over the buff's life, then Delay(0) @533 (a pending Delay ignores the call).
    if e.retries >= 5 {
        return;
    }
    e.retries += 1;
    if e.retry_at < 0 {
        e.retry_at = w.flow.tick + i32::from(link);
    }
}

/// `victimKind.begin`: ReceiveBeginPlay @963 (the owner is a fighter here: the BP_FighterCharacter cast holds).
fn victim_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, duration: Option<Real>) {
    let mut e = HomingVictimExt { marker: from, retry_at: -1, ..HomingVictimExt::default() };
    if let Some(i) = from {
        e.team = w.fighter(i).team.team; // @1390
    }
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::C018HomingVictim(e);
    }
    if from.is_none() {
        return; // @1309: no Instigator's Comp_Actor_Destructible -> no Team Index, no MulticastUI
    }
    let t = duration.filter(|&d| d > 0.0).unwrap_or(-1.0);
    multicast_ui(w, f, t, false); // @1524
}

/// `victimKind.status` (BPI After Status Update Multicast): RefreshBuff's StatusUpdateMulticast(0, refreshed) is
/// followed by @823's MulticastUI(BuffDuration > 0 ? BuffDuration : -1, true).
fn victim_status(w: &mut World, f: FighterId, _cls: BuffClass, _stacks: Real, applied: bool, removed: bool) {
    if applied || removed {
        return;
    }
    let Some(b) = buff_of(w, f, name!("Buff_C018HomingVictim_C")) else { return };
    let dur = w.scratch.homing_refresh_dur.unwrap_or_else(|| {
        let fr = buff(w, b).map_or(Real::INFINITY, |x| x.frames);
        if fr == Real::INFINITY { -1.0 } else { (fr - 1.0) * 0.0166668 }
    });
    multicast_ui(w, f, if dur > 0.0 { dur } else { -1.0 }, true);
}

/// `victimKind.on`: OwnerKnockedBack @2127.
fn victim_on(w: &mut World, _f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::KnockedBack || buff(w, b).is_none_or(|x| x.frames == Real::INFINITY) {
        return false; // BuffDuration > 0
    }
    let Some(by) = a.by else { return false };
    if by.fighter.is_some() || matches!(by.actor, ActorRef::Fighter(_) | ActorRef::Buff(_) | ActorRef::Stage(_)) {
        return false; // the Attacker a SpawnedActorParent
    }
    by.attack_type == 0 || by.attack_type == 1 // @2321-2421 -> K2_DestroyActor
}

/// `victimKind.timer`: CheckForProjectiles' overlaps, the Delay(0) retry and ReceiveTick (the header's note on the
/// pass).
fn victim_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(mut e) = take_vext(w, b) else { return false };
    let mut now: Vec<ActorRef> = Vec::new();
    if w.fighter(f).life.dead <= 0.0 {
        let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
        let types = [crate::real(OBJ_PROJECTILE_DESTRUCTIBLE), crate::real(OBJ_ITEM_DESTRUCTIBLE)];
        for o in sphere_overlap_actors(w, x, y, CHECK_RADIUS, &types, &[], OBJ_HIT_BOX) {
            if is_spawned(o) && !actor_dead(w, o) {
                now.push(o);
            }
        }
    }
    // End edges @2446: out of QueuedItems.
    for &a in &e.over {
        if now.contains(&a) || !matches!(a, ActorRef::Item(_)) {
            continue;
        }
        if let Some(i) = e.queued.iter().position(|&q| q == a) {
            e.queued.remove(i);
        }
    }
    // Begin edges @5109.
    let before = std::mem::replace(&mut e.over, now);
    for a in e.over.clone() {
        if before.contains(&a) {
            continue;
        }
        e.last_cast = Some(a);
        e.last_cast_seq = match a {
            ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|p| p.seq),
            ActorRef::Item(id) => w.actors.items.get(id).map(|i| i.seq),
            _ => None,
        };
        on_actor(w, f, &mut e, a, false);
    }
    // The Delay(0) @533 -> @15 with the cast's temp as it is now.
    if e.retry_at >= 0 && w.flow.tick >= e.retry_at {
        e.retry_at = -1;
        if let Some(a) = e.last_cast
            && !actor_dead(w, a)
        {
            on_actor(w, f, &mut e, a, true);
        }
    }
    // ReceiveTick @5779: QueuedItems backwards.
    let mut i = e.queued.len();
    while i > 0 {
        i -= 1;
        let Some(&it) = e.queued.get(i) else { continue };
        if actor_dead(w, it) || team_of(w, it) != e.team || !viable(w, e.marker, it) {
            continue;
        }
        apply_homing_buff(w, f, e.marker, it);
        if let Some(j) = e.queued.iter().position(|&q| q == it) {
            e.queued.remove(j);
        }
    }
    put_vext(w, b, e);
    false
}

/// `victimKind.end`: ReceiveEndPlay @5055 (authority): the overlapped actors' HomingProjectile buffs on this owner
/// end; the HUD state goes.
fn victim_end(w: &mut World, f: FighterId, b: BuffRef) {
    if let Some(e) = vext(w, b) {
        let (over, team) = (e.over.clone(), e.team);
        for a in over {
            if actor_dead(w, a) || team_of(w, a) != team {
                continue;
            }
            let hb = actor_elem(w, a).and_then(Option::as_ref).and_then(|s| {
                s.buffs.iter().find(|x| {
                    matches!(x.class, BuffClass::BuffC018HomingProjectile | BuffClass::BuffC018PerkHomingProjectile)
                })
            });
            let Some(hb) = hb else { continue };
            let serial = hb.serial;
            if let ElemExt::C018HomingProjectile(x) = hb.ext
                && x.target == Some(f)
            {
                end_actor_buff(w, a, serial);
            }
        }
    }
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("HomingVictim"))); // @4001
}

// ---- Buff_C018HomingProjectile

fn pext(w: &World, a: ActorRef, b: u32) -> Option<HomingProjExt> {
    match elem_buff(w, a, b)?.ext {
        ElemExt::C018HomingProjectile(e) => Some(e),
        _ => None,
    }
}

fn pext_mut(w: &mut World, a: ActorRef, b: u32) -> Option<&mut HomingProjExt> {
    match &mut elem_buff_mut(w, a, b)?.ext {
        ElemExt::C018HomingProjectile(e) => Some(e),
        _ => None,
    }
}

/// VSize in float32.
fn vsize(x: Real, y: Real) -> Real {
    ((x * x) + (y * y)).sqrt()
}

/// Normal(V, 0.0001) x speed (the Normal node: a squared size of 1 returns V, one below the tolerance the zero vector,
/// else V x InvSqrt).
fn normal_times(vx: Real, vy: Real, speed: Real) -> (Real, Real) {
    let [_, nx, ny] = crate::math::ue::normal([0.0, vx, vy], crate::math::ue::KINDA_SMALL_NUMBER);
    ((nx * speed), (ny * speed))
}

/// `serverHomingInfo` (ServerHomingInfo(Fighter) @15) on a live buff `b` of actor `a` (perks.rs's perk child calls it
/// too, with its HomingTarget: `None` when its platform found none). The Velocity test comes first; a Fighter that is no
/// BP_FighterCharacter (`None` here) then fails the cast @93-158 and the event ends: no Velocity write, no
/// MulticastHomingInfo, ValuesAssigned stays false.
pub fn server_homing_info(w: &mut World, a: ActorRef, b: u32, v: Option<FighterId>) {
    let Some(e) = pext(w, a, b) else { return };
    let Some((ax, ay, avx, avy)) = state_of(w, a) else { return };
    let (im, min_speed, mag_mult) = {
        let d = actor_inst_def(w, a, b);
        let d = d.as_deref();
        (
            d.and_then(|d| num(d, "InitialVelocityMultiplier")).unwrap_or(0.0),
            d.and_then(|d| num(d, "MinSpeedOnLockOnStart")).unwrap_or(0.0),
            d.and_then(|d| num(d, "MagnitudeVelocityMultiplier")).unwrap_or(0.0),
        )
    };
    let (vx, vy) = if e.pm { (avx, avy) } else { (0.0, 0.0) }; // a None component reads the zero vector
    if vx == 0.0 && vy == 0.0 {
        if let Some(x) = pext_mut(w, a, b) {
            x.waiting = true; // Delay(0) @833 -> @15 (the actor buff's next tick)
            x.pending = v;
        }
        return;
    }
    let Some(v) = v else { return }; // @158 pop_flow
    let speed = max(min_speed, vsize(vx * im, vy * im)); // FMax @435
    let (vfx, vfy) = (w.fighter(v).body.x, actor_y(w, v));
    let (nx, ny) = normal_times(vfx - ax, vfy - ay, speed); // @481-569
    set_v(w, a, nx, ny); // @615
    multicast_homing_info(w, a, b, v, vsize(nx, ny) * mag_mult, nx, ny); // @769
}

/// MulticastHomingInfo @4010.
fn multicast_homing_info(w: &mut World, a: ActorRef, b: u32, v: FighterId, mag: Real, vx: Real, vy: Real) {
    let Some(e) = pext_mut(w, a, b) else { return };
    e.assigned = true; // @4085
    if !e.pm {
        return; // @4196
    }
    e.target = Some(v); // New Homing Component @4488
    set_v(w, a, vx, vy); // @4277
    set_homing(w, a, ActorRef::Fighter(v), mag); // magnitude @4397, target @4578, bIsHomingProjectile @4806
    set_gravity(w, a, 0.0); // @4690
}

/// `homingProjectileActor.begin`: ReceiveBeginPlay @888.
fn projectile_begin(w: &mut World, a: ActorRef, b: u32, from: Option<FighterId>) -> bool {
    if !is_spawned(a) || !viable(w, from, a) {
        return true; // @983 / @1082 / @1171
    }
    let e = HomingProjExt {
        pm: has_movement(w, a),
        o_home: buff_home_of(w, a),
        o_grav: gravity_of(w, a),
        ..HomingProjExt::default()
    };
    if let Some(x) = elem_buff_mut(w, a, b) {
        x.ext = ElemExt::C018HomingProjectile(e);
    }
    false // (authority) HitAnything bound @2265: `on` below.
}

/// `homingProjectileActor.tick`: ReceiveTick @2659: the cancel test @2346, then its pending Delay(0) (the actor's
/// latent actions come behind its tick).
fn projectile_tick(w: &mut World, a: ActorRef, b: u32) -> bool {
    let Some(e) = pext(w, a, b) else { return false };
    let home = if e.pm { buff_home_of(w, a) } else { None };
    let differs = match (home, e.target) {
        (None, _) | (Some(_), None) => true,
        (Some((to, _)), Some(t)) => to != ActorRef::Fighter(t),
    };
    if e.assigned && !e.closed && differs {
        if let Some(x) = pext_mut(w, a, b) {
            x.closed = true;
            x.cancelled = true; // MulticastCancel @2964
        }
        return true; // K2_DestroyActor @3005
    }
    if e.waiting {
        let v = e.pending;
        if let Some(x) = pext_mut(w, a, b) {
            x.waiting = false;
            x.pending = None;
        }
        server_homing_info(w, a, b, v);
    }
    false
}

/// `homingProjectileActor.on`: OnHitAnything @4845.
fn projectile_on(_w: &mut World, _a: ActorRef, _b: u32, s: ActorBuffSignal, _arg: Option<&ActorBuffArg>) -> bool {
    s == ActorBuffSignal::HitAnything
}

/// `homingProjectileActor.end`: ReceiveEndPlay @3020.
fn projectile_end(w: &mut World, a: ActorRef, b: u32) {
    if !is_spawned(a) {
        return;
    }
    let e = pext(w, a, b);
    if e.is_some_and(|e| e.cancelled) || !has_movement(w, a) {
        return; // @3149 / @3259
    }
    // A buff its BeginPlay destroyed kept nothing: the variables' defaults (off, None, 0, gravity 0) are written.
    match e.and_then(|e| e.o_home) {
        Some((to, mag)) => {
            let h = crate::actors::homing::buff_home(to, mag);
            match a {
                ActorRef::Projectile(id) => {
                    if let Some(p) = w.actors.projectiles.get_mut(id) {
                        p.buff_home = Some(h);
                    }
                }
                ActorRef::Item(id) => {
                    if let Some(it) = w.actors.items.get_mut(id) {
                        it.buff_home = Some(h);
                        it.write_homing(true);
                    }
                }
                _ => {}
            }
        }
        None => match a {
            ActorRef::Item(_) => clear_homing(w, a),
            ActorRef::Projectile(id) => {
                if let Some(p) = w.actors.projectiles.get_mut(id) {
                    p.buff_home = None;
                }
            }
            _ => {}
        },
    }
    set_gravity(w, a, e.map_or(0.0, |e| e.o_grav)); // @3684
}

/// `homingProjectileActor`: the actor hooks of Buff_C018HomingProjectile_C and of perks.rs's
/// Buff_C018PerkHomingProjectile_C.
pub fn projectile_actor() -> ActorBuffHooks {
    ActorBuffHooks {
        begin: Some(projectile_begin),
        tick: Some(projectile_tick),
        on: Some(projectile_on),
        end: Some(projectile_end),
        ..ActorBuffHooks::default()
    }
}

/// This file's registry entries: the two buffs.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let victim = BuffHooks {
        begin: Some(victim_begin),
        status: Some(victim_status),
        on: Some(victim_on),
        timer: Some(victim_timer),
        end: Some(victim_end),
        ..BuffHooks::default()
    };
    r.buffs.enter(HOMING_VICTIM, BuffEntry::new(BuffClass::BuffC018HomingVictim, victim))?;
    r.buffs.enter(
        HOMING_PROJECTILE,
        BuffEntry {
            actor: Some(projectile_actor()),
            ..BuffEntry::new(BuffClass::BuffC018HomingProjectile, BuffHooks::default())
        },
    )
}
