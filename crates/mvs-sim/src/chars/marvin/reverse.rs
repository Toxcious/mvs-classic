//! Marvin's reversal: Pawn_C018's BPI_ReverseProjectiles
//! (HandleReverseAllProjectiles, ReverseProjectile), Buff_C018ReverseProjectile_C (an actor buff, its ext
//! [`ReverseExt`]) and AnimNotify_C018ReverseProjectiles_C. No spawned actor, no pawn hook and no state on Marvin.
//!
//! The ProjectileReversed delegate (`onProjectileReversed` / `projectileReversed`, a listener list that perks.rs's
//! Redirect fills at its load) is a direct call to perks.rs's
//! [`super::perks::redirect_reversed`] from [`projectile_reversed`].
// The listener list (set once at load) is the one direct call above: the same listener in the same place, with no
// list.
//
// HandleReverseAllProjectiles (BPI_ReverseProjectiles @2888), authority: 1. Marvin's Comp_Spawner.SpawnedActors in order
// @81-529: ReverseProjectile on each; a success AddUnique's the actor to LocalReversedProjectiles; 2. each ally with a
// valid Comp_Spawner (GetAllies @530: the other fighters of his team that are not knocked out, players only), its
// SpawnedActors @959-1733, the same; 3. every ItemActorParent not in the list @2022 whose TeamIndex is Marvin's @2380:
// the same; 4. with any success, TriggerAbilityServer(3) @2722. The first two passes do not test the list.
// Unverified: GetAllActorsOfClass's order is taken as the items' (the spawn order).
//
// Buff_C018ReverseProjectile on the reversed actor (Instigator Marvin). Class defaults: DurationOfPause 0.5, CanRefresh,
// AlwaysRefresh. ReceiveBeginPlay @507: the owner a SpawnedActorParent and not ActorHasTag(NoReverse), else K2_DestroyActor
// @804; BPI_SetMobilityToStationary(true) @728, BPI_SetLifetimePaused(true) @318 and the one-shot timer
// (MulticastReverseMovement, DurationOfPause) @379. MulticastReverseMovement @895: the owner's BPI_ReverseMoveDirection,
// then K2_DestroyActor. ReceiveEndPlay @1036: BPI_SetMobilityToStationary(false) @1154, BPI_SetLifetimePaused(false)
// @109.
// Deviation: the one-shot timer is counted in the actor's buff pass (`actor.tick`, as the buffs' life timers there),
// timerFrames(DurationOfPause) ticks after the spawn, not in the world's timer pass.
// BPI_SetMobilityToStationary @728 / @1154 (`combat::holds::set_mobility_to_stationary`): of the World.actors kinds
// only Jerry_SpawnedActor's own ProjectileMovement reads it.
// Unverified: a shield actor (WW_ShieldActor, Velma_ProjectileShieldActor) goes on following its owner under a
// Stationary root (`actors::shield_actor` moves it by place, not by a ProjectileMovement).

use super::pawn::is_viable_for_projectile_tweaks;
use super::state::marvin_of;
use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::item::item_destructible_team;
use crate::actors::projectile::LifetimePause;
use crate::actors::world_actor::{actor_cls, actor_dead};
use crate::combat::BuffClass;
use crate::combat::elem::{ElemExt, actor_inst_def, elem_buff_mut, spawn_actor_buff};
use crate::combat::holds::set_mobility_to_stationary;
use crate::combat::reverse::reverse_move_direction;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_def, lookup_owner, peers, timer_frames, warn_text};
use crate::fighter::control::trigger_ability;
use crate::name;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks};
use crate::registry::props::num;
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Value;

/// Buff_C018ReverseProjectile's class.
pub const REVERSE_PROJECTILE: &str = "Buff_C018ReverseProjectile_C";

/// TriggerAbilityServer(3) @2722: the reversal's slot.
const REVERSE_ABILITY: i32 = 3;

/// Buff_C018ReverseProjectile_C's extension on the actor (`ElemExt::C018ReverseProjectile`).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct ReverseExt {
    /// The pause's ticks left (`t`): timerFrames(DurationOfPause), counted down in the buff's tick.
    pub t: Real,
}

/// `reverseProjectiles` (HandleReverseAllProjectiles): the sweep of Marvin's, his allies' and his team's items.
pub fn reverse_projectiles(w: &mut World, f: FighterId) {
    let mut done: Vec<ActorRef> = Vec::new();
    let mut any = false;
    let try_one = |w: &mut World, a: ActorRef, done: &mut Vec<ActorRef>, any: &mut bool| {
        if !reverse_projectile(w, f, a) {
            return;
        }
        *any = true;
        if !done.contains(&a) {
            done.push(a);
        }
    };
    for a in w.fighter(f).items.spawned_actors.clone() {
        if !actor_dead(w, a) {
            try_one(w, a, &mut done, &mut any);
        }
    }
    let team = w.fighter(f).team.team;
    // GetAllies: players only
    for o in peers(w).to_vec() {
        let x = w.fighter(o);
        if o == f || x.team.team != team || x.life.dead > 0.0 || x.life.is_minion {
            continue;
        }
        for a in x.items.spawned_actors.clone() {
            if !actor_dead(w, a) {
                try_one(w, a, &mut done, &mut any);
            }
        }
    }
    let ids: Vec<_> = w.actors.items.ids().collect();
    for id in ids {
        let a = ActorRef::Item(id);
        if actor_dead(w, a) || done.contains(&a) {
            continue;
        }
        if item_destructible_team(w, id) != team {
            continue;
        }
        try_one(w, a, &mut done, &mut any);
    }
    if any {
        trigger_ability(w, f, REVERSE_ABILITY);
    }
}

/// ReverseProjectile(Actor) (its own function @0-177): viable -> SpawnBuffActor(Buff_C018ReverseProjectile_C) @46,
/// broadcast ProjectileReversed @122, Success true (the new buff is not tested); else false.
fn reverse_projectile(w: &mut World, f: FighterId, a: ActorRef) -> bool {
    if actor_dead(w, a) || !is_viable_for_projectile_tweaks(w, a) {
        return false;
    }
    spawn_actor_buff(w, a, name!("Buff_C018ReverseProjectile_C"), 0, 0.0, true, Some(f));
    projectile_reversed(w, f, a);
    true
}

/// `projectileReversed` (the ProjectileReversed broadcast @122): perks.rs's Redirect listener.
pub fn projectile_reversed(w: &mut World, f: FighterId, a: ActorRef) {
    super::perks::redirect_reversed(w, f, a);
}

/// The actor's LifetimePausedCount holder (`lifetimePause`).
fn pause_slot(w: &mut World, a: ActorRef) -> Option<&mut Option<LifetimePause>> {
    let w = &mut w.actors;
    match a {
        ActorRef::Projectile(id) => w.projectiles.get_mut(id).map(|p| &mut p.lifetime_pause),
        ActorRef::Item(id) => w.items.get_mut(id).map(|p| &mut p.lifetime_pause),
        ActorRef::Actor(id) => w.actors.get_mut(id).map(|p| &mut p.lifetime_pause),
        ActorRef::Marker(id) => w.markers.get_mut(id).map(|p| &mut p.lifetime_pause),
        ActorRef::Mortar(id) => w.mortars.get_mut(id).map(|p| &mut p.lifetime_pause),
        _ => None,
    }
}

/// `setLifetimePaused` (SpawnedActorParent BPI_SetLifetimePaused(Paused) @8637 -> @3249-3614, authority): true counts
/// LifetimePausedCount up and K2_PauseTimerHandle(MaxLifetimeTimer); false, with the count above 0, counts it down and
/// at 0 K2_UnPauseTimerHandle. A Projectile's MaxLifetimeTimer is `pause_max_lifetime` / `unpause_max_lifetime`; an
/// item has none in the port, so only its count moves; a world actor kind with a MaxLifetime warns. A Projectile's
/// UpdateMaxLifetimeWithAuthority while the count is above 0 pauses the new timer at once (@4432-4476,
/// `update_max_lifetime`).
pub fn set_lifetime_paused(w: &mut World, a: ActorRef, paused: bool) {
    if !matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_)) {
        let ml = crate::actors::world_actor::actor_p(w, a).get("MaxLifetime").and_then(Value::as_f64).unwrap_or(0.0);
        if ml > 0.0 {
            let c = actor_cls(w, a).map_or_else(String::new, |c| w.names.str(c).to_owned());
            warn_text(
                w,
                &format!(
                    "BPI_SetLifetimePaused on {c} (a World.actors entry with a MaxLifetime): its timer is not paused"
                ),
            );
        }
    }
    let n = pause_slot(w, a).and_then(|s| s.as_ref().map(|x| x.count)).unwrap_or(0);
    if paused {
        let left = match a {
            ActorRef::Projectile(id) if n == 0 => {
                w.actors.projectiles.get_mut(id).map(crate::actors::projectile::pause_max_lifetime)
            }
            _ => None,
        };
        let Some(s) = pause_slot(w, a) else { return };
        let p = s.get_or_insert_with(LifetimePause::default);
        p.count = n + 1;
        if let Some(l) = left {
            p.left = l;
        }
        return;
    }
    if n <= 0 {
        return;
    }
    let Some(s) = pause_slot(w, a) else { return };
    if n > 1 {
        if let Some(x) = s.as_mut() {
            x.count = n - 1;
        }
        return;
    }
    let left = s.take().map_or(0, |x| x.left);
    if let ActorRef::Projectile(id) = a
        && let Some(p) = w.actors.projectiles.get_mut(id)
    {
        crate::actors::projectile::unpause_max_lifetime(p, left);
    }
}

/// The buff's `actor.begin` @507.
fn actor_begin(w: &mut World, a: ActorRef, b: u32, _from: Option<FighterId>) -> bool {
    let spawned = matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_)) || {
        let own = ActorRef::Fighter(lookup_owner(w, a));
        actor_cls(w, a)
            .and_then(|c| actor_def(w, own, c))
            .is_some_and(|d| format!(" {} ", d.supers.as_deref().unwrap_or("")).contains(" SpawnedActorParent_C "))
    };
    if !spawned {
        return true;
    }
    let tagged = crate::actors::world_actor::actor_p(w, a)
        .get("Tags")
        .and_then(Value::as_arr)
        .is_some_and(|t| t.iter().any(|v| v.as_str() == Some("NoReverse")));
    if tagged {
        return true;
    }
    set_mobility(w, a, true);
    set_lifetime_paused(w, a, true);
    let dur = actor_inst_def(w, a, b).and_then(|d| num(&d, "DurationOfPause")).unwrap_or(0.0);
    if let Some(x) = elem_buff_mut(w, a, b) {
        x.ext = ElemExt::C018ReverseProjectile(ReverseExt { t: timer_frames(Some(dur)) });
    }
    false
}

/// `setMobilityToStationary` on the actor's root.
fn set_mobility(w: &mut World, a: ActorRef, on: bool) {
    set_mobility_to_stationary(w, a, on);
}

/// The buff's `actor.tick`: the pause's end.
fn actor_tick(w: &mut World, a: ActorRef, b: u32) -> bool {
    let Some(x) = elem_buff_mut(w, a, b) else { return false };
    let ElemExt::C018ReverseProjectile(e) = &mut x.ext else { return false };
    e.t -= 1.0;
    if e.t > 0.0 {
        return false;
    }
    reverse_move_direction(w, a);
    true
}

/// The buff's `actor.end` @1036.
fn actor_end(w: &mut World, a: ActorRef, _b: u32) {
    set_mobility(w, a, false);
    set_lifetime_paused(w, a, false);
}

/// AnimNotify_C018ReverseProjectiles_C's begin @0-361 (no NotifyEnd): BPI_ReverseProjectiles @313.
fn reverse_begin(w: &mut World, f: FighterId, _n: NotifyRef, _i: i32) {
    if marvin_of(w, f).is_none() {
        return;
    }
    reverse_projectiles(w, f);
}

/// This file's registry entries: the buff (actor hooks only) and the notify.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let actor = ActorBuffHooks {
        begin: Some(actor_begin),
        tick: Some(actor_tick),
        end: Some(actor_end),
        ..ActorBuffHooks::default()
    };
    r.buffs.enter(
        REVERSE_PROJECTILE,
        BuffEntry { actor: Some(actor), ..BuffEntry::new(BuffClass::BuffC018ReverseProjectile, BuffHooks::default()) },
    )?;
    r.notifies.enter(
        "AnimNotify_C018ReverseProjectiles_C",
        NotifyHandler { begin: Some(reverse_begin), ..NotifyHandler::default() },
    )
}
