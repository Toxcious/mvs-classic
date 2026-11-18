//! Rick's teleport buffs: Buff_Teleporting3 ([`TeleportExt`]) and
//! Buff_PortalBuff ([`PortalBuffExt`]), and Pawn_C020's BPI_GetSafePortalLocation, which both the buff's EndPlay and
//! Comp_Pawn_Portal's one-portal path call. Offsets are of the class's ubergraph unless a function is named. The
//! component itself (Comp_Pawn_Portal) is portal_comp.rs. Sim space: x = -UE Y, y = UE Z; the UE X (depth) of the
//! graphs' vectors is the view's alone.
//!
//! Buff_Teleporting3 (class data: StateInfo Group 13, BlockerGroups [Held], BlockerBuffs [Buff_Tunneling], BreakGroups
//! [13]; Stun 0 with IsGrappledAlly and StunnedHitReaction 6; TimeTillDestination 0.5, TeleportDelay 0.1, ExitTime 0.1,
//! FighterVisible true, CanOverride; the three timelines of length 1 with linear 0 -> 1 tracks, the DelayTimeline's X
//! track CRV_Exp, depth alone). Spawned by Comp_Pawn_Portal TeleportFighter @1173 with BreakIfBlocked false, then
//! Activate @1842 on the new actor in the same call.
//! - ReceiveBeginPlay @2916 (owner a BP_Fighter_Character, authority): SetCollisions(false) @3051 (the fighter's
//!   SetActorEnableCollision(false): `actor_collision_off`, the whole fighter's collision off while the entry lives);
//!   the attached actors' loop @3075-4695 (Deviation teleport c); SetMovementMode(3 Falling) @4700,
//!   StopMovementImmediately @4762.
//! - Activate @1855: StartingLocation = Start + StartingPointOffset (-400, 0, 0: depth), Destination,
//!   TimeTillDestination, the two portals, VelocityBeforeTeleport, ShouldClampVelocity, Moving = true, Knockback at End;
//!   PlayTimeLine @1700 with DelayPlayRate 1 / TeleportDelay and MainPlayRate 1 / (TimeTillDestination - ExitTime)
//!   (@1140: the ExitTimeline's 1 / ExitTime), DelayTimeline.PlayFromStart @1359; Delay(TimeTillDestination +
//!   TeleportDelay) @1796 -> @15.
//! - DelayTimeline update @5583: the location lerped from where it is toward StartingLocation. Finished @8048 -> @7769:
//!   SetFighterVisibility(false) @7864; @6675 Stun.StunDuration = TimeTillDestination + TeleportDelay @6755,
//!   UpdatePlayerStatus(self) @6972; MainTimeline.PlayFromStart @5057.
//! - MainTimeline update @7529: VLerp(StartingLocation, Destination, Alpha). Finished @8053: SetFighterVisibility(true)
//!   @8148, ExitTimeline.PlayFromStart @6642.
//! - ExitTimeline update @6028: VLerp(the location, Destination, Delta). Finished @5476: the EndingPortal valid ->
//!   MulticastFighterTeleportedEffects(false) @5545.
//! - The Delay's end @15: K2_SetActorLocation(Destination); ShouldClampVelocity ? LaunchCharacter((0, 0, 500), true,
//!   true) @91 : LaunchCharacter(VelocityBeforeTeleport, true, true) @322; Knockback at End -> the EndingPortal, else
//!   the StartingPortal, if valid, KnockbackFighterOnExit(owner, VelocityBeforeTeleport) @253 / @437; K2_DestroyActor @307.
//! - ReceiveEndPlay @2124, after BuffParentActor's (its stun pick, made here first): once (TeleportingEnded @2260):
//!   StopTimelines; the Instigator a BPI_C020 -> BPI_GetSafePortalLocation(the root capsule's radius and half height,
//!   the location, StartingLocation) @2673, else (0, 0, 0) @2880; K2_SetActorLocation(that) @2755; SetCollisions(true)
//!   @2817; !Knockback at End -> Stun.StunDuration = 0 @951 and UpdatePlayerStatus(null) @983; SetFighterVisibility(true) @926.
#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(a < b)` is true for NaN")]
// Unverified (teleport a): a timeline started from another's Finished (Main from Delay's, Exit from Main's) takes its
// first advance in the next frame; the DelayTimeline's first advance is in Activate's frame.
// Unverified (teleport b): on the frame the ExitTimeline finishes and the Delay ends, the timeline runs first (its
// Finished's multicast ahead of the Delay's knockback and destroy).
// Deviation (teleport c): GetAttachedActors is not walked (BeginPlay's detach / collision loop, SetFighterVisibility's
// attached actors, TeleportFighter's AddTeleportedActor of them): no ported actor attaches to a fighter that could
// stand in a portal (a held fighter is the holder's own buff state, and Buff_Teleporting3 is blocked while Held).
// Every hook takes the World.
// Buff_PortalBuff (perk only: AddPortalBuff, with Buff_C020PortalMeleeBuff on Rick, which no data of the build gives;
// BuffDuration 6, CooldownTime 0.2): ProcBuff @1191: SpawnBuffActor(Buff_Fire, the hit player, Instigator the owner, 1
// stack, 0, true) @1338; none -> K2_DestroyActor @1545; else Delay(CooldownTime) @1132 (a second call while it runs:
// ignored) -> @15 K2_DestroyActor.
// Unverified (teleport e): the Delay armed inside a hit counts from the buff's next tick.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{
    BuffExt, BuffRef, buff, buff_mut, inst_view, recalc_ability_mods, recalc_charge_mod, recalc_head_size,
    recalc_size_mod, recalc_speed_mod, set_buff_values, spawn_buff, update_states,
};
use crate::combat::holds::apply_stun_buffs;
use crate::consts::DELTA_SECONDS;
use crate::fighter::anim::{add_invisibility, remove_invisibility};
use crate::fighter::common::{latent_frames, warn_text};
use crate::fighter::moves::Impulse;
use crate::fighter::moves::recall_move;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_PORTAL, OBJ_WORLD_STATIC, TraceHit, capsule_trace_multi_for_objects,
};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, set_grounded};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

use super::portal::{knockback_fighter_on_exit, multicast_fighter_teleported_effects, portal_id, portal_valid};

/// Buff_Teleporting3's class.
pub const TELEPORTING: &str = "Buff_Teleporting3_C";

/// Buff_PortalBuff's class.
pub const PORTAL_BUFF: &str = "Buff_PortalBuff_C";

const TL_NONE: i32 = 0;
const TL_DELAY: i32 = 1;
const TL_MAIN: i32 = 2;
const TL_EXIT: i32 = 3;

/// Buff_Teleporting3_C's extension (`TeleportExt`). What the view reads: `tl` (the playing timeline), sx / sy and dx /
/// dy (the beam's ends), `visible`.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct TeleportExt {
    /// The Instigator (Rick).
    pub by: Option<FighterId>,
    /// StartingLocation (YZ).
    pub sx: Real,
    pub sy: Real,
    /// Destination.
    pub dx: Real,
    pub dy: Real,
    /// TimeTillDestination.
    pub time: Real,
    /// StartingPortal.
    pub start: Option<ActorRef>,
    /// EndingPortal.
    pub end: Option<ActorRef>,
    /// VelocityBeforeTeleport.
    pub vx: Real,
    pub vy: Real,
    /// ShouldClampVelocity.
    pub clamp: bool,
    /// Knockback at End.
    pub kb: bool,
    /// Moving.
    pub moving: bool,
    /// The playing timeline (TL_*).
    pub tl: i32,
    /// Its position (length 1).
    pub alpha: Real,
    /// The play rates.
    pub r_delay: Real,
    pub r_main: Real,
    pub r_exit: Real,
    /// The Delay(TimeTillDestination + TeleportDelay)'s frames left (0: not armed).
    pub left: Real,
    /// FighterVisible.
    pub visible: bool,
    /// TeleportingEnded.
    pub ended: bool,
}

/// Buff_PortalBuff_C's extension (`{ left }`).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct PortalBuffExt {
    /// The Delay(CooldownTime)'s frames left (0: not armed).
    pub left: Real,
}

/// `vsize`: VSize of a play-plane vector, float32 at each step.
pub fn vsize(x: Real, y: Real) -> Real {
    ((x * x) + (y * y)).sqrt()
}

/// `isC020`: the fighter is a Rick.
pub fn is_c020(w: &World, f: Option<FighterId>) -> bool {
    f.is_some_and(|f| matches!(w.fighter(f).cs, CharState::Rick(_)))
}

/// `isValid`: a portal not dead, or in its EndPlay. Called by portal_comp.rs.
pub fn is_valid(w: &World, p: Option<ActorRef>) -> bool {
    p.and_then(|p| portal_id(w, p)).is_some_and(|id| portal_valid(w, id))
}

/// `portalLoc`: a portal's location, (0, 0) for a pending-kill one. Called by portal_comp.rs.
pub fn portal_loc(w: &World, p: Option<ActorRef>) -> (Real, Real) {
    match p.and_then(|p| portal_id(w, p)) {
        Some(id) if portal_valid(w, id) => match &w.actors.actors.get(id).expect("a listed actor").state {
            crate::actors::WorldActorState::PortalActor(s) => (s.x, s.y),
            _ => (0.0, 0.0),
        },
        _ => (0.0, 0.0),
    }
}

// ------------------------------------------------------------------ BPI_GetSafePortalLocation (Pawn_C020 @0-3382)

/// `bpiGetSafePortalLocation` (BPI_GetSafePortalLocation(Radius, HalfHeight, OriginalDestination, InstigatorLocation)
/// on Rick `f` (the traces' bIgnoreSelf actor), each vector flattened to X 0 (@141-362). Every trace is a
/// CapsuleTrace*ForObjects of [WorldStatic, DamageDestructible, Portal]; an object query's hits all block, so a Single
/// is the first of the sorted hits. Called by aimer.rs and portal_comp.rs.
/// - The capsule at the destination (a zero-length sweep @389) meets nothing: the destination.
/// - Else D = clamp(3 HalfHeight, 200, 400) and four sweeps toward the destination, from above (+Z), below, UE -Y (sim
///   +x) and UE +Y (sim -x) (@708, @1756, @2042, @2328); each candidate @989: the hit's Location (bBlockingHit), else
///   TraceEnd (the destination); kept when the capsule (Radius - 1, HalfHeight - 1) is clear there and it is strictly
///   nearer the destination than the last kept one.
/// - None kept: a multi sweep from the instigator's location to the destination @2768, its hits tested in order until
///   one is kept (@2986-3377); none: the instigator's location.
// Unverified (safe a): a no-hit OutHit's TraceEnd is taken as the sweep's End (stock FHitResult init in the sweep), so
// a side sweep that meets nothing tests the destination itself with the smaller capsule.
// Takes `&mut World`: the trace queries reuse the world's scratch.
#[allow(clippy::too_many_arguments, reason = "the BPI's parameters")]
pub fn bpi_get_safe_portal_location(
    w: &mut World,
    f: FighterId,
    r: Real,
    hh: Real,
    dest_x: Real,
    dest_y: Real,
    inst_x: Real,
    inst_y: Real,
) -> (Real, Real) {
    let types = [crate::real(OBJ_WORLD_STATIC), crate::real(OBJ_DAMAGE_DESTRUCTIBLE), crate::real(OBJ_PORTAL)];
    let own = Some(ActorRef::Fighter(f));
    let (dx, dy) = (dest_x, dest_y);
    let mut hits: Vec<TraceHit> = Vec::new();
    let mut tmp: Vec<TraceHit> = Vec::new();
    capsule_trace_multi_for_objects(w, dx, dy, dx, dy, r, hh, &types, &[], own, &mut hits);
    if hits.is_empty() {
        return (dx, dy);
    }
    let d = min(max(hh * 3.0, 200.0), 400.0);
    let mut best = Best { closest: 7.7662794e18, x: inst_x, y: inst_y, found: false };
    let q = Query { r1: (r - 1.0), hh1: (hh - 1.0), dx, dy, own, types };
    let sides = [(dx, (dy + d)), (dx, (dy - d)), ((dx + d), dy), ((dx - d), dy)];
    for (sx, sy) in sides {
        capsule_trace_multi_for_objects(w, sx, sy, dx, dy, r, hh, &types, &[], own, &mut hits);
        let first = hits.first().copied();
        consider(w, &q, &mut best, first.as_ref(), &mut tmp);
    }
    if best.found {
        return (best.x, best.y);
    }
    capsule_trace_multi_for_objects(w, inst_x, inst_y, dx, dy, r, hh, &types, &[], own, &mut hits);
    let list = hits.clone();
    for h in &list {
        if best.found {
            break;
        }
        consider(w, &q, &mut best, Some(h), &mut tmp);
    }
    (best.x, best.y)
}

/// The kept candidate of the safe-location search.
struct Best {
    closest: Real,
    x: Real,
    y: Real,
    found: bool,
}

/// The search's fixed values.
struct Query {
    r1: Real,
    hh1: Real,
    dx: Real,
    dy: Real,
    own: Option<ActorRef>,
    types: [Real; 3],
}

/// The candidate @989: the hit's Location (bBlockingHit), else the destination; kept when the smaller capsule is clear
/// there and it is strictly nearer the destination than the last kept one.
fn consider(w: &mut World, q: &Query, best: &mut Best, h: Option<&TraceHit>, tmp: &mut Vec<TraceHit>) {
    let (cx, cy) = match h {
        Some(h) if h.blocking => (h.x, h.y),
        _ => (q.dx, q.dy),
    };
    capsule_trace_multi_for_objects(w, cx, cy, cx, cy, q.r1, q.hh1, &q.types, &[], q.own, tmp);
    if !tmp.is_empty() {
        return;
    }
    let dist = vsize(cx - q.dx, cy - q.dy);
    if !(dist < best.closest) {
        return;
    }
    *best = Best { closest: dist, x: cx, y: cy, found: true };
}

// ------------------------------------------------------------------ Buff_Teleporting3

fn ext_of(w: &World, b: BuffRef) -> Option<TeleportExt> {
    match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::Teleporting3(e)) => Some(*e),
        _ => None,
    }
}

fn put_ext(w: &mut World, b: BuffRef, e: TeleportExt) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Teleporting3(e);
    }
}

/// A number of the buff's values (a missing one warns and reads 0).
fn bnum(w: &World, b: BuffRef, k: &str, cls: &str) -> Real {
    match inst_view(w, b).num(k) {
        Some(v) => v,
        None => {
            warn_text(w, &format!("{cls} data missing: {k}"));
            0.0
        }
    }
}

/// SetFighterVisibility(Visible) (function @0): only on a change of FighterVisible.
fn set_visibility(w: &mut World, f: FighterId, e: &mut TeleportExt, visible: bool) {
    if visible == e.visible {
        return;
    }
    e.visible = visible;
    if visible {
        remove_invisibility(w, f);
    } else {
        add_invisibility(w, f);
    }
}

/// Comp_State_Manager.UpdatePlayerStatus(NewBuff): the speed, stun, ability, charge, size and head applications with
/// the buff as the new one (`spawn_buff`'s own call).
fn update_player_status(w: &mut World, f: FighterId, b: BuffRef) {
    recalc_speed_mod(w, f, Some(b));
    apply_stun_buffs(w, f, Some(b), false);
    recalc_ability_mods(w, f, Some(b));
    recalc_charge_mod(w, f, Some(b)); // ApplyChargeBuffs (@483)
    recalc_size_mod(w, f, Some(b));
    recalc_head_size(w, f, Some(b));
}

fn update(w: &mut World, f: FighterId, e: &TeleportExt) {
    let a = e.alpha;
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    if e.tl == TL_DELAY {
        // Lerp(the location, StartingLocation, YZ) per axis @5745-5910; Lerp(A, B, 0) is A: the same place.
        if a == 0.0 {
            return;
        }
        recall_move(w, f, x + (a * (e.sx - x)), y + (a * (e.sy - y)));
    } else if e.tl == TL_MAIN {
        recall_move(w, f, e.sx + (a * (e.dx - e.sx)), e.sy + (a * (e.dy - e.sy)));
    } else if e.tl == TL_EXIT {
        if a == 0.0 {
            return;
        }
        recall_move(w, f, x + (a * (e.dx - x)), y + (a * (e.dy - y)));
    }
}

/// PlayFromStart: the position 0 and its update.
fn play_from_start(w: &mut World, f: FighterId, e: &mut TeleportExt, tl: i32) {
    e.tl = tl;
    e.alpha = 0.0;
    update(w, f, e);
}

fn finished(w: &mut World, f: FighterId, b: BuffRef, e: &mut TeleportExt, tl: i32) {
    if tl == TL_DELAY {
        set_visibility(w, f, e, false);
        let tele_delay = bnum(w, b, "TeleportDelay", TELEPORTING);
        let mut stun = inst_view(w, b).obj("Stun").cloned().unwrap_or_default();
        stun.insert("StunDuration".into(), mvs_data::value::Value::Num(f64::from(e.time + tele_delay)));
        let mut v = mvs_data::value::Props::new();
        v.insert("Stun".into(), mvs_data::value::Value::Obj(stun));
        set_buff_values(w, b, &v);
        update_player_status(w, f, b);
        play_from_start(w, f, e, TL_MAIN);
    } else if tl == TL_MAIN {
        set_visibility(w, f, e, true);
        play_from_start(w, f, e, TL_EXIT);
    } else if tl == TL_EXIT
        && is_valid(w, e.end)
        && let Some(p) = e.end
    {
        multicast_fighter_teleported_effects(w, p, false);
    }
}

/// The playing timeline's tick: position += dt x rate, its update, then Finished at the end (FTimeline order).
fn advance(w: &mut World, f: FighterId, b: BuffRef, e: &mut TeleportExt) {
    if e.tl == TL_NONE {
        return;
    }
    let rate = if e.tl == TL_DELAY {
        e.r_delay
    } else if e.tl == TL_MAIN {
        e.r_main
    } else {
        e.r_exit
    };
    e.alpha += DELTA_SECONDS * rate;
    let done = e.alpha >= 1.0;
    if done {
        e.alpha = 1.0;
    }
    update(w, f, e);
    if !done {
        return;
    }
    let tl = e.tl;
    e.tl = TL_NONE;
    finished(w, f, b, e, tl);
}

/// Activate(Start, Destination, TimeTillDestination, StartingPortal, EndingPortal, Velocity, ShouldClampVelocity,
/// KnockbackAtEnd) @1855, from Comp_Pawn_Portal TeleportFighter @1842 (`from`: the enter portal's location).
/// Called by portal_comp.rs.
#[allow(clippy::too_many_arguments, reason = "the parameters")]
pub fn activate_teleport(
    w: &mut World,
    f: FighterId,
    b: BuffRef,
    from: (Real, Real),
    to: (Real, Real),
    time: Real,
    start: Option<ActorRef>,
    end: Option<ActorRef>,
    v: (Real, Real),
    clamp: bool,
    kb: bool,
) {
    let Some(mut e) = ext_of(w, b) else { return };
    e.sx = from.0;
    e.sy = from.1;
    e.dx = to.0;
    e.dy = to.1;
    e.time = time;
    e.start = start;
    e.end = end;
    e.vx = v.0;
    e.vy = v.1;
    e.clamp = clamp;
    e.moving = true;
    e.kb = kb;
    let delay = bnum(w, b, "TeleportDelay", TELEPORTING);
    let exit = bnum(w, b, "ExitTime", TELEPORTING);
    e.r_delay = 1.0 / delay;
    e.r_main = 1.0 / (e.time - exit);
    e.r_exit = 1.0 / exit;
    play_from_start(w, f, &mut e, TL_DELAY);
    advance(w, f, b, &mut e); // the first advance in this frame (Unverified teleport a)
    e.left = latent_frames(e.time + delay) - 1.0; // the Delay counts this frame
    put_ext(w, b, e);
}

fn teleporting_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    let visible = inst_view(w, b).get("FighterVisible").is_none_or(|v| v.as_bool() != Some(false));
    put_ext(w, b, TeleportExt { by: from, visible, ..TeleportExt::default() });
    // SetCollisions(false) @3051: actor_collision_off, while the entry lives. SetMovementMode(3 Falling) @4700,
    // StopMovementImmediately @4762.
    let fx = w.fighter_mut(f);
    fx.moves.flying = false;
    fx.hit.kb_mode = false;
    set_grounded(w, f, false);
    let body = &mut w.fighter_mut(f).body;
    body.vx = 0.0;
    body.vy = 0.0;
}

fn teleporting_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(mut e) = ext_of(w, b) else { return false };
    advance(w, f, b, &mut e); // the timeline ahead of the Delay (Unverified teleport b)
    let fire = e.left > 0.0 && {
        e.left -= 1.0;
        e.left == 0.0
    };
    put_ext(w, b, e);
    if !fire {
        return false;
    }
    // The Delay's end @15.
    recall_move(w, f, e.dx, e.dy);
    w.fighter_mut(f).moves.impulse = Some(if e.clamp {
        Impulse { vx: 0.0, vy: 500.0, xy: true, kind: 0, z: true, d: None }
    } else {
        Impulse { vx: e.vx, vy: e.vy, xy: true, kind: 0, z: true, d: None }
    });
    if e.kb {
        let p = if is_valid(w, e.end) {
            e.end
        } else if is_valid(w, e.start) {
            e.start
        } else {
            None
        };
        if let Some(p) = p {
            knockback_fighter_on_exit(w, p, f, e.vx, e.vy);
        }
    }
    true
}

fn teleporting_end(w: &mut World, f: FighterId, b: BuffRef) {
    let stun_set =
        inst_view(w, b).obj("Stun").and_then(|s| s.get("StunDuration")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    apply_stun_buffs(w, f, None, stun_set > 0.0); // BuffParentActor's EndPlay
    let Some(mut e) = ext_of(w, b) else { return };
    if e.ended {
        return;
    }
    e.ended = true;
    e.tl = TL_NONE; // StopTimelines
    e.left = 0.0;
    let (mut x, mut y) = (0.0, 0.0);
    if is_c020(w, e.by) {
        let by = e.by.expect("is_c020 holds");
        let (r, hh, fx, fy) = (cap_radius(w, f), capsule_half(w, f), w.fighter(f).body.x, actor_y(w, f));
        (x, y) = bpi_get_safe_portal_location(w, by, r, hh, fx, fy, e.sx, e.sy);
    }
    recall_move(w, f, x, y);
    // SetCollisions(true) @2817: the entry is no longer live (frames 0) when `end` runs.
    if !e.kb {
        let mut stun = inst_view(w, b).obj("Stun").cloned().unwrap_or_default();
        stun.insert("StunDuration".into(), mvs_data::value::Value::Num(0.0));
        let mut v = mvs_data::value::Props::new();
        v.insert("Stun".into(), mvs_data::value::Value::Obj(stun));
        set_buff_values(w, b, &v);
        update_states(w, f);
    }
    set_visibility(w, f, &mut e, true);
    put_ext(w, b, e);
}

fn teleporting_collision_off(_w: &World, _f: FighterId, _b: BuffRef) -> bool {
    true // BeginPlay @3051, EndPlay @2817
}

// ------------------------------------------------------------------ Buff_PortalBuff

fn portal_buff_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::PortalBuff(PortalBuffExt { left: 0.0 });
    }
}

fn portal_buff_on(w: &mut World, f: FighterId, b: BuffRef, sig: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(victim) = a.victim.filter(|_| sig == BuffSignal::HitEnemy) else { return false };
    if spawn_buff(w, victim, name!("Buff_Fire_C"), 0.0, Some(1), Some(f), true, None).is_none() {
        return true; // @1338, none -> K2_DestroyActor @1545
    }
    let Some(BuffExt::PortalBuff(e)) = buff(w, b).map(|x| &x.ext) else { return false };
    if e.left == 0.0 {
        let cd = bnum(w, b, "CooldownTime", PORTAL_BUFF);
        if let Some(BuffExt::PortalBuff(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
            e.left = latent_frames(cd); // Unverified teleport e
        }
    }
    false
}

fn portal_buff_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    let Some(BuffExt::PortalBuff(e)) = buff_mut(w, b).map(|x| &mut x.ext) else { return false };
    if e.left > 0.0 {
        e.left -= 1.0;
        return e.left == 0.0;
    }
    false
}

/// This file's registry entries (Buff_Teleporting3_C, Buff_PortalBuff_C), in registration order.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        TELEPORTING,
        BuffEntry::new(
            BuffClass::BuffTeleporting3,
            BuffHooks {
                begin: Some(teleporting_begin),
                tick: Some(teleporting_tick),
                end: Some(teleporting_end),
                actor_collision_off: Some(teleporting_collision_off),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.buffs.enter(
        PORTAL_BUFF,
        BuffEntry::new(
            BuffClass::BuffPortalBuff,
            BuffHooks {
                begin: Some(portal_buff_begin),
                on: Some(portal_buff_on),
                tick: Some(portal_buff_tick),
                ..BuffHooks::default()
            },
        ),
    )?;
    Ok(())
}
