//! Rick's Comp_Pawn_Portal: the allied portals, the spawn of a portal, the
//! teleport of an actor through one (TeleportServer), the TeleportVisuals delay; joined to world_tick. Its variables are
//! `RickState::portal_comp` (state.rs). Class data (Comp_Pawn_Portal_GEN_VARIABLE on Pawn_C020): MaxPortals 2,
//! ImmediatelyDestroyExtraPortals, UseTeleportBuff, TeleportProjectiles, OnlyRotateCharactersIfHitstun,
//! FighterTeleportDurationShort 0.2 / Long 0.5, LongTeleportDistance 5000, MeleeBuffDuration 6, "Do 1 Portal At A Time
//! Approach", PortalSpawnAtOwnerOffset (0, 200, 0); OnlyTeleportAllies false (absent). Sim space: x = -UE Y, y = UE Z.
//! Offsets are of the component's ubergraph unless a function is named.
//!
//! What the portals (portal.rs) call and provide:
//! - [`teleport_server`]: TeleportServer -> HandleTeleport, on the component of the enter portal's owner. actor: a
//!   fighter, a projectile, an item or a World.actors entry.
//! - [`add_allied_portal`] / [`get_allied_portals`] / [`remove_allied_portal`]: Add / Get / Remove Allied Portal. The
//!   array is the build's own: Remove takes every copy out and nothing else; Add is AddUnique then CleanUpAlliedPortals;
//!   Get is CleanUp then a copy of the array. A portal destroyed while listed stays in it (invalid) until the next
//!   CleanUp, so a portal's EndPlay that does Remove then UpdatePortalCount (which adds it back while IsValid holds
//!   inside its own EndPlay) leaves it listed: SpawnObject's loop and HandleTeleport's "more than one" read the raw
//!   length, as the build.
//! - [`spawn_object`]: SpawnObject(SpawnLocation, Facing, PortalClass) on the owner's component (the aimer's calls,
//!   C020PortalAimer @3346 / @4837); Facing is not read.
//!
//! HandleTeleport(EnterPortal, TeleportingActor) (function @0-3144):
//! - More than one entry in AlliedPortals @124: (OnlyTeleportAllies -> CheckIfAlly @152) SetNearestPortal @198 (the
//!   nearest other entry by K2_GetActorLocation, strictly nearer, whose Is Teleporting Actor is false; an invalid
//!   winner gives None @1179) -> None: nothing.
//! - One or none: with "Do 1 Portal At A Time Approach" @1660, the location in front of Rick (his location +
//!   PortalSpawnAtOwnerOffset with Y by BPI_IsFacingLeft: +1 left, -1 right @1988-2128) made safe by
//!   BPI_GetSafePortalLocation(175, 300, it, his location) @2244 (an owner without BPI_C020: (0, 0, 0) @3108) into
//!   LocalNewPortalLocation, SpawnObject(it, !FacingLeft, VisualOnlyPortal_C) @2374, then @301 with no exit portal.
//! - A Base_Fighter @301: the exit valid -> IncrementFightersTeleported @423; Direct Fighter Velocity on Exit @593; an
//!   ally (CheckIfAlly @670): Rick with Buff_C020PortalMeleeBuff -> AddPortalBuff @963, with Buff_C020PortalCatch ->
//!   CatchAlly @1234 (perks: no data of the build gives either buff); else SetMostRecentAttacker @2900; Teleport FX
//!   Calls(actor, enter, the exit's location or LocalNewPortalLocation) @1445; UseTeleportBuff -> TeleportFighter @1500
//!   (false: nothing more); else TeleportActor @3062; broadcast AfterActorTeleported @1582 (to the owner's buffs:
//!   Buff_C020_Portal); EnterPortal.AddTeleportedActor @1610.
//! - Else a BPI_SpawnedActor with TeleportProjectiles @2512: TeleportActor @2526, the broadcast, Teleport FX Calls
//!   @2729, EnterPortal.AddTeleportedActor @2770, the exit valid -> MulticastFighterTeleportedEffects(false) @2858.
//!
//! SpawnObject @2506: AlliedPortals' length + 1 > MaxPortals -> the loop @754 (its condition re-read each pass: i <
//! length && no break): i < length + 1 - MaxPortals ? (Immediately -> K2_DestroyActor @1282 : not TransitioningOut ->
//! Transition Out @2469) : break; then the owner a BP_Fighter_Character with a Comp_Spawner: CompSpawnActor(PortalClass)
//! at SpawnLocation + (0, 0, BPI_GetCapsuleCollision's unscaled half height - 100), X 0, no rotation @1700-2045.
//! Teleport FX Calls (function, owner authority): BeamVisuals (empty), TeleportVisuals(ExitLocation, actor) @2708 (a
//! fighter: sfx_cha_C020_portal_gun_warp_basefighter, else _warp_generic, attached to Rick, then Delay(0.5) @699 -> @15
//! sfx_cha_C020_portal_gun_exit at the event's ExitLocation as it is when the Delay ends; a call while the Delay runs
//! posts its warp sound and leaves the Delay), TeleportAudio(actor) @3691 (sfx_cha_C020_portal_gun_fail at the actor's
//! location, attached to it when it is a fighter).
#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(a < b)` is true for NaN")]
// Unverified (portalcomp a): the TeleportVisuals Delay counts from Rick's next world_tick (the component's latent
// action; its place in the frame not read).
// Deviation (portalcomp b): the component's ReceiveEndPlay (K2_DestroyActor of every AlliedPortals entry @3336) is not
// ported: Rick's pawn is not destroyed in a match of the port (a match reset makes a new state and the world's reset
// ends the actors). ClearPortalServer / HandleClearPortals, TransitionOutAllPortal, RotateActor and Calulate Spawned
// Actor Rotation have no caller in the build's graphs and are not ported.
// K2_TeleportTo of a spawned actor is world::spawn_spot k2_teleport_to (its note: an Item or a World.actors entry is
// placed with no spot test); its return value is read by nothing here.
// Deviation (portalcomp f): a spawned actor's TeamIndex is taken as its owner's team (its Comp_Actor_Destructible
// TeamIndex is not kept by the port); an actor whose team differs from its spawner's is not met.
// SpawnObject lists the actor before its BeginPlay (`comp_spawn_actor`'s order).

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::actor_place;
use crate::combat::buffs::{buff_of, buff_signal, spawn_buff};
use crate::combat::elem::instigator_charge;
use crate::combat::hit::{remove_hitstun_from_knock_back, set_most_recent_attacker};
use crate::events::Event;
use crate::fighter::common::{data_of, latent_frames, warn_text};
use crate::fighter::{FighterId, actor_y, facing, in_hitstun};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::CharHooks;
use crate::registry::props::{flag, num, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::world::World;
use crate::world::spawn_spot::k2_teleport_to;
use crate::world::spawning::comp_spawn_actor;

use super::portal::{
    add_teleported_actor, destroy_actor, increment_fighters_teleported, is_spawned_actor_parent, is_teleporting_actor,
    multicast_fighter_teleported_effects, new_portal, portal_actor, portal_id, transition_out,
};
use super::state::{PortalComp, rick_mut, rick_of};
use super::teleport::{activate_teleport, bpi_get_safe_portal_location, is_c020, is_valid, portal_loc, vsize};

const COMP: &str = "Comp_Pawn_Portal_GEN_VARIABLE";
const VISUAL_ONLY: Name = name!("VisualOnlyPortal_C");
const MELEE_PERK: Name = name!("Buff_C020PortalMeleeBuff_C");
const CATCH_PERK: Name = name!("Buff_C020PortalCatch_C");
const SFX_WARP_FIGHTER: Name = name!("sfx_cha_C020_portal_gun_warp_basefighter");
const SFX_WARP_GENERIC: Name = name!("sfx_cha_C020_portal_gun_warp_generic");
const SFX_EXIT: Name = name!("sfx_cha_C020_portal_gun_exit");
const SFX_FAIL: Name = name!("sfx_cha_C020_portal_gun_fail");

/// The class data (no default: a missing value warns and reads 0 / false).
fn params(w: &World, f: FighterId) -> Option<&mvs_data::value::Props> {
    data_of(w, f).fighter.components.pawn.get(COMP).and_then(|c| c.p.as_obj())
}

fn pnum(w: &World, f: FighterId, k: &str) -> Real {
    match params(w, f).and_then(|p| num(p, k)) {
        Some(v) => v,
        None => {
            warn_text(w, &format!("Comp_Pawn_Portal data missing: {k}"));
            0.0
        }
    }
}

fn pflag(w: &World, f: FighterId, k: &str) -> bool {
    params(w, f).is_some_and(|p| flag(p, k))
}

/// GetComponentByClass(Comp_Pawn_Portal_C): a pawn without one has none (a Rick's only).
pub fn has_comp(w: &World, f: FighterId) -> bool {
    params(w, f).is_some() && rick_of(w, f).is_some()
}

fn comp(w: &World, f: FighterId) -> Option<&PortalComp> {
    params(w, f)?;
    rick_of(w, f).map(|s| &s.portal_comp)
}

fn comp_mut(w: &mut World, f: FighterId) -> Option<&mut PortalComp> {
    params(w, f)?;
    rick_mut(w, f).map(|s| &mut s.portal_comp)
}

// ------------------------------------------------------------------ AlliedPortals

/// CleanUpAlliedPortals: the loop re-reads the length and does not step back after a removal.
fn clean_up(w: &mut World, f: FighterId) {
    let mut i = 0;
    while let Some(a) = comp(w, f).and_then(|c| c.allied.get(i).copied()) {
        if !is_valid(w, Some(a))
            && let Some(c) = comp_mut(w, f)
        {
            c.allied.retain(|&x| x != a);
        }
        i += 1;
    }
}

/// `addAlliedPortal`. Called by portal.rs.
pub fn add_allied_portal(w: &mut World, owner: FighterId, p: ActorRef) {
    let Some(c) = comp_mut(w, owner) else { return };
    if !c.allied.contains(&p) {
        c.allied.push(p);
    }
    clean_up(w, owner);
}

/// `removeAlliedPortal`. Called by portal.rs.
pub fn remove_allied_portal(w: &mut World, owner: FighterId, p: ActorRef) {
    if let Some(c) = comp_mut(w, owner) {
        c.allied.retain(|&x| x != p);
    }
}

/// `getAlliedPortals`. Called by aimer.rs and portal.rs.
// Deviation: takes `&mut World` (CleanUp writes the array).
pub fn get_allied_portals(w: &mut World, owner: FighterId) -> Vec<ActorRef> {
    if comp(w, owner).is_none() {
        return Vec::new();
    }
    clean_up(w, owner);
    comp(w, owner).map(|c| c.allied.clone()).unwrap_or_default()
}

// ------------------------------------------------------------------ the component's functions

/// CheckIfAlly(EnteredActor): the TeamIndex of the fighter's or the spawned actor's Comp_Actor_Destructible against the
/// owner's; anything else false (Deviation portalcomp f).
fn check_if_ally(w: &World, owner: FighterId, a: ActorRef) -> bool {
    if let ActorRef::Fighter(g) = a {
        return w.fighter(g).team.team == w.fighter(owner).team.team;
    }
    if !is_spawned_actor_parent(w, a) {
        return false;
    }
    let o = match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).and_then(|p| p.owner),
        ActorRef::Item(id) => w.actors.items.get(id).and_then(|p| p.owner),
        ActorRef::Actor(id) => w.actors.actors.get(id).map(|p| p.owner),
        _ => None,
    };
    o.map_or(-1, |f| w.fighter(f).team.team) == w.fighter(owner).team.team
}

/// SetNearestPortal(TeleportingActor, EnterPortal) (function @0-1411).
fn set_nearest_portal(w: &mut World, owner: FighterId, a: ActorRef, enter: ActorRef) -> Option<ActorRef> {
    let mut min_d: Real = 1.590898e18;
    let mut best: Option<ActorRef> = None;
    let list = comp(w, owner).map(|c| c.allied.clone()).unwrap_or_default();
    for p in list {
        if p == enter {
            continue;
        }
        let (ax, ay) = portal_loc(w, Some(enter));
        let (bx, by) = portal_loc(w, Some(p));
        let d = vsize(bx - ax, by - ay);
        if !(d < min_d) {
            continue;
        }
        if is_valid(w, Some(p)) && is_teleporting_actor(w, p, a) {
            continue; // the call on a pending-kill actor reads false
        }
        min_d = d;
        best = Some(p);
    }
    if is_valid(w, best) { best } else { None }
}

/// Direct Fighter Velocity on Exit (function): (0, 0, |v|) unless OnlyRotateCharactersIfHitstun and he is not in
/// hitstun (then his velocity).
fn direct_velocity(w: &World, owner: FighterId, g: FighterId) -> (Real, Real) {
    let b = &w.fighter(g).body;
    if pflag(w, owner, "OnlyRotateCharactersIfHitstun") && !in_hitstun(w, g) {
        return (b.vx, b.vy);
    }
    (0.0, vsize(b.vx, b.vy))
}

/// CatchAlly(Fighter, CurrentVelocity) (function, perk): a team mate in hitstun -> MulticastCatch (HitstunHandler
/// RemoveHitstunFromKnockBack @4066) and the velocity Normal(v, 1e-4) x |v| x 0.3. Returns the velocity.
pub fn catch_ally(w: &mut World, owner: FighterId, g: FighterId, vx: Real, vy: Real) -> (Real, Real, bool) {
    if w.fighter(owner).team.team != w.fighter(g).team.team || !in_hitstun(w, g) {
        return (vx, vy, false);
    }
    remove_hitstun_from_knock_back(w, g);
    let sq = (vx * vx) + (vy * vy);
    let size = vsize(vx, vy);
    let k = size * 0.3;
    if !(sq > 1e-4) {
        return (0.0, 0.0, true);
    }
    (((vx / size) * k), ((vy / size) * k), true)
}

/// AddPortalBuff(Fighter, EnterPortal) (function, perk): SpawnBuffActor(Buff_PortalBuff, Instigator the portal's
/// owner, MeleeBuffDuration, BreakIfBlocked true).
fn add_portal_buff(w: &mut World, owner: FighterId, g: FighterId, enter_owner: FighterId) {
    let dur = pnum(w, owner, "MeleeBuffDuration");
    spawn_buff(w, g, name!("Buff_PortalBuff_C"), dur, Some(0), Some(enter_owner), true, None);
}

/// Teleport FX Calls(TeleportingActor, EnterPortal, ExitPortalLocation) (function).
fn fx_calls(w: &mut World, owner: FighterId, a: ActorRef, ex: Real, ey: Real) {
    let oid = w.fighter(owner).id;
    let ev = if matches!(a, ActorRef::Fighter(_)) { SFX_WARP_FIGHTER } else { SFX_WARP_GENERIC };
    w.events.push(Event::Sound { fighter: oid, ev, x: None, y: None, hit: None, pooled: false, key: None });
    if let Some(c) = comp_mut(w, owner) {
        c.vfx_x = ex;
        c.vfx_y = ey;
        if c.exit_left == 0.0 {
            c.exit_left = latent_frames(0.5);
        }
    }
    if let ActorRef::Fighter(g) = a {
        let gid = w.fighter(g).id;
        w.events.push(Event::Sound {
            fighter: gid,
            ev: SFX_FAIL,
            x: None,
            y: None,
            hit: None,
            pooled: false,
            key: None,
        });
    } else {
        let (x, y) = actor_place(w, a).map_or((0.0, 0.0), |(x, y, _)| (x, y));
        w.events.push(Event::Sound {
            fighter: oid,
            ev: SFX_FAIL,
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: None,
        });
    }
}

/// TeleportFighter(Fighter, CalculatedVelocity, EnterPortal, ExitPortal, PendingExitPortalLocation) (function):
/// Success.
#[allow(clippy::too_many_arguments, reason = "the function's parameters")]
fn teleport_fighter(
    w: &mut World,
    owner: FighterId,
    g: FighterId,
    v: (Real, Real),
    enter: ActorRef,
    exit: Option<ActorRef>,
    pending: (Real, Real),
) -> bool {
    let ally = w.fighter(owner).team.team == w.fighter(g).team.team;
    if is_valid(w, exit)
        && let Some(e) = exit
    {
        add_teleported_actor(w, e, ActorRef::Fighter(g)); // @471 (its attached actors not walked)
    }
    let hitstun = in_hitstun(w, g); // CheckHitstunActive @1057
    let Some(b) = spawn_buff(w, g, name!("Buff_Teleporting3_C"), 0.0, Some(0), Some(owner), false, None) else {
        return false; // @1173
    };
    let (dx, dy) = if is_valid(w, exit) { portal_loc(w, exit) } else { pending }; // @1397
    let (lx, ly) = portal_loc(w, Some(enter));
    let dist = vsize(lx - dx, ly - dy);
    let t = min(max(dist / pnum(w, owner, "LongTeleportDistance"), 0.0), 1.0);
    let short = pnum(w, owner, "FighterTeleportDurationShort");
    let long = pnum(w, owner, "FighterTeleportDurationLong");
    let time = short + (t * (long - short));
    activate_teleport(w, g, b, (lx, ly), (dx, dy), time, Some(enter), exit, v, !hitstun || ally, !ally);
    true
}

/// TeleportActor(TeleportingActor, ExitPortal, PendingExitPortalLocation) (function).
fn teleport_actor(w: &mut World, a: ActorRef, exit: Option<ActorRef>, pending: (Real, Real)) {
    if is_valid(w, exit)
        && let Some(e) = exit
    {
        add_teleported_actor(w, e, a); // @48 (attached actors not walked)
        let (x, y) = portal_loc(w, exit);
        k2_teleport_to(w, a, x, y); // @686
    } else {
        k2_teleport_to(w, a, pending.0, pending.1); // @543
    }
}

/// SpawnObject(SpawnLocation, Facing, PortalClass) @2506 on the owner's component (portal.rs; Facing is not read).
/// Called by aimer.rs.
pub fn spawn_object(w: &mut World, owner: FighterId, x: Real, y: Real, cls: Name) {
    if comp(w, owner).is_none() {
        warn_text(w, "Comp_Pawn_Portal: SpawnObject on a pawn without the component");
        return;
    }
    let max_p = pnum(w, owner, "MaxPortals");
    let len = comp(w, owner).map_or(0, |c| c.allied.len());
    if crate::real(i32::try_from(len).expect("a few portals")) + 1.0 > max_p {
        let mut i = 0;
        loop {
            let allied = comp(w, owner).map(|c| c.allied.clone()).unwrap_or_default();
            if i >= allied.len() {
                break;
            }
            let n = crate::real(i32::try_from(allied.len()).expect("a few portals"));
            if !(crate::real(i32::try_from(i).expect("a few portals")) < n + 1.0 - max_p) {
                break;
            }
            let q = allied[i];
            i += 1;
            if !is_valid(w, Some(q)) {
                continue; // a call on a pending-kill actor is skipped
            }
            if pflag(w, owner, "ImmediatelyDestroyExtraPortals") {
                destroy_actor(w, q);
            } else if let Some(id) = portal_id(w, q)
                && !portal_transitioning_out(w, id)
            {
                transition_out(w, q);
            }
        }
    }
    let half = crate::fighter::body::def(w, owner).inner_cap_half;
    let at_y = y + crate::math::narrow(half - 100.0);
    let Some(state) = new_portal(w, owner, cls, x, at_y) else {
        warn_text(w, &format!("spawned actor not implemented ({})", w.names.str(cls)));
        return;
    };
    let Some(actor) = portal_actor(w, owner, cls, state) else {
        warn_text(w, &format!("spawned actor not implemented ({})", w.names.str(cls)));
        return;
    };
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, owner);
    comp_spawn_actor(w, id, charge, owner);
}

fn portal_transitioning_out(w: &World, id: crate::actors::ActorId) -> bool {
    match &w.actors.actors.get(id).expect("a listed actor").state {
        crate::actors::WorldActorState::PortalActor(p) => p.transitioning_out,
        _ => false,
    }
}

/// TeleportServer(EnterPortal, ActorToTeleport) -> HandleTeleport (see the head). Called by portal.rs.
pub fn teleport_server(w: &mut World, enter: ActorRef, actor: ActorRef) {
    let Some(eid) = portal_id(w, enter) else { return };
    let owner = w.actors.actors.get(eid).expect("a listed actor").owner;
    if comp(w, owner).is_none() {
        warn_text(w, "Comp_Pawn_Portal: TeleportServer on a pawn without the component");
        return;
    }
    let mut exit: Option<ActorRef> = None;
    let mut pending = (0.0, 0.0); // LocalNewPortalLocation
    let n = comp(w, owner).map_or(0, |c| c.allied.len());
    if n > 1 {
        if pflag(w, owner, "OnlyTeleportAllies") && !check_if_ally(w, owner, actor) {
            return;
        }
        exit = set_nearest_portal(w, owner, actor, enter);
        if exit.is_none() {
            return;
        }
    } else {
        if !pflag(w, owner, "Do 1 Portal At A Time Approach") {
            return;
        }
        let left = facing(w, owner) < 0.0; // BPI_IsFacingLeft
        let off = params(w, owner).and_then(|p| obj(p, "PortalSpawnAtOwnerOffset"));
        let oy = off.and_then(|o| num(o, "Y")).unwrap_or(0.0);
        let oz = off.and_then(|o| num(o, "Z")).unwrap_or(0.0);
        let (ox, oay) = (w.fighter(owner).body.x, actor_y(w, owner));
        let rx = ox - (oy * if left { 1.0 } else { -1.0 });
        let ry = oay + oz;
        if is_c020(w, Some(owner)) {
            pending = bpi_get_safe_portal_location(w, owner, 175.0, 300.0, rx, ry, ox, oay);
        }
        spawn_object(w, owner, pending.0, pending.1, VISUAL_ONLY);
    }
    // @301
    let (ex, ey) = if is_valid(w, exit) { portal_loc(w, exit) } else { pending };
    if let ActorRef::Fighter(g) = actor {
        if is_valid(w, exit)
            && let Some(e) = exit
        {
            increment_fighters_teleported(w, e);
        }
        let (mut vx, mut vy) = direct_velocity(w, owner, g);
        if check_if_ally(w, owner, actor) {
            if buff_of(w, owner, MELEE_PERK).is_some() {
                add_portal_buff(w, owner, g, owner);
            }
            if buff_of(w, owner, CATCH_PERK).is_some() {
                (vx, vy, _) = catch_ally(w, owner, g, vx, vy);
            }
        } else if !in_hitstun(w, g) {
            set_most_recent_attacker(w, g, owner); // SetMostRecentAttacker (function)
        }
        fx_calls(w, owner, actor, ex, ey);
        if !pflag(w, owner, "UseTeleportBuff") {
            warn_text(w, "Comp_Pawn_Portal: UseTeleportBuff false (TeleportActor of a fighter) not ported");
            return;
        }
        if !teleport_fighter(w, owner, g, (vx, vy), enter, exit, pending) {
            return;
        }
        buff_signal(
            w,
            owner,
            BuffSignal::AfterActorTeleported,
            BuffSignalArg { teleported: Some(actor), ..BuffSignalArg::default() },
        ); // @1582
        add_teleported_actor(w, enter, actor);
        return;
    }
    if !is_spawned_actor_parent(w, actor) || !pflag(w, owner, "TeleportProjectiles") {
        return;
    }
    teleport_actor(w, actor, exit, pending);
    buff_signal(
        w,
        owner,
        BuffSignal::AfterActorTeleported,
        BuffSignalArg { teleported: Some(actor), ..BuffSignalArg::default() },
    ); // @2567
    fx_calls(w, owner, actor, ex, ey);
    add_teleported_actor(w, enter, actor);
    if is_valid(w, exit)
        && let Some(e) = exit
    {
        multicast_fighter_teleported_effects(w, e, false);
    }
}

/// Rick's world_tick part: the TeleportVisuals Delay's end @15.
fn world_tick(w: &mut World, f: FighterId) {
    let Some(c) = comp_mut(w, f) else { return };
    if c.exit_left == 0.0 {
        return;
    }
    c.exit_left -= 1.0;
    if c.exit_left > 0.0 {
        return;
    }
    let (x, y) = (c.vfx_x, c.vfx_y);
    let fid = w.fighter(f).id;
    w.events.push(Event::Sound {
        fighter: fid,
        ev: SFX_EXIT,
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// The hooks this file joins in Rick's CharHooks (`world_tick`).
pub fn hooks(h: &mut CharHooks) {
    h.world_tick = Some(world_tick);
}
