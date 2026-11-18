//! Buff_HeldByEnemyGarnet (a child of Buff_HeldByEnemy, `combat::holds`): the carry
//! (HoldKind before_begin for AttachSocket, the instigator-hitstun destroy @98-513, BPI_SetIsHoldingEnemy @934, the
//! stun-end binding @1132-1206), SetIsHoldingEnemy @12061-12230, ThrowFighters @11876 -> @5477-6279, ThrowFighter,
//! AnimNotify_GarnetThrowFighters (Begin or End by DoAtEnd). No spawned class. Its state: the shared IsHoldingEnemy
//! (BaseFighter's, `holds.holding_enemy`).

use super::state::garnet_of;
use crate::Real;
use crate::combat::BuffClass;
use crate::combat::buffs::{BuffRef, end_buff};
use crate::combat::holds::{Hold, HoldHit, HoldKind, end_hold_buff};
use crate::events::NotifyRef;
use crate::fighter::common::peers;
use crate::fighter::{FighterId, in_hitstun};
use crate::name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::{flag, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// BPI_SetIsHoldingEnemy(True) @12061-12230, Pawn_Garnet's override: IsHoldingEnemy (BaseFighter's own variable, which
/// FighterHoldingEnemyCondition_C reads) = max(HeldEnemies + (True ? 1 : -1), 0) > 0. HeldEnemies is never written (CDO
/// 0), so it is True itself.
pub fn set_is_holding_enemy_body(w: &mut World, f: FighterId, holding: bool) {
    w.fighter_mut(f).holds.holding_enemy = holding;
}

/// The instigator's BPI_SetIsHoldingEnemy: Garnet's override above, else BaseFighter's (@100761: IsHoldingEnemy = True).
fn set_holding(w: &mut World, by: FighterId, on: bool) {
    if garnet_of(w, by).is_some() {
        set_is_holding_enemy_body(w, by, on);
    } else {
        w.fighter_mut(by).holds.holding_enemy = on;
    }
}

/// ReceiveBeginPlay, ahead of the parent's: the owner a BP_FighterCharacter, the owner not the instigator, the
/// instigator's HitstunHandler valid and its CheckHitstunActive false (@98-498); any failure -> K2_DestroyActor @513 and
/// nothing more of the BeginPlay (true: no hold is made). Else AttachSocket = the instigator facing left ? 'VB L_Hand' :
/// 'VB R_Hand' (@673-754), then the parent's BeginPlay (its L_ / R_ mirror at InitiateGrapple).
fn before_begin(w: &mut World, vic: FighterId, h: &mut Hold) -> bool {
    let by = h.by;
    if vic == by || in_hitstun(w, by) {
        return true;
    }
    h.socket = Some(if crate::fighter::facing(w, by) < 0.0 { name!("VB L_Hand") } else { name!("VB R_Hand") });
    false
}

/// ReceiveEndPlay: the parent's, then the owner's SetActorRotation((0, 0, yaw)) (the parent's @7027 set (0, 0, 0)
/// already: nothing to add; neither is ported, the port's fighter has no actor rotation off its facing), the
/// instigator's BPI_SetIsHoldingEnemy(false) @1612, the unbind.
fn end(w: &mut World, _vic: FighterId, h: &Hold) {
    set_holding(w, h.by, false);
}

/// Buff_HeldByEnemyGarnet's kind (`combat::holds::hold_kind`).
// Unverified: the refused path's destroy runs inside SpawnBuffActor ahead of its UpdatePlayerStatus, as the port's
// end_buff inside begin does; the parent's EndPlay with no InstigatorRef (its StatusUpdateMulticast) is the port's common
// end.
pub static KIND: HoldKind = HoldKind {
    before_begin: Some(before_begin),
    begin: None,
    tick: None,
    end: Some(end),
    track: None,
    warning: None,
    exited_arena: None,
    latent: None,
};

/// The buff entry's begin: the instigator's BPI_SetIsHoldingEnemy(true) @934 and the owner's HitstunHandler
/// OnBuffStunEnded bound @1132-1206 (`on`). A refused hold (before_begin, or one the port refuses: a second hold, no
/// instigator) is destroyed.
fn buff_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(h) = &w.fighter(f).holds.hold
        && h.cls == BuffClass::BuffHeldByEnemyGarnet
    {
        let by = h.by;
        set_holding(w, by, true);
        return;
    }
    if let Some(from) = from {
        set_holding(w, from, false);
    }
    end_buff(w, f, b);
}

/// OnBuffStunEnded @1264: K2_DestroyActor (the parent's own Escaped runs ahead of it in buff_stun_end).
fn buff_on(_w: &mut World, _f: FighterId, _b: BuffRef, sig: BuffSignal, _arg: &BuffSignalArg) -> bool {
    sig == BuffSignal::BuffStunEnded
}

/// ThrowFighters(HitStruct, Drop) @11876 (authority) -> @5477-6279: a counted loop over GetAttachedActors(out, true), the
/// array fetched again for the length test and each read; an entry that is a BP_FighterCharacter and a BPI_BaseFighter
/// with BPI_CheckIfHasBuff(Buff_HeldByEnemy_C) (a child counts): Drop -> that buff's K2_DestroyActor @6152; else
/// ThrowFighter(HitStruct, the entry, the buff) @6279. The index goes on by one after a destroy, which detaches the
/// fighter inside the call, so the attached actor after a thrown or dropped one is skipped (with two held, the second
/// stays held).
// Unverified: GetAttachedActors' order is taken as the port's fighters in World order (`consume_attached`), and only
// fighters are counted: an actor of another fighter attached to her (Arya's knife, Harley's bomb) would take an index
// of the build's array.
pub fn throw_fighters_body(w: &mut World, f: FighterId, hit: &Props, drop: bool) {
    let mut i = 0;
    loop {
        let victim = peers(w)
            .iter()
            .copied()
            .filter(|&p| p != f && w.fighter(p).holds.hold.as_ref().is_some_and(|h| h.attached && h.by == f))
            .nth(i);
        let Some(v) = victim else { return };
        if drop {
            end_hold_buff(w, v);
        } else {
            throw_fighter(w, f, v, hit);
        }
        i += 1;
    }
}

/// ThrowFighter(HitStruct, HitActor, HeldBuff): HeldBuff.K2_DestroyActor @46; KnockBackDirectionVector = (Y, Z) of
/// VLerp(Normal(LocationBeforeMove - her location, 1e-4), (0, 0, 1), 0.4) @82-345, IgnoreFacing = true @381;
/// pfgDoDamage(null, HitActor, the struct) @401 (`holds.hold_hits`: the World deals it after the frame's ticks,
/// unverified; the vector's X is UE Y, read as -sim x with IgnoreFacing).
fn throw_fighter(w: &mut World, f: FighterId, vic: FighterId, hit: &Props) {
    end_hold_buff(w, vic);
    let (lx, ly) = garnet_of(w, f).map_or((0.0, 0.0), |g| (g.location_before_move_x, g.location_before_move_y));
    let ue_y = -(lx - w.fighter(f).body.x);
    let ue_z = ly - crate::fighter::actor_y(w, f);
    let n = crate::math::ue::normal([0.0, ue_y, ue_z], crate::math::ue::KINDA_SMALL_NUMBER);
    let (ny, nz) = (n[1], n[2]);
    let x = ny * 0.6;
    let y = nz * 0.6 + 0.4;
    let mut p = hit.clone();
    let mut d = Props::new();
    d.insert("X".to_owned(), Value::Num(f64::from(x)));
    d.insert("Y".to_owned(), Value::Num(f64::from(y)));
    p.insert("KnockBackDirectionVector".to_owned(), Value::Obj(d));
    p.insert("IgnoreFacing".to_owned(), Value::Bool(true));
    w.fighter_mut(f).holds.hold_hits.push(HoldHit { vic, p });
}

/// AnimNotify_GarnetThrowFighters (state): Begin when !DoAtEnd, End when DoAtEnd; with authority, on a BPI_Garnet owner:
/// BPI_ThrowFighters(ThrowHitStruct, Drop). M4's _C_0 (Drop + DoAtEnd over the whole montage: the cleanup at its end) and
/// _C_1 at f120.142 (the throw: Damage 7, BKB 1500, scaling 22).
fn throw_notify(w: &mut World, f: FighterId, n: NotifyRef, at_end: bool) {
    let p = crate::fighter::match_side::notify_props(w, f, n);
    if flag(p, "DoAtEnd") != at_end || garnet_of(w, f).is_none() {
        return;
    }
    let (hit, drop) = (obj(p, "ThrowHitStruct").cloned().unwrap_or_default(), flag(p, "Drop"));
    throw_fighters_body(w, f, &hit, drop);
}

fn throw_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    throw_notify(w, f, n, false);
}

fn throw_end(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    throw_notify(w, f, n, true);
}

/// This file's registry entries (Buff_HeldByEnemyGarnet_C's buff entry, AnimNotify_GarnetThrowFighters_C). The hold's
/// class is in the holds' list of `combat::holds`.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        "Buff_HeldByEnemyGarnet_C",
        BuffEntry::new(
            BuffClass::BuffHeldByEnemyGarnet,
            BuffHooks { begin: Some(buff_begin), on: Some(buff_on), ..BuffHooks::default() },
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_GarnetThrowFighters_C",
        NotifyHandler { begin: Some(throw_begin), end: Some(throw_end), ..NotifyHandler::default() },
    )
}
