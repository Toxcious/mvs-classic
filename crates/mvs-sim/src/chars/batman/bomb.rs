//! Batman's bomb (the ON_HIT buff of the batarang): Buff_BatmanOnHitApplyBuff_C,
//! the bomb he carries until his next damaging hit or batarang throw, and Buff_DelayKnockUp_C, the bomb on the fighter
//! it went to. Offsets are each class's ubergraph's.
//!
//! Buff_BatmanOnHitApplyBuff: BeginPlay @517 (authority, the owner's Comp_Spawner) EquipObject(SatchelCharge);
//! ReceiveDestroyed @788 UnequipObject(SatchelCharge). DealtDamageModCondition: Active = Damage > 0 && BaseKnockBack >
//! 0 @188, the parent's answer. BPI On Fighter Owner Hit Enemy Player @803: Active -> DoOnce @15 -> the Instigator's
//! SpawnBuffActor(Buff_DelayKnockUp_C, HitPlayer, Instigator, 0 stacks, 0.0 s) @132 and K2_DestroyActor @216. The
//! Instigator is the owner (Batman's own ability). A batarang thrown while he has it takes it instead
//! (AttachBatarangBomb).
//!
//! Buff_DelayKnockUp: class defaults Delay 3, BuffDuration 3, IsDebuff, MaxStacks 1, DestroyBuffWhenOwnerDefeated;
//! HitBoxDamage Damage 2, BaseKnockBack 1500, KnockBackScaling 22, direction (0, 1), IgnoreFacing, HitReaction 4,
//! HitPauseTime 0.075, ForceKnockback.
//! - ReceiveBeginPlay @611: StartTime, (authority) ReveresedDirection = the Instigator's Buff_BatmanBombDownSpike
//!   @806-1006, TriggerVisuals @1302 (sfx_cha_batman_bomb_attach at the owner, sfx_cha_batman_bomb_timer_start on the
//!   buff, PS_BatmanBombAttach at the owner), TriggerArrowVisuals @2949 (drawing), then K2_SetTimerDelegate(KnockUp,
//!   Delay) @1085 and OnExitedArena bound to the owner's.
//! - KnockUp @2180: the buff's own Comp_Pawn_HitBoxHandler.pfgDoDamage(null, the owner, ReveresedDirection ?
//!   ReversedDirectionHitBoxDamage : HitBoxDamage) @2200-2219, KnockUpVisuals @4414 (sfx_cha_batman_bomb_explode at
//!   the owner, sfx_cha_batman_bomb_timer_stop, PS_BatmanBombExplosion at the owner), DestroyBuff @2429,
//!   K2_DestroyActor.
//! - OnExitedArena @5295 (BuffSignal ExitedArena): DestroyBuff alone (the particles and the timer sound; the knock-up
//!   timer stays). The ring-out's defeat tail destroys the buff in the same call: KnockUp never fires.
//! - ReceiveTick -> UpdateBuff: the arrow's Color over the delay (drawing).
//!
//! The spawn with 0.0 s (Buff_BatmanOnHitApplyBuff) makes no life timer; the batarang's 5.0 s outlives the 3 s delay.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::buffs::{BuffExt, buff_of, inst_def_of, inst_view, spawn_buff};
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hits::{apply_projectile_hit, dealer};
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::events::{Event, SoundKey};
use crate::fighter::common::warn_once;
use crate::fighter::control::attack_dodged;
use crate::fighter::moves::V2;
use crate::fighter::{DodgeState, FighterId, equip_object, invulnerable, no_hit, unequip_object};
use crate::name;
use crate::names::Name;
use crate::registry::entries::BuffHooks;
use crate::registry::props::{buff, buff_mut};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;

/// Buff_DelayKnockUp's own variables.
#[derive(State, Debug, Default)]
pub struct BombState {
    /// The owner's tick at which KnockUp fires.
    pub at: Real,
    /// The Instigator (the buff handler's).
    pub by: Option<FighterId>,
    /// The timer sound stopped.
    pub stopped: bool,
    /// ReveresedDirection: the Instigator's BombDownSpike perk at the begin.
    pub rev: bool,
    /// The buff actor's Comp_Pawn_HitBoxHandler dodge variables.
    pub dodge: Option<DodgeState>,
}

fn key(w: &World, f: FighterId) -> SoundKey {
    SoundKey { tag: name!("batmanBomb:"), n: w.fighter(f).id }
}

/// The owner's (x, actor y): where the bomb's sounds and particles play.
fn at(w: &World, f: FighterId) -> (Real, Real) {
    (w.fighter(f).body.x, crate::fighter::actor_y(w, f))
}

fn bomb(w: &World, b: BuffRef) -> Option<&BombState> {
    match &buff(w, b)?.ext {
        BuffExt::DelayKnockUp(e) => Some(e),
        _ => None,
    }
}

fn bomb_mut(w: &mut World, b: BuffRef) -> Option<&mut BombState> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::DelayKnockUp(e) => Some(e),
        _ => None,
    }
}

fn particles(w: &World, f: FighterId, vfx: Name) -> Event {
    let (x, y) = at(w, f);
    Event::ProjectileDeath {
        cls: name!("Buff_DelayKnockUp_C"),
        fighter: w.fighter(f).id,
        vfx: Some(vfx),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    }
}

/// ReceiveBeginPlay @611. Set inside the spawn (the tick groups): the timer counts from the next timer pass.
fn knock_up_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    let delay = inst_def_of(w, b).and_then(|d| d.get("Delay").and_then(|v| v.as_f64())).unwrap_or(3.0);
    // @707-1006: GetInstigator cast to BaseFighter, its CheckHasBuff(Buff_BatmanBombDownSpike). The arrow
    // (TriggerArrowVisuals' argument) is the Up one either way (the argument is read by nothing).
    let rev = from.is_some_and(|g| buff_of(w, g, name!("Buff_BatmanBombDownSpike_C")).is_some());
    let e = BombState {
        at: crate::real(w.fighter(f).clock.tick) + timer_ticks(Some(crate::math::narrow(delay))),
        by: from,
        stopped: false,
        rev,
        dodge: None,
    };
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::DelayKnockUp(e);
    }
    let by = w.fighter(from.unwrap_or(f)).id;
    let (x, y) = at(w, f);
    let (fid, key) = (w.fighter(f).id, key(w, f));
    let attach = name!("sfx_cha_batman_bomb_attach");
    let start = Event::SoundStart { fighter: fid, ev: name!("sfx_cha_batman_bomb_timer_start"), key };
    let fx = particles(w, f, name!("PS_BatmanBombAttach"));
    let ev = &mut w.fighter_mut(f).events;
    ev.push(Event::Sound { fighter: by, ev: attach, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    ev.push(start);
    ev.push(fx);
}

/// KnockUp @2180 on its timer; true: DestroyBuff and K2_DestroyActor.
fn knock_up_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = bomb(w, b) else { return false };
    if crate::real(w.fighter(f).clock.tick) < e.at {
        return false;
    }
    let (by, rev) = (e.by, e.rev);
    match by {
        None => warn_once(w, name!("Buff_DelayKnockUp without an Instigator: no hit")),
        Some(_) => knock_up_hit(w, f, b, rev),
    }
    // KnockUpVisuals @4414
    let byid = w.fighter(by.unwrap_or(f)).id;
    let (x, y) = at(w, f);
    let (fid, key) = (w.fighter(f).id, key(w, f));
    let explode = name!("sfx_cha_batman_bomb_explode");
    w.events.push(Event::Sound {
        fighter: byid,
        ev: explode,
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    let stop = Some(name!("sfx_cha_batman_bomb_timer_stop"));
    w.events.push(Event::SoundStop { fighter: fid, key, fade: None, ev: stop, curve: None, cut: false });
    if let Some(e) = bomb_mut(w, b) {
        e.stopped = true;
    }
    let fx = particles(w, f, name!("PS_BatmanBombExplosion"));
    w.events.push(fx);
    true
}

/// The Select @2219 and pfgDoDamage(null, owner): the BombDownSpike's packet (ReversedDirectionHitBoxDamage) is all
/// zero, not the downward hit the perk names. The dodge branch for an invulnerable owner; no hit on a fighter out of
/// play; else the owner's DamageAndLaunch with the buff actor as the dealer (HitLocation of a null HitCollision: the
/// victim's capsule). The dodge branch shows no DODGED text: the buff's handler has no hit box over the fighter for
// its tick.
fn knock_up_hit(w: &mut World, f: FighterId, b: BuffRef, rev: bool) {
    let k = if rev { "ReversedDirectionHitBoxDamage" } else { "HitBoxDamage" };
    let h = inst_view(w, b).obj(k).map_or_else(HitPacket::default, |p| HitPacket::from_props(&w.names, p, None));
    let attack_id = h.attack_id.unwrap_or(0);
    if invulnerable(w, f) {
        // AttackingActor: the buff actor (its handler's owner).
        let tick = w.flow.tick;
        attack_dodged(w, ActorRef::Buff(b), f, attack_id, h.base_knock_back.unwrap_or(0.0), tick, None, false);
    } else if w.fighter(f).life.dead <= 0.0 && !no_hit(w, f) {
        let mut d = dealer(w, ActorRef::Buff(b));
        d.attack_id = attack_id;
        let (x, y) = at(w, f);
        apply_projectile_hit(w, &d, f, &h, &[], V2 { x, y }, None, &[], 0.0);
    }
}

/// OnExitedArena @5295 -> DestroyBuff @2429: K2_DestroyComponent on each Particles entry, then
/// PF_Post_Event(sfx_cha_batman_bomb_timer_stop). The actor and its timer stay.
fn knock_up_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    if s != BuffSignal::ExitedArena {
        return false;
    }
    if let Some(e) = bomb_mut(w, b) {
        e.stopped = true;
    }
    let (fid, key) = (w.fighter(f).id, key(w, f));
    let stop = Some(name!("sfx_cha_batman_bomb_timer_stop"));
    w.fighter_mut(f).events.push(Event::SoundStop { fighter: fid, key, fade: None, ev: stop, curve: None, cut: false });
    false
}

/// Destroyed another way (a cleanse; the owner's defeat after the exit above): ReceiveDestroyed posts no stop event;
/// the sound goes with the buff's AkComponent (every PF_Post_Event sets StopWhenOwnerDestroyed and OnUnregister calls
/// StopAll).
// Unverified: StopAll taken as a stop without a fade.
fn knock_up_end(w: &mut World, f: FighterId, b: BuffRef) {
    match bomb_mut(w, b) {
        Some(e) if !e.stopped => e.stopped = true,
        _ => return,
    }
    let (fid, key) = (w.fighter(f).id, key(w, f));
    w.fighter_mut(f).events.push(Event::SoundStop {
        fighter: fid,
        key,
        fade: Some(0.0),
        ev: None,
        curve: None,
        cut: false,
    });
}

/// Buff_BatmanOnHitApplyBuff BeginPlay @517.
fn on_hit_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _duration: Option<Real>) {
    equip_object(w, f, name!("SatchelCharge"));
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::BatmanOnHit { active: false };
    }
}

/// ReceiveDestroyed @788.
fn on_hit_end(w: &mut World, f: FighterId, _b: BuffRef) {
    unequip_object(w, f, name!("SatchelCharge"));
}

/// DealtDamageModCondition @188: Active = Damage > 0 && BaseKnockBack > 0; the parent's answer (true).
// Unverified: Active is the last of the three dealt passes of the hit (as Buff_PassiveApplyBuffonHit).
fn on_hit_dealt(
    w: &mut World,
    _victim: ActorRef,
    _owner: FighterId,
    _dealer: FighterId,
    hit: &HitPacket,
    b: BuffRef,
    _first: bool,
) -> bool {
    let on = hit.damage.unwrap_or(0.0) > 0.0 && hit.base_knock_back.unwrap_or(0.0) > 0.0;
    if let Some(x) = buff_mut(w, b)
        && let BuffExt::BatmanOnHit { active } = &mut x.ext
    {
        *active = on;
    }
    true
}

/// BPI On Fighter Owner Hit Enemy Player @803: Active -> DoOnce -> the knock-up on the victim, K2_DestroyActor (true).
fn on_hit_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let (BuffSignal::HitEnemy, Some(victim)) = (s, a.victim) else { return false };
    let active = matches!(buff(w, b).map(|x| &x.ext), Some(BuffExt::BatmanOnHit { active: true }));
    if !active {
        return false;
    }
    spawn_buff(w, victim, name!("Buff_DelayKnockUp_C"), 0.0, Some(0), Some(f), true, None);
    true
}

/// The two buff entries.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let on_hit = BuffHooks {
        begin: Some(on_hit_begin),
        end: Some(on_hit_end),
        dealt_cond: Some(on_hit_dealt),
        on: Some(on_hit_on),
        ..BuffHooks::default()
    };
    r.buffs.enter("Buff_BatmanOnHitApplyBuff_C", BuffEntry::new(BuffClass::BuffBatmanOnHitApplyBuff, on_hit))?;
    let knock_up = BuffHooks {
        begin: Some(knock_up_begin),
        timer: Some(knock_up_timer),
        on: Some(knock_up_on),
        end: Some(knock_up_end),
        ..BuffHooks::default()
    };
    r.buffs.enter("Buff_DelayKnockUp_C", BuffEntry::new(BuffClass::BuffDelayKnockUp, knock_up))
}
