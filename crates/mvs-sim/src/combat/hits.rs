//! The hits as they land: damage, knockback, hitstun, combo decay, the vacuums and the on-hit buffs,
//! Comp_HoldCharge and its notify, and pfgDoDamage's routing of a
//! spawned actor's hit on an actor that is no fighter.
//!
//! A fighter's hit box call is [`apply_hit`]; a spawned actor's on a fighter [`apply_projectile_hit`], over the
//! dealer's fields read once into a [`Dealer`].

use crate::Real;
use crate::actors::{ActorId, ActorRef, ItemId, ProjectileId};
use crate::combat::HitPacket;
use crate::combat::OnHitBuffRow;
use crate::combat::VacuumInfo;
use crate::combat::buffs as buff;
use crate::combat::decay::DecayHit;
use crate::combat::elem;
use crate::combat::hit as fh;
use crate::combat::packet::DataEnum;
use crate::combat::perks::PerkDealer;
use crate::consts as C;
use crate::events::{Event, NotifyRef, PopKind, SoundHit};
use crate::fighter::anim::OpenPose;
use crate::fighter::moves::V2;
use crate::fighter::{self, Fighter, FighterId};
use crate::math::ue;
use crate::math::{max, min, sign};
use crate::names::Name;
use crate::registry::ActorHooks;
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal, BuffSignal, BuffSignalArg, CondSignal};
use crate::world::World;
use mvs_data::CharData;
use mvs_data::fighter::{ActorDef, Notify};
use mvs_data::value::{Props, Value};

/// `DamageCall`: what pfgDoDamage has beside the dealer, the HitActor and the packet.
#[derive(Clone, Copy, Debug, Default)]
pub struct DamageCall<'a> {
    /// A HitBox component (false for a query dealer's pfgDoDamage(null, actor)).
    pub hit_box: bool,
    /// That component's name.
    pub comp: Option<Name>,
    /// IsClankParam (Handle Projectile Clank's reciprocal call).
    pub clank: bool,
    /// OnHitBuffsParam: the caller's own rows, which every read of pfgDoDamage takes. `None`: the handler's own
    /// (the victim-kind rule the routes have); `Some(&[])`: none.
    pub rows: Option<&'a [OnHitBuffRow]>,
}

// ---- the fighter's reads

/// The fighter's character data (its box tables, montages, components and CharDef).
pub(crate) fn char_data<'a>(w: &'a World, f: &Fighter) -> &'a CharData {
    &w.data.chars[w.names.str(f.ch)]
}

/// `meshWX`.
pub(crate) fn mesh_wx(f: &Fighter) -> Real {
    f.body.x + f.body.facing_dir * f.hit.mesh_x
}

/// `meshWY`.
pub(crate) fn mesh_wy(f: &Fighter) -> Real {
    f.body.y + f.hit.mesh_z
}

/// `inHitstun`.
pub(crate) fn in_hitstun(f: &Fighter) -> bool {
    f.hit.hitstun > 0.0
        || (f.hit.hitpause > 0.0 && f.hit.launch.is_some() && !f.hit.free_launch)
        || f.moves.pull.is_some()
        || f.buffs.buff_stunned
}

/// `invulnerable`.
pub(crate) fn invulnerable(f: &Fighter) -> bool {
    f.invuln.invuln_count > 0
}

/// `weight`: a buff's set weight, else Comp_Actor_Damage's Weight of the pawn's data, of the base's, else KB_WEIGHT.
pub(crate) fn weight(w: &World, f: &Fighter) -> Real {
    if let Some(v) = f.body.weight_set {
        return v;
    }
    let cs = &char_data(w, f).fighter.components;
    let of = |c: Option<&mvs_data::fighter::Component>| {
        c.and_then(|c| c.p.as_obj()).and_then(|p| p.get("Weight")).and_then(Value::as_f64)
    };
    crate::math::narrow(
        of(cs.pawn.get("Comp_Actor_Damage_GEN_VARIABLE"))
            .or_else(|| of(cs.base.get("Comp_Actor_Damage_GEN_VARIABLE")))
            .unwrap_or(f64::from(C::KB_WEIGHT)),
    )
}

/// `pendingLaunch`: a launch not yet given its DI.
fn pending_launch(f: &Fighter) -> Option<(Real, Real)> {
    f.hit.launch.as_ref().filter(|l| !l.di).map(|l| (l.vx, l.vy))
}

/// `World.all[n]`: the pawns, else the players, by place (read anew each turn, as a `for of` does).
pub(crate) fn fighter_at(w: &World, n: usize) -> Option<FighterId> {
    w.pawns.as_ref().unwrap_or(&w.players).get(n).copied()
}

/// The notify a `NotifyRef` names: in the fighter's own montages, else (a copy's montage) in another character's.
pub(crate) fn notify_of<'a>(w: &'a World, f: FighterId, n: NotifyRef) -> Option<&'a Notify> {
    let m = w.names.str(n.montage);
    let own = char_data(w, w.fighter(f));
    let pick = |d: &'a CharData| d.fighter.montages.get(m).and_then(|m| m.notifies.get(n.index as usize));
    pick(own).or_else(|| w.data.chars.values().find_map(|d| pick(d)))
}

/// [`hitframe_packet`] into `out`, its lists' buffers reused.
pub(crate) fn hitframe_packet_into(w: &World, f: FighterId, n: NotifyRef, out: &mut HitPacket) {
    match notify_of(w, f, n) {
        Some(nt) => out.read_props(&w.names, &nt.p, Some(n)),
        None => *out = HitPacket { from: Some(n), ..HitPacket::default() },
    }
}

/// The packet of a Hitframe notify (its properties, typed).
pub(crate) fn hitframe_packet(w: &World, f: FighterId, n: NotifyRef) -> HitPacket {
    match notify_of(w, f, n) {
        Some(nt) => HitPacket::from_props(&w.names, &nt.p, Some(n)),
        None => HitPacket { from: Some(n), ..HitPacket::default() },
    }
}

/// `actorDef(own, cls)`: the class's data in the owner's character, then in the match's other
/// fighters', the stage's and the items'.
pub(crate) fn class_def(w: &World, own: Option<FighterId>, cls: Name) -> Option<&ActorDef> {
    let c = w.names.str(cls);
    if let Some(a) = own.and_then(|o| char_data(w, w.fighter(o)).fighter.actors.as_ref()?.get(c)) {
        return Some(a);
    }
    for (_, f) in w.fighters.iter() {
        if let Some(a) = char_data(w, f).fighter.actors.as_ref().and_then(|m| m.get(c)) {
            return Some(a);
        }
    }
    if let Some(a) = w.data.stage.actors.as_ref().and_then(|m| m.get(c)) {
        return Some(a);
    }
    w.data.items.as_ref().and_then(|i| i.actors.get(c))
}

pub(crate) fn obj<'a>(p: &'a Props, k: &str) -> Option<&'a Props> {
    p.get(k).and_then(Value::as_obj)
}

pub(crate) fn truthy(p: &Props, k: &str) -> bool {
    match p.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

/// An enum member as a plain number reads it (`AttackDecayType ?? 0`). Unverified: a member the data holds by
/// name (no ported packet does) is taken as -1, no decay type.
fn enum_int(e: Option<DataEnum>) -> i32 {
    match e {
        None => 0,
        Some(DataEnum::Value(v)) => v,
        Some(DataEnum::Name(_)) => -1,
    }
}

fn enum_real(e: Option<DataEnum>) -> Option<Real> {
    match e {
        None => None,
        Some(DataEnum::Value(v)) => Some(crate::real(v)),
        Some(DataEnum::Name(_)) => Some(Real::NAN),
    }
}

// ---- Comp_HoldCharge
//
// Comp_HoldCharge and AnimNotify_HoldChargeSet, shared classes: the component of any pawn whose data carries it
// (ChargePack {0: 0.6}), its state on the fighter, its functions, the timer and the notify. StartHoldCharge
// @447-1500, EndHoldCHarge (elapsed / ChargeMultiplier @105), UpdateChargeValues @1996-2486, GetCHargeValue,
// UseCharge, ClearCharges, BeginPlay @1543, TimerEvent @1885-1991; the delegates HoldChargeReachedMax /
// HoldChargeEnd / HoldChargeUsed go to the handlers the owner's pawn binds (Pawn_C023A @1150-1278; Arya's pawn binds
// none).
// Unverified: the hold timer. K2_SetTimerDelegate's timer is pending in the frame it is set and made active in that
// frame's timer pass (FTimerManager::Tick: InternalTime += dt, the active heap, then the pending set with ExpireTime =
// InternalTime + Rate); it fires in the pass where InternalTime > ExpireTime (timer_ticks), and
// K2_GetTimerElapsedTimeHandle is InternalTime - (ExpireTime - Rate): 0 while pending, then the passes since its
// start x DeltaSeconds. The stock FTimerManager read, as the port's other timers take it.
// Not ported: OnOwnerEndPlay's ClearCharges (the pawn's end: nothing reads it after).

const HOLD_COMP: &str = "Comp_HoldCharge_GEN_VARIABLE";

pub use crate::fighter::control::HoldCharge;

/// `HoldChargeHandlers`: the pawn's handlers of the component's three delegates,
/// HoldChargeReachedMax, HoldChargeEnd and HoldChargeUsed, bound at the pawn's BeginPlay (Pawn_C023A @1150-1278; Arya's
/// pawn binds none).
#[derive(Clone, Copy, Debug, Default)]
pub struct HoldChargeHandlers {
    pub reached_max: Option<HoldChargeFn>,
    pub ended: Option<HoldChargeFn>,
    pub used: Option<HoldChargeFn>,
}

/// A handler of one of Comp_HoldCharge's delegates: the owner.
pub type HoldChargeFn = fn(&mut World, FighterId);

/// `bindHoldCharge`: a character's handlers, entered in its `CharHooks` (a table by
/// character key: the fighter's own character answers, so under Arya's Disguise of Gizmo her pawn's, none). Entered
/// twice is a bug of the registry.
pub fn bind_hold_charge(h: &mut crate::registry::char_hooks::CharHooks, handlers: HoldChargeHandlers) {
    assert!(h.hold_charge.is_none(), "hold charge handlers entered twice");
    h.hold_charge = Some(handlers);
}

/// The fighter's character's handler of one delegate.
fn hold_charge_handler(
    w: &World,
    f: FighterId,
    pick: fn(&HoldChargeHandlers) -> Option<HoldChargeFn>,
) -> Option<HoldChargeFn> {
    w.fighter(f).hooks.char.hold_charge.as_ref().and_then(pick)
}

/// HoldChargeReachedMax (TimerEvent @1900).
fn reached_max(w: &mut World, f: FighterId) {
    if let Some(h) = hold_charge_handler(w, f, |h| h.reached_max) {
        h(w, f);
    }
}

/// `holdChargeBeginPlay`: ReceiveBeginPlay @1543 of a new pawn (the World's match
/// reset): ChargePackDefaults = ChargePack (the class data), then every pack 0 @30-442. A pawn whose data has no
/// component has none.
pub fn hold_charge_begin_play(w: &mut World, f: FighterId) {
    let h = {
        let fr = w.fighter(f);
        let p = char_data(w, fr).fighter.components.pawn.get(HOLD_COMP).and_then(|c| c.p.as_obj());
        p.map(|p| {
            let rows = p.get("ChargePack").and_then(Value::as_arr).unwrap_or(&[]);
            let key = |r: &Value| match r.as_obj().and_then(|r| r.get("Key")) {
                Some(Value::Num(n)) => crate::math::dbl::to_int32(*n),
                Some(Value::Str(s)) => s.trim().parse::<f64>().map(crate::math::dbl::to_int32).unwrap_or(0),
                _ => 0,
            };
            let val = |r: &Value| {
                r.as_obj().and_then(|r| r.get("Value")).and_then(Value::as_f64).unwrap_or(f64::from(Real::NAN))
            };
            HoldCharge {
                keys: rows.iter().map(key).collect(),
                defaults: rows.iter().map(|r| crate::math::narrow(val(r))).collect(),
                pack: rows.iter().map(|_| 0.0).collect(),
                multiplier: crate::math::narrow(p.get("ChargeMultiplier").and_then(Value::as_f64).unwrap_or(1.0)),
                ..HoldCharge::default()
            }
        })
    };
    w.fighter_mut(f).charge.hold_charge = h;
}

/// `setHoldChargeMultiplier`: MulticastSetChargeMultiplier @2492, from
/// Comp_State_Manager ApplyChargeBuffs @781: the field only. A running timer keeps its length.
pub fn set_hold_charge_multiplier(w: &mut World, f: FighterId, m: Real) {
    if let Some(h) = &mut w.fighter_mut(f).charge.hold_charge {
        h.multiplier = m;
    }
}

/// TimerEvent @1885 -> TimerEventMulticast @1991: the timer cleared @1955, AtMaxCharge = true @1975; the caller then
/// broadcasts HoldChargeReachedMax @1900 ([`reached_max`]).
fn timer_event(h: &mut HoldCharge) {
    h.timer = 0.0;
    h.at_max = true;
}

fn charge_index(h: &HoldCharge, n: i32) -> Option<usize> {
    h.keys.iter().position(|&k| k == n)
}

/// UpdateChargeValues(Charge, TimeCharged, ClearCharge) @1996-2486. True: HoldChargeUsed @2467 is owed (the caller
/// broadcasts it).
fn update_charge_values(h: &mut HoldCharge, n: i32, t: Real, clear: bool) -> bool {
    h.last_hold = crate::real(n);
    if let Some(i) = charge_index(h, n) {
        // SelectFloat(0, FMin(Pack + T, Default))
        h.pack[i] = if clear { 0.0 } else { min(h.pack[i] + t, h.defaults[i]) };
        if !clear {
            return false;
        }
    }
    h.at_max = false; // @2456
    true // HoldChargeUsed @2467
}

/// HoldChargeUsed @2467.
fn used(w: &mut World, f: FighterId) {
    if let Some(h) = hold_charge_handler(w, f, |h| h.used) {
        h(w, f);
    }
}

fn charge_value(h: &HoldCharge, n: i32) -> GetChargeValueOut {
    match charge_index(h, n) {
        None => GetChargeValueOut { pct: 0.0, has: false },
        Some(i) => GetChargeValueOut { pct: (min(h.pack[i], h.defaults[i]) / h.defaults[i]), has: true },
    }
}

/// `startHoldCharge`: StartHoldCharge(Charge) @447-1500, only for a key of both maps.
pub fn start_hold_charge(w: &mut World, f: FighterId, n: i32) {
    let Some(h) = &mut w.fighter_mut(f).charge.hold_charge else { return };
    let Some(i) = charge_index(h, n) else { return };
    let left = h.defaults[i] - h.pack[i];
    if left <= 0.0 {
        timer_event(h); // @851-865
        return reached_max(w, f);
    }
    let rate = left * h.multiplier;
    if rate == 0.0 {
        timer_event(h); // @1110-1158: TimerEventMulticast, the same path
        return reached_max(w, f);
    }
    h.timer = -1.0; // K2_SetTimerDelegate(TimerEvent, rate, false) @1416: pending until the frame's timer pass
    h.timer_ticks = fh::timer_ticks(Some(rate));
}

/// `endHoldCharge`: EndHoldCHarge(Charge): the elapsed time of a live timer /
/// ChargeMultiplier, else the charge's default.
pub fn end_hold_charge(w: &mut World, f: FighterId, n: i32) {
    let Some(h) = &mut w.fighter_mut(f).charge.hold_charge else { return };
    let t = if h.timer != 0.0 {
        let passes = if h.timer < 0.0 { 0.0 } else { h.timer_ticks - h.timer }; // K2_GetTimerElapsedTimeHandle
        let t = (passes * C::DELTA_SECONDS) / h.multiplier;
        h.timer = 0.0; // K2_ClearAndInvalidateTimerHandle @178
        t
    } else {
        charge_index(h, n).map_or(0.0, |i| h.defaults[i]) // Map_Find(ChargePackDefaults) @250-319
    };
    if update_charge_values(h, n, t, false) {
        used(w, f);
    }
    // EndChargeMulticast -> HoldChargeEnd @2520
    if let Some(h) = hold_charge_handler(w, f, |h| h.ended) {
        h(w, f);
    }
}

/// `getChargeValue`: ChargePercent = FMin(Pack, Default) / Default and HasCharge;
/// (0, false) without the component or the key.
pub fn get_charge_value(w: &World, f: FighterId, n: i32) -> GetChargeValueOut {
    match &w.fighter(f).charge.hold_charge {
        Some(h) => charge_value(h, n),
        None => GetChargeValueOut { pct: 0.0, has: false },
    }
}

/// `useCharge`: with HasCharge, UpdateChargeValues(Charge, ChargePercent, true).
pub fn use_charge(w: &mut World, f: FighterId, n: i32) {
    let Some(h) = &mut w.fighter_mut(f).charge.hold_charge else { return };
    let v = charge_value(h, n);
    if v.has && update_charge_values(h, n, v.pct, true) {
        used(w, f);
    }
}

/// `clearCharges`: AtMaxCharge = false, every pack 0 (the timer stays).
pub fn clear_charges(w: &mut World, f: FighterId) {
    if let Some(h) = &mut w.fighter_mut(f).charge.hold_charge {
        h.at_max = false;
        h.pack.iter_mut().for_each(|p| *p = 0.0);
    }
}

/// `holdChargeTimer`: the timer, in the frame's timer pass.
pub fn hold_charge_timer(w: &mut World, f: FighterId) {
    let Some(h) = &mut w.fighter_mut(f).charge.hold_charge else { return };
    if h.timer == 0.0 {
        return;
    }
    if h.timer < 0.0 {
        h.timer = h.timer_ticks; // made active: fires after timer_ticks passes from here
        return;
    }
    h.timer -= 1.0;
    if h.timer == 0.0 {
        timer_event(h);
        reached_max(w, f);
    }
}

/// AnimNotify_HoldChargeSet's ChargeNumber (the notify's, default 0).
fn charge_number(w: &World, f: FighterId, n: NotifyRef) -> i32 {
    notify_of(w, f, n)
        .and_then(|nt| nt.p.get("ChargeNumber"))
        .and_then(Value::as_f64)
        .map_or(0, crate::math::dbl::to_int32)
}

fn hold_charge_set_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let c = charge_number(w, f, n);
    start_hold_charge(w, f, c);
}

fn hold_charge_set_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let c = charge_number(w, f, n);
    end_hold_charge(w, f, c);
}

/// AnimNotify_HoldChargeSet: StartHoldCharge(ChargeNumber) at Begin, EndHoldCHarge(ChargeNumber) at End, on
/// MeshComp.GetOwner()'s Comp_HoldCharge. An owner without the component does nothing (the IsValid @191-220).
pub fn register(r: &mut crate::registry::Registry) -> Result<(), crate::registry::RegistryError> {
    r.notifies.enter(
        "AnimNotify_HoldChargeSet_C",
        crate::registry::NotifyHandler {
            begin: Some(hold_charge_set_begin),
            end: Some(hold_charge_set_end),
            ..Default::default()
        },
    )
}

// ---- a spawned dealer's fields

/// The fields of a spawned dealer a hit reads, taken once at the call (one record for every kind).
#[derive(Clone, Copy, Debug)]
pub struct Dealer {
    /// The actor; `None` for a dealer no list holds (a `PerkDealer`, a literal dealer).
    pub a: Option<ActorRef>,
    pub cls: Name,
    /// GetOwner: the team, the hit buffs, the attack decay, the owner hit pause.
    pub owner: Option<FighterId>,
    pub lookup_fighter: Option<FighterId>,
    /// GetInstigator of a Projectile (its override, else the owner); `None` for every other kind.
    pub inst: Option<FighterId>,
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    pub facing: Real,
    /// A Projectile's `yawX`.
    pub yaw_x: Option<Real>,
    /// The velocity pfgDoDamage reads (`hitVelocity`) of a Projectile or an Item.
    pub hit_velocity: Option<[Real; 2]>,
    pub is_item: bool,
    /// The dealer carries a buff list (`elem`).
    pub has_elem: bool,
    /// It holds charge values (`charge`).
    pub charged: bool,
    pub attack_id: i32,
    pub dead: bool,
    pub age: i32,
    pub has_hit_a_fighter: bool,
}

/// The buff classes whose actor deals a hit, with the class data a dealer reads none of (`DestroyedOnHit`, ...).
// Unverified: Buff_Perk_WonderWoman_FastFallSlam_C (`chars::wonder_woman::perks`) and Buff_ConfettiExplosionCount_C
// (`chars::harley::actors`) are entered ahead of their ports; their data's dealer keys are checked by those ports.
// Buff_Arrested_C (`chars::velma::police`) reads none of its class data (`dealer.p` is `instDef`).
fn buff_actor_dealer(cls: Name) -> bool {
    cls == crate::name!("Buff_DelayKnockUp_C")
        || cls == crate::name!("Buff_BasketballPass_C")
        || cls == crate::name!("Buff_IgnitedBasketballPass_C")
        || cls == crate::name!("Buff_C021_EarthSmashVictim_C")
        || cls == crate::combat::electric::ELECTRIC
        || cls == crate::combat::electric::LOW_ELECTRIC
        || cls == crate::name!("Buff_Perk_WonderWoman_FastFallSlam_C")
        || cls == crate::name!("Buff_ConfettiExplosionCount_C")
        || cls == crate::name!("Buff_Arrested_C")
}

/// The dealer fields of `a` (a Projectile or an Item; another kind is not ported here).
pub fn dealer(w: &World, a: ActorRef) -> Dealer {
    match a {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id).expect("a live projectile");
            let inst = match p.inst_override {
                crate::actors::projectile::OwnerOverride::Unset => p.owner,
                crate::actors::projectile::OwnerOverride::None => None,
                crate::actors::projectile::OwnerOverride::Fighter(f) => Some(f),
            };
            let hv = {
                let (hx, hy) = crate::actors::projectile::hit_velocity(p);
                [hx, hy]
            };
            let side = p.spawn_side.unwrap_or(p.facing);
            Dealer {
                a: Some(a),
                cls: p.cls,
                owner: p.owner,
                lookup_fighter: p.lookup_fighter,
                inst,
                x: p.x,
                y: p.y,
                vx: p.vx,
                vy: p.vy,
                facing: p.facing,
                yaw_x: Some(if matches!(p.ext, crate::actors::projectile::ProjectileExt::Bomb(_)) {
                    p.facing // C017_Bomb_C overrides yawX
                } else if matches!(p.ext, crate::actors::projectile::ProjectileExt::GunShot(_)) {
                    -1.0 // C023B_SpawnedGunShot_C overrides yawX
                } else if matches!(p.ext, crate::actors::projectile::ProjectileExt::MeeseeksSummon(_)) {
                    crate::actors::projectile::yaw_x(p)
                } else if p.rot_x != 0.0 && !p.rot_x.is_nan() {
                    p.rot_x
                } else {
                    side
                }),
                hit_velocity: Some(hv),
                is_item: false,
                has_elem: p.elem.is_some(),
                charged: p.charge.is_some(),
                attack_id: p.attack_id,
                dead: p.dead,
                age: p.age,
                has_hit_a_fighter: p.has_hit_a_fighter,
            }
        }
        ActorRef::Item(id) => {
            let it = w.actors.items.get(id).expect("a live item");
            Dealer {
                a: Some(a),
                cls: it.cls,
                owner: it.owner,
                lookup_fighter: it.lookup_fighter,
                inst: None,
                x: it.x,
                y: it.y,
                vx: it.vx,
                vy: it.vy,
                facing: it.facing,
                yaw_x: None,
                hit_velocity: Some({
                    let v = it.hit_velocity();
                    [v.x, v.y]
                }),
                is_item: true,
                has_elem: it.elem.is_some(),
                charged: it.charge.is_some(),
                attack_id: it.attack_id,
                dead: it.dead,
                age: it.age,
                has_hit_a_fighter: it.has_hit_a_fighter,
            }
        }
        ActorRef::Actor(id) => {
            use crate::actors::world_actor::WorldActorState;
            let x = w.actors.actors.get(id).expect("a live world actor");
            // Its place, facing and AttackID by class: a shockwave's and a laser damage actor's own, a flight actor's the
            // owner's (attached).
            let (px, py, pfacing, pattack) = match &x.state {
                WorldActorState::Shockwave(sw) => (sw.x, sw.y, sw.facing, sw.attack_id),
                WorldActorState::FlightActor(fl) => (
                    w.fighter(x.owner).body.x,
                    crate::fighter::actor_y(w, x.owner),
                    crate::fighter::facing(w, x.owner),
                    fl.attack_id,
                ),
                WorldActorState::GrabHit(g) => (
                    w.fighter(x.owner).body.x,
                    crate::fighter::actor_y(w, x.owner),
                    crate::fighter::facing(w, x.owner),
                    g.attack_id,
                ),
                WorldActorState::LaserDamage(d) => (d.x, d.y, d.facing, d.attack_id),
                WorldActorState::DisguiseShock(s) => (s.x, s.y, s.facing, s.attack_id),
                WorldActorState::ForceField(_) | WorldActorState::PerkForceField(_) => {
                    crate::chars::black_adam::field::dealer_parts(&x.state).expect("a force field")
                }
                WorldActorState::Bolt(s) => (s.x, s.y, s.facing, s.attack_id),
                WorldActorState::HarleyBomb(b) => crate::chars::harley::bomb::dealer_parts(w, x, b),
                WorldActorState::JackBox(j) => crate::chars::harley::actors::dealer_parts(w, x, j),
                WorldActorState::CannonBurst(b) => (b.x, b.y, b.facing, b.attack_id),
                WorldActorState::AntenaBolts(s) => crate::chars::iron_giant::actor_state::antena_dealer_parts(w, x, s),
                WorldActorState::StompWave(s) => crate::chars::iron_giant::actor_state::stomp_dealer_parts(w, x, s),
                WorldActorState::DownThruster(s) => {
                    crate::chars::iron_giant::actor_state::thruster_dealer_parts(w, x, s)
                }
                WorldActorState::TurretBomb(s) => crate::chars::iron_giant::turret_actors::bomb_dealer_parts(w, x, s),
                WorldActorState::Dogpile(s) => crate::chars::taz::dogpile::dealer_parts(w, x, s),
                WorldActorState::ShockwaveHitBox(s) => crate::chars::garnet::shockwave::dealer_parts(w, x, s),
                WorldActorState::ElectricZone(s) => crate::chars::garnet::zone::dealer_parts(w, x, s),
                WorldActorState::AirFlap(s) => crate::chars::rick::air_flap::dealer_parts(w, x, s),
                WorldActorState::PolymorphGrenade(s) => crate::chars::rick::grenade::dealer_parts(w, x, s),
                WorldActorState::PortalActor(s) => crate::chars::rick::portal::dealer_parts(w, x, s),
                WorldActorState::HighFiveShockwave(s) => crate::chars::finn::high_five::dealer_parts(w, x, s),
                WorldActorState::GemBoostBlast(s) => crate::chars::finn::perks::dealer_parts(w, x, s),
                WorldActorState::FinnShockwave(s) => crate::chars::finn::shockwave::dealer_parts(w, x, s),
                WorldActorState::Beam(s) => crate::chars::velma::beam::dealer_parts(w, x, s),
                WorldActorState::VelmaCarryRun(s) => crate::chars::velma::carry::dealer_parts(w, x, s),
                WorldActorState::Ray(s) => crate::chars::marvin::ray::dealer_parts(w, x, s),
                WorldActorState::RocketExplosion(s) => crate::chars::marvin::ufo::rocket_dealer_parts(w, x, s),
                WorldActorState::AllySaw(s) => crate::chars::stripe::saw::dealer_parts(w, x, s),
                WorldActorState::Stretch(s) => crate::chars::jake::stretch::dealer_parts(w, x, s),
                WorldActorState::JakeHouse(s) => crate::chars::jake::house::dealer_parts(w, x, s),
                WorldActorState::JakeHorse(s) => crate::chars::jake::horse::dealer_parts(w, x, s),
                WorldActorState::SpitHitBox(s) => crate::chars::jake::spit::dealer_parts(w, x, s),
                WorldActorState::JakeSpitAllyHitBox(s) => crate::chars::jake::ally_spit::dealer_parts(w, x, s),
                WorldActorState::JerryActor(s) => crate::chars::tom_and_jerry::jerry::dealer_parts(w, x, s),
                WorldActorState::MouseTrap(s) => crate::chars::tom_and_jerry::trap::dealer_parts(w, x, s),
                WorldActorState::BananaGuard(s) => crate::items::banana_guard::dealer_parts(w, x, s),
                _ => crate::unported!("hits: a dealer of this World.actors class", w, a),
            };
            // A DealerActor: no velocity of its own (vx = vy = 0, no hitVelocity), no yaw; Jerry's actor has a
            // ProjectileMovement of its own (`DealerActor.hitVelocity`: `jerry::dealer_motion`).
            let (pvx, pvy, phv) = match &x.state {
                WorldActorState::JerryActor(s) => crate::chars::tom_and_jerry::jerry::dealer_motion(w, x, s),
                _ => (0.0, 0.0, None),
            };
            // Unverified: HasHitAFighter is kept on a projectile or an item alone. A flight actor's two packets set
            // HitOnlyOneFighter, but the actor deals one packet and is destroyed (nothing reads the flag again), and
            // the laser damage actor tracks its victims in its own set.
            Dealer {
                a: Some(a),
                cls: x.cls,
                owner: Some(x.owner),
                lookup_fighter: None,
                inst: None,
                x: px,
                y: py,
                vx: pvx,
                vy: pvy,
                facing: pfacing,
                yaw_x: None,
                hit_velocity: phv,
                is_item: false,
                has_elem: x.elem.is_some(),
                charged: x.charge.is_some(),
                attack_id: pattack,
                dead: x.dead,
                age: 0,
                has_hit_a_fighter: false,
            }
        }
        ActorRef::Buff(b) => {
            // A buff actor's own Comp_Pawn_HitBoxHandler (DealerActor.buffActor): at its owner (the victim's x,
            // actorY), no velocity, yaw or charge, no buff list; credited to the class's Instigator.
            let x = crate::combat::buffs::buff(w, b).expect("a live buff");
            // The class data a dealer reads (`pr.p`: DestroyOnBlocked, ApplyHitPauseToOwner, ...) is none: the
            // buff-actor dealers ported (`buff_actor_dealer`: Buff_DelayKnockUp, Buff_BasketballPass,
            // Buff_IgnitedBasketballPass, Buff_C021_EarthSmashVictim, Buff_Damage_Electric and its child
            // Buff_Low_Damage_Electric) have none of them.
            if !buff_actor_dealer(x.cls) {
                crate::unported!("hits: a dealer of this buff actor class", w, a);
            }
            let attack_id = crate::combat::buffs::inst_view(w, b)
                .obj("HitBoxDamage")
                .and_then(|h| h.get("AttackID"))
                .and_then(Value::as_f64)
                .map_or(0, crate::math::dbl::to_int32);
            Dealer {
                a: Some(a),
                cls: x.cls,
                owner: x.ext.actor_owner(),
                lookup_fighter: None,
                inst: None,
                x: w.fighter(b.fighter).body.x,
                y: crate::fighter::actor_y(w, b.fighter),
                vx: 0.0,
                vy: 0.0,
                facing: 1.0,
                yaw_x: None,
                hit_velocity: None,
                is_item: false,
                has_elem: false,
                charged: false,
                attack_id,
                dead: false,
                age: 0,
                has_hit_a_fighter: false,
            }
        }
        ActorRef::Marker(_) | ActorRef::Mortar(_) => crate::chars::morty::actors::dealer(w, a),
        _ => crate::unported!("hits: a dealer of a typed list", w, a),
    }
}

impl Dealer {
    /// The class data (`pr.p`, the actor CDO, and its components).
    fn def<'a>(&self, w: &'a World) -> Option<&'a ActorDef> {
        class_def(w, self.owner.or(self.lookup_fighter), self.cls)
    }

    /// `pr.p`: a projectile's own instance variables where a graph wrote them (`setInstanceVars`), else the class's.
    fn p<'a>(&self, w: &'a World) -> Option<&'a Props> {
        if let Some(ActorRef::Projectile(id)) = self.a
            && let Some(pr) = w.actors.projectiles.get(id)
        {
            return Some(crate::actors::projectile::class_p(w, pr));
        }
        self.def(w).map(|d| &d.p)
    }

    pub(crate) fn flag(&self, w: &World, k: &str) -> bool {
        self.p(w).is_some_and(|p| truthy(p, k))
    }

    /// The class's `On Hit Buffs`.OnHitBuffs rows, or an item's own where its graph wrote them.
    pub fn buffs(&self, w: &World) -> Vec<OnHitBuffRow> {
        if let Some(ActorRef::Item(id)) = self.a
            && let Some(rows) = crate::actors::item::item_own_rows(w, id)
        {
            return rows;
        }
        let rows = self.p(w).and_then(|p| obj(p, "On Hit Buffs")).and_then(|o| o.get("OnHitBuffs"));
        rows.map_or_else(Vec::new, |r| OnHitBuffRow::rows(&w.names, Some(r)))
    }

    /// One of [`Dealer::buffs`] meets `f`; a class's rows are read without a list (an item's own rows are one).
    pub fn any_buff_row(&self, w: &World, f: impl Fn(&OnHitBuffRow) -> bool) -> bool {
        if let Some(ActorRef::Item(id)) = self.a
            && let Some(rows) = crate::actors::item::item_own_rows(w, id)
        {
            return rows.iter().any(f);
        }
        let rows = self.p(w).and_then(|p| obj(p, "On Hit Buffs")).and_then(|o| o.get("OnHitBuffs"));
        rows.is_some_and(|r| OnHitBuffRow::any(&w.names, Some(r), f))
    }

    fn set_has_hit_a_fighter(&self, w: &mut World) {
        match self.a {
            Some(ActorRef::Projectile(id)) => {
                w.actors.projectiles.get_mut(id).expect("a live projectile").has_hit_a_fighter = true
            }
            Some(ActorRef::Item(id)) => w.actors.items.get_mut(id).expect("a live item").has_hit_a_fighter = true,
            _ => {}
        }
    }

    fn is_dead(&self, w: &World) -> bool {
        match self.a {
            Some(ActorRef::Projectile(id)) => w.actors.projectiles.get(id).is_none_or(|p| p.dead),
            Some(ActorRef::Item(id)) => w.actors.items.get(id).is_none_or(|i| i.dead),
            _ => self.dead,
        }
    }

    fn has_elem_now(&self, w: &World) -> bool {
        match self.a {
            Some(ActorRef::Projectile(id)) => w.actors.projectiles.get(id).is_some_and(|p| p.elem.is_some()),
            Some(ActorRef::Item(id)) => w.actors.items.get(id).is_some_and(|i| i.elem.is_some()),
            _ => self.has_elem,
        }
    }
}

// ---- the call surface of the hit routes

/// `dealerIsProjectile`: DamageAndLaunch @6238-6327: the DamageDealer is a projectile
/// when its Comp_Actor_Destructible has IsProjectile (a thrown item has it from the throw on, Item ubergraph
/// 10289: every hit an Item deals here).
/// Unverified: "or implements BPI_Projectiles" is not read (which classes have the interface is not in the data).
pub fn dealer_is_projectile(w: &World, pr: &Dealer) -> bool {
    pr.is_item || class_is_projectile(w, pr.owner.or(pr.lookup_fighter), pr.cls)
}

/// `dealerIsProjectile` of a dealer given by its class and owner alone (a struct literal, no Item): its
/// class's Comp_Actor_Destructible has IsProjectile.
pub fn class_is_projectile(w: &World, owner: Option<FighterId>, cls: Name) -> bool {
    class_def(w, owner, cls)
        .and_then(|d| obj(&d.components, "Comp_Actor_Destructible"))
        .is_some_and(|c| truthy(c, "IsProjectile"))
}

/// `reflectingProjectiles`: one of the handler's active Hitframes has ReflectProjectiles.
pub fn reflecting_projectiles(w: &World, f: FighterId) -> bool {
    w.fighter(f)
        .montages
        .active_hits
        .values()
        .any(|h| notify_of(w, f, h.n).is_some_and(|n| truthy(&n.p, "ReflectProjectiles")))
}

/// `allyObserved`: the pawn binds HitAlly or reads MostRecentHitActor.
pub fn ally_observed(w: &World, f: FighterId) -> bool {
    let c = w.fighter(f).hooks.char;
    c.hit_ally.is_some() || c.reads_recent_hit_actor
}

/// `fighterAllyActorHit`: NotifyAllyOfHit (Comp_Pawn_HitBoxHandler fn @0-923) of a
/// fighter's handler on a HitActor that is no fighter: a team mate's spawned actor hit by a packet with
/// DestroyProjectiles or ReflectProjectiles takes MostRecentHitActor (@433) and HitAllyProjectile @657.
/// Not here: ElementalBuffHitAlly @472 and the victim handler's HitByAlly @584 (the DamageManager's broadcast).
pub fn fighter_ally_actor_hit(
    w: &mut World,
    att: FighterId,
    vic: ActorRef,
    vic_owner: Option<FighterId>,
    p: &HitPacket,
) {
    let Some(o) = vic_owner else { return };
    if w.fighter(o).team.team != w.fighter(att).team.team {
        return;
    }
    if !(p.destroy_projectiles.unwrap_or(false) || p.reflect_projectiles.unwrap_or(false)) {
        return;
    }
    w.fighter_mut(att).decay.recent_hit_actor = Some(vic);
    buff::buff_signal(
        w,
        att,
        BuffSignal::HitAllyProjectile,
        BuffSignalArg { ally_actor: Some(vic), ..Default::default() },
    );
    if let Some(h) = w.fighter(att).hooks.char.hit_ally_projectile {
        h(w, att, vic);
    }
}

/// `hitBy`: a projectile's own OnHitByAlly / OnHitByEnemy (SpawnedActorParent's
/// handlers do nothing, so a class without the hook gets no call).
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn hit_by(
    w: &mut World,
    g: crate::actors::ProjectileId,
    ally: bool,
    dx: Real,
    dy: Real,
    bkb: Real,
    attack_type: i32,
    by: Option<ActorRef>,
) {
    let cls = w.actors.projectiles.get(g).expect("a live projectile").cls;
    let hook = w.registry.actors.get(w.names.str(cls)).and_then(|e| match &e.hooks {
        ActorHooks::Projectile(h) => h.hit_by,
        _ => None,
    });
    if let Some(h) = hook {
        h(w, g, ally, dx, dy, bkb, attack_type, by);
    }
}

/// `projectileHitDir`: pfgDoDamage's direction for a hitbox owned by a spawned actor
/// (ubergraph 9032-11499). VelocityInsteadOfFacing with a velocity whose Normal2D in (Y, Z) is not zero (@9468-9505
/// EqualEqual_Vector2DVector2D(.., (0, 0), 0.0001)): the direction is that normal, and BaseKnockBack = Lerp(BKB,
/// BKB x (normal.Z < 0 ? 0.7 : 1.2), clamp(|normal.Z|, 0, 1)). Otherwise the hit's vector with X flipped when the dealer comes from the left. Then KnockBackMirror
/// (projectile_mirror) on either branch.
pub fn projectile_hit_dir(w: &World, pr: &Dealer, p: &HitPacket, vic: Option<Real>) -> ProjectileHitDirOut {
    let (dxv, dyv) =
        p.knock_back_direction_vector.map_or((1.0, 0.0), |d| (d.x.unwrap_or(Real::NAN), d.y.unwrap_or(Real::NAN)));
    let bkb = p.base_knock_back.unwrap_or(0.0);
    let hv = if p.velocity_instead_of_facing.unwrap_or(false) { pr.hit_velocity } else { None };
    if let Some([hx, hy]) = hv {
        let [nx, ny] = ue::safe_normal2d([hx, hy]);
        if !(nx.abs() <= 1e-4 && ny.abs() <= 1e-4) {
            let b = bkb + (bkb * (if ny < 0.0 { 0.7 } else { 1.2 }) - bkb) * min(ny.abs(), 1.0);
            return ProjectileHitDirOut { x: projectile_mirror(w, pr, p, vic, nx), y: ny, bkb: b };
        }
    }
    let s = pr.yaw_x.unwrap_or_else(|| sign(if pr.vx != 0.0 && !pr.vx.is_nan() { pr.vx } else { pr.facing }));
    ProjectileHitDirOut { x: projectile_mirror(w, pr, p, vic, dxv * s), y: dyv, bkb }
}

/// `projectileMirror`: pfgDoDamage @11535-12094 (KnockBackMirror): X x -1 when the hit
/// actor's location is behind the dealer actor (HitBoxHandlerFacingLeft ? HitActor.Y <= Owner.Y : HitActor.Y >=
/// Owner.Y; the handler's facing is false for every ported dealer). Without `vic` X stays.
pub fn projectile_mirror(w: &World, pr: &Dealer, p: &HitPacket, vic: Option<Real>, x: Real) -> Real {
    let Some(vx) = vic else { return x };
    if !p.knock_back_mirror.unwrap_or(false) {
        return x;
    }
    // Projectile.handlerFacingLeft; every other dealer kind writes no such field (false)
    let left = match pr.a {
        Some(ActorRef::Projectile(id)) => {
            w.actors.projectiles.get(id).is_some_and(crate::actors::projectile::handler_facing_left)
        }
        _ => false,
    };
    let behind = if left { vx >= pr.x } else { vx <= pr.x };
    if behind { -x } else { x }
}

/// `allyHit`: NotifyAllyOfHit for a spawned actor's handler, at the end of pfgDoDamage:
/// a victim that is not the Instigator, of its TeamIndex, a fighter. The victim's handler broadcasts HitByAlly
/// @584; then the dealer's own HitAlly @886 (its passive elemental's ElementalHitAlly), then the class's own.
/// Not here: ElementalBuffHitAlly @472; a fighter dealer; a victim that is no fighter; ItemActorParent's HitAlly
/// (`item_ally_hit`).
pub fn ally_hit(w: &mut World, pr: &Dealer, vic: FighterId, p: &HitPacket) {
    notify_ally_of_hit(w, pr, vic, p, false);
}

/// [`ally_hit`] of a thrown item's handler, whose HitAlly @886 also reaches ItemActorParent's HitAlly ->
/// ItemThrowHit(true, false) @7501 (bound in the item's BeginPlay, after the elemental's: behind it here).
pub fn item_ally_hit(w: &mut World, pr: &Dealer, vic: FighterId, p: &HitPacket) {
    notify_ally_of_hit(w, pr, vic, p, true);
}

fn notify_ally_of_hit(w: &mut World, pr: &Dealer, vic: FighterId, p: &HitPacket, item_hit_ally: bool) {
    let Some(att) = pr.owner else { return };
    if vic == att || w.fighter(vic).team.team != w.fighter(att).team.team {
        return;
    }
    if buff::hit_by_ally_bound(w, vic) {
        // KnockbackTargetSocket's fighter: the dealer's Instigator; the rest GetOwner.
        let vacuum = p.vacuum_knockback.unwrap_or(false);
        let bkb = if vacuum {
            legacy_vacuum(w, pr.inst.unwrap_or(att), vic, p, None).bkb
        } else {
            projectile_hit_dir(w, pr, p, None).bkb
        };
        let mut hit = p.clone();
        if vacuum {
            hit.base_knock_back = Some(bkb);
            hit.knock_back_scaling = Some(1.0);
        } else if bkb != p.base_knock_back.unwrap_or(0.0) {
            hit.base_knock_back = Some(bkb);
        }
        // Unverified: LocalHitInformation is taken as the packet after the direction step; CalculateArmor @7560 and
        // the ForceKnockback launch @8093 are not made for a spawned actor's hit on a team mate.
        let a = pr.a.expect("a dealer of a list: a PerkDealer deals no hit on a team mate");
        let by = crate::fighter::common::actor_by(w, a, None);
        let arg = BuffSignalArg { hit: Some(Box::new(hit.clone())), by: Some(by), ..Default::default() };
        buff::buff_signal(w, vic, BuffSignal::DamageHitByAlly, arg);
        let arg = BuffSignalArg { hit: Some(Box::new(hit)), dealer: Some(att), ..Default::default() };
        buff::buff_signal(w, vic, BuffSignal::HitByAlly, arg);
    }
    if let Some(a) = pr.a
        && pr.has_elem_now(w)
    {
        elem::elem_hit_ally(w, a, vic);
    }
    // The dealer's own HitAlly override (C021_ElectricChain @5273), behind its passive elemental's.
    if let Some(ActorRef::Projectile(id)) = pr.a
        && !pr.is_dead(w)
    {
        let hook = w.registry.actors.get(w.names.str(pr.cls)).and_then(|e| match &e.hooks {
            ActorHooks::Projectile(h) => h.hit_ally,
            _ => None,
        });
        if let Some(h) = hook {
            h(w, id, vic);
        }
    }
    if item_hit_ally
        && let Some(ActorRef::Item(it)) = pr.a
        && !pr.is_dead(w)
    {
        crate::actors::item::item_throw_hit(w, it, true, false);
    }
}

/// pfgDoDamage of a spawned actor's handler on a team mate, behind the gates its caller tested (the overlap, the
/// DoLOSCheck, the projectile block, the invulnerable pop @4608 -> @7381, HitEachActorOnlyOnce's TrackActorsHitByTag
/// @5290): the On Hit Buffs rows for an ally (@5931-6835, `hit_buffs_ally`, Round(StackCount x `mult`)), then
/// NotifyAllyOfHit @21344 (`ally_hit`). DamageAndLaunch between them refuses the team mate (CanBeHitBy @718); its
/// HitByAlly broadcast @8047 is `ally_hit`'s DamageHitByAlly. The ally path of the World's projectile test
/// (`combat::phases` PendingHit::ProjectileAlly, behind the entry rows) and of the actors that test their own shape
/// (WonderWoman_BracerClash and Steven_BubbleCharge's `clash_allies`, C016_SassBubble's box).
pub fn actor_ally_hit(w: &mut World, d: &Dealer, vic: FighterId, p: &HitPacket, rows: &[OnHitBuffRow], mult: Real) {
    let Some(att) = d.owner else { return };
    hit_buffs_ally(w, att, vic, rows, mult);
    ally_hit(w, d, vic, p);
}

/// `projectileBlocks`: pfgDoDamage @4300-4598: a projectile dealer against a victim
/// whose IsInvulnerableToProjectiles holds pops the call; the same pop @4145-4471 for a dealer whose
/// BPI_CanBeReflected holds against a victim whose handler IsReflectingProjectiles.
/// Deviation: one variable per handler, the last HitBoxesOn's; the port reads "one of the active Hitframes has
/// it".
pub fn projectile_blocks(w: &World, pr: &Dealer, vic: FighterId) -> bool {
    blocks(w, pr, None, vic)
}

/// `projectileBlocks` of a dealer or of a dealer no list holds (`lit`: its class data's CanBeReflected, as
/// `canBeReflected` reads an actor with no class hook).
fn blocks(w: &World, pr: &Dealer, lit: Option<&PerkDealer>, vic: FighterId) -> bool {
    if crate::actors::smoke_bomb::projectile_blocked(w, vic) && dealer_is_projectile(w, pr) {
        return true;
    }
    reflecting_projectiles(w, vic)
        && match (pr.a, lit) {
            (Some(a), _) => crate::combat::reflect::can_be_reflected(w, a),
            (None, Some(l)) => truthy(&l.p, "CanBeReflected"),
            (None, None) => false,
        }
}

/// `pr.p`: the class data of a dealer, or of a dealer no list holds.
fn dealer_p<'a>(w: &'a World, pr: &Dealer, lit: Option<&'a PerkDealer>) -> Option<&'a Props> {
    match lit {
        Some(l) => Some(&l.p),
        None => pr.p(w),
    }
}

/// `projectileShielded`: DamageAndLaunch @6113-6966: HasProjectileArmor against a
/// projectile dealer whose attack type is not 4.
fn projectile_shielded(w: &World, pr: &Dealer, lit: Option<&PerkDealer>, vic: FighterId, elementals: &[Name]) -> bool {
    if elementals.contains(&crate::name!("Buff_Elemental_NoArmorDamage_C")) || !w.fighter(vic).invuln.projectile_armor {
        return false;
    }
    let at = dealer_p(w, pr, lit).and_then(|p| p.get("ProjectileAttackType")).and_then(Value::as_f64).unwrap_or(0.0);
    dealer_is_projectile(w, pr) && at != 4.0
}

/// `lightArmorFails`: DamageAndLaunch @6494 -> @7067-7268: Light Projectile Armor
/// against a projectile dealer, when HasProjectileArmor did not take it.
pub fn light_armor_fails(w: &World, pr: &Dealer, vic: FighterId, elementals: &[Name]) -> bool {
    armor_fails(w, pr, None, vic, elementals)
}

fn armor_fails(w: &World, pr: &Dealer, lit: Option<&PerkDealer>, vic: FighterId, elementals: &[Name]) -> bool {
    if elementals.contains(&crate::name!("Buff_Elemental_NoArmorDamage_C"))
        || w.fighter(vic).invuln.light_projectile_armor != Some(true)
    {
        return false;
    }
    dealer_is_projectile(w, pr) && !projectile_shielded(w, pr, lit, vic, elementals)
}

/// `actorHitEnemy`: pfgDoDamage's HitEnemyPlayer broadcast for a spawned actor whose hit
/// the port deals on its own path: its elementals, the buffs granted to it ('hitEnemyPlayerFirst', then
/// 'hitEnemyPlayer'). Nothing for an actor without a buff list.
/// Unverified: the late bindings are signalled ahead of the class's own part on these paths.
pub fn actor_hit_enemy(w: &mut World, pr: &Dealer, vic: FighterId) {
    let team = pr.owner.map_or(-1, |o| w.fighter(o).team.team);
    // (a dealer no list holds has no buff list)
    let Some(a) = pr.a.filter(|_| pr.has_elem_now(w)) else { return };
    if w.fighter(vic).team.team == team {
        return;
    }
    elem::elem_hit_enemy(w, a, vic);
    let arg = ActorBuffArg { by: vic, kx: 0.0, ky: 0.0 };
    if pr.has_elem_now(w) {
        elem::actor_buff_signal(w, a, ActorBuffSignal::HitEnemyPlayerFirst, Some(&arg));
    }
    if pr.has_elem_now(w) {
        elem::actor_buff_signal(w, a, ActorBuffSignal::HitEnemyPlayer, Some(&arg));
    }
}

/// `applyProjectileHit`: HitBoxHandler for a spawned actor. The dealer is the
/// projectile, and SpawnedActorParent carries a Comp_State_Manager, so Calculate Knock Back takes path A like a
/// fighter's hit: the dealer's dealt KB mods (none), the victim's taken mods, then the damage tiers. The charge is
/// the dealer actor's own Comp_Pawn_ChargeAttack values; no dealer hitpause and no dealer evade refund. AttackID
/// and combo attribution go to the projectile's owner. `p`: the packet (the dealer's `hit` by default); `buffs`:
/// the OnHitBuffs rows (the class's by default); `at`: HitLocation; `dir`: a world direction for an IgnoreFacing
/// packet an actor built; `elementals`: pfgDoDamage's ElementalBuffsParam; `charge_percent`: DamageAndLaunch's
/// ChargePercentParam (the dealer's PercentCharged by default). True: Hit.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn apply_projectile_hit(
    w: &mut World,
    pr: &Dealer,
    vic: FighterId,
    p: &HitPacket,
    buffs: &[OnHitBuffRow],
    at: V2,
    dir: Option<V2>,
    elementals: &[Name],
    charge_percent: Real,
) -> bool {
    spawned_hit(w, pr, None, vic, p, buffs, at, dir, elementals, charge_percent)
}

/// `applyProjectileHit` with a dealer no list holds (`lit`: a literal dealer: the perks'
/// BlastBoxExitKnockbackExplosion, checkForBlastBoxBounce, and Buff_C016_DistanceShot's
/// explosion): its class data and charge values are the literal's own; it carries no buff
/// list, no class hook and no collision of its own (HitCollision none: `boxed` false); a buff actor's handler
/// (`buff_actor`) has no attack decay. No OnHitBuffs rows (both callers pass none); `dir` the world direction
/// of the IgnoreFacing packet the caller built; ChargePercentParam the literal's PercentCharged.
/// Deviation: the build hands the dealer actor itself to the victim's taken conditions (their Attacker), to the
/// KnockedBack broadcast (its DamageDealer, and `actorBy` of it for the buffs) and to HitByEnemy (`by`); it is in no
/// list, so the port has no `ActorRef` for it: the taken conditions get no Attacker, the KnockedBack buffs no argument
/// and the pawn's KnockedBack hooks the Instigator, the HitByEnemy buffs no `by`.
pub fn apply_dealer_hit(w: &mut World, d: &PerkDealer, vic: FighterId, p: &HitPacket, at: V2, dir: V2) -> bool {
    let pr = Dealer {
        a: None,
        cls: d.cls,
        owner: Some(d.owner),
        lookup_fighter: None,
        inst: None,
        x: d.x,
        y: d.y,
        vx: 0.0,
        vy: 0.0,
        facing: d.facing,
        yaw_x: None,
        hit_velocity: None,
        is_item: false,
        has_elem: false,
        charged: d.charge.is_some(),
        attack_id: d.attack_id,
        dead: false,
        age: 0,
        has_hit_a_fighter: false,
    };
    let pct = d.charge.map_or(0.0, |c| c.pct);
    spawned_hit(w, &pr, Some(d), vic, p, &[], at, Some(dir), &[], pct)
}

/// The dealer is an item the stage dropped (`Item::spawner` none), whoever threw it.
fn stage_item(w: &World, pr: &Dealer) -> bool {
    match pr.a {
        Some(ActorRef::Item(id)) => w.actors.items.get(id).is_some_and(|i| i.spawner.is_none()),
        _ => false,
    }
}

/// The port's own Dodgeball rule spares `vic` what the dealer deals: its owner's opponent, unless the dealer is an item
/// the stage dropped.
fn dealer_spares(w: &World, pr: &Dealer, vic: FighterId) -> bool {
    w.setup.dodgeball.is_some()
        && !stage_item(w, pr)
        && pr.owner.is_some_and(|o| crate::world::dodgeball::spares(w, o, vic))
}

/// The body of `applyProjectileHit`, for a dealer of a list (`lit` none) or one no list holds.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
fn spawned_hit(
    w: &mut World,
    pr: &Dealer,
    lit: Option<&PerkDealer>,
    vic: FighterId,
    p: &HitPacket,
    buffs: &[OnHitBuffRow],
    at: V2,
    dir: Option<V2>,
    elementals: &[Name],
    charge_percent: Real,
) -> bool {
    // The port's own Dodgeball rule, behind the tests that pick the victim: no hit, so a projectile is not spent.
    if dealer_spares(w, pr, vic) {
        return false;
    }
    // Not ported: the Lab reset's pass (its end paths act on the old pawns) has no World flag;
    // no match of the sim's reaches it.
    // HitOnlyOneFighter (@3236-3288), ahead of the projectile block (HasHitAFighter, set on the Hit @16069-16144).
    if p.hit_only_one_fighter.unwrap_or(false) && pr.has_hit_a_fighter {
        return false;
    }
    if blocks(w, pr, lit, vic) {
        return false;
    }
    let Some(att) = pr.owner else {
        // An actor with no owner deals pfgDoDamage with no Instigator and no owner, as a level actor's hit.
        // Unverified: the projectile armor, the light armor and the shield tests of DamageAndLaunch (@6113-7268) are
        // not run for it; its knockback direction is the packet's as projectile_hit_dir makes it.
        let vx = w.fighter(vic).body.x;
        let d = dir.unwrap_or_else(|| {
            let hd = projectile_hit_dir(w, pr, p, Some(vx));
            V2 { x: hd.x, y: hd.y }
        });
        let a = pr.a.expect("a dealer no list holds has an owner");
        return apply_level_hit(w, a, pr.cls, vic, p, at, d, elementals);
    };
    let inst = pr.inst.unwrap_or(att);
    hit_buffs_call(w, att, buffs); // pfgDoDamage's entry: the OnHitBuffs rows that need no hit
    // pfgDoDamage @5617-5904: the dealer actor's own ApplyBuffDamageMods over the buffs it carries.
    let mut p = match pr.a {
        Some(a) if pr.has_elem_now(w) => elem::actor_dealt_damage_mods_hit(w, a, ActorRef::Fighter(vic), p, true),
        _ => p.clone(),
    };
    let no_armor = elementals.contains(&crate::name!("Buff_Elemental_NoArmorDamage_C"));
    let shielded = projectile_shielded(w, pr, lit, vic, elementals);
    let failed = armor_fails(w, pr, lit, vic, elementals);
    if shielded && w.fighter(att).team.team != w.fighter(vic).team.team {
        match (pr.a, lit) {
            (Some(a), _) => blocked(w, a, ActorRef::Fighter(vic)),
            // `blocked` of the literal: no class hook, and DestroyOnBlocked is in neither class's data.
            (None, Some(l)) if truthy(&l.p, "DestroyOnBlocked") => {
                crate::fighter::common::warn_once(w, crate::name!("a PerkDealer with DestroyOnBlocked: not ported"));
            }
            _ => {}
        }
    }
    if shielded || failed {
        p.damage = Some(0.0);
        p.base_knock_back = Some(0.0);
        p.hit_pause_time = Some(0.0);
        p.hit_stun_mult = Some(0.0);
    }
    let vic_x = w.fighter(vic).body.x;
    let hd = projectile_hit_dir(w, pr, &p, Some(vic_x));
    // The charge: the actor's own Comp_Pawn_ChargeAttack values, or the literal's (`receiveCharge`).
    let held = match (pr.a, lit) {
        (Some(a), _) => elem::charge_vals(w, a),
        (None, l) => l.and_then(|l| l.charge),
    };
    let ckb = held.map_or(1.0, |c| c.kb);
    let pdmg = p.damage.unwrap_or(0.0);
    let charged = if pr.charged { pdmg * held.map_or(1.0, |c| c.dmg) } else { pdmg };
    let phs = p.hit_stun_mult.unwrap_or(1.0);
    let ar = if shielded || failed {
        crate::combat::defense::CalculateArmorOut { damage: 0.0, kb: 0.0, hs: 0.0 }
    } else if no_armor {
        crate::combat::defense::CalculateArmorOut { damage: charged, kb: hd.bkb, hs: phs }
    } else {
        crate::combat::defense::calculate_armor(
            w,
            vic,
            charged,
            hd.bkb,
            phs,
            ckb,
            p.armor_pierce.unwrap_or(false),
            None,
            Some(at),
        )
    };
    // GetAttackDecayFromHitbox / GetComboResAttackNameFromHitbox: the projectile's class name.
    let dt = enum_int(p.attack_decay_type);
    let dh = DecayHit {
        attack_id: pr.attack_id,
        decay_type: dt,
        decay_name: if [1, 2, 3, 6].contains(&dt) {
            p.attack_decay_name.unwrap_or(crate::name!("None"))
        } else {
            pr.cls
        },
        combo_name: pr.cls,
        base_knock_back: hd.bkb,
        launch_knock_back: Some(ar.kb * ckb),
        additive_kb_mode: p.additive_kb_mode,
        knock_back_scaling: Some(p.knock_back_scaling.unwrap_or(0.0)),
        hit_stun_mult: Some(ar.hs),
    };
    // A buff actor's handler (DealerActor.buffActor) has no attack decay.
    let from_hitbox = !matches!(pr.a, Some(ActorRef::Buff(_))) && !lit.is_some_and(|l| l.buff_actor);
    fh::register_hit_from(w, vic, inst, &dh, from_hitbox, att);
    // The taken conditions' Attacker is the actor; HitCollision its own collision, none for a buff actor's handler
    // and none for a dealer no list holds (a plain packet).
    // Unverified: every class instance is taken as hitting through its own collision.
    let boxed = from_hitbox && lit.is_none();
    // The hit's direction ahead of DamageAndLaunch: the socket step, the caller's, DirectionalKnockback, else
    // projectile_hit_dir (the vacuum replaces it in Calculate Knock Back, below).
    let ds = if !p.vacuum_knockback.unwrap_or(false)
        && p.direct_knockback_towards_socket.unwrap_or(false)
        && socket_exists(w, inst, p.knockback_target_socket)
    {
        Some(direct_socket_dir(w, inst, vic, &p, p.account_for_velocity_with_vacuum_knockback.unwrap_or(true), None))
    } else {
        None
    };
    let vic_ay = fighter::actor_y(w, vic);
    let directional = p.directional_knockback.unwrap_or(false);
    let d0 = if let Some(d) = ds {
        d
    } else if let Some(d) = dir {
        d
    } else if directional {
        V2 { x: projectile_mirror(w, pr, &p, Some(vic_x), vic_x - pr.x), y: vic_ay - pr.y }
    } else {
        V2 { x: hd.x, y: hd.y }
    };
    let x = taken_in(ar.damage, p.knock_back_scaling.unwrap_or(0.0), ar.hs, p.hit_pause_time.unwrap_or(0.0), d0, &p);
    let t = buff::apply_taken_hit_mods(w, vic, x, Some(inst), boxed, pr.a, Some(&p));
    let (bdmg, bscale, bhs) = (t.damage, t.scaling, t.hit_stun);
    let hp = fh::HitParams {
        damage: pdmg,
        base_knock_back: ar.kb,
        knock_back_scaling: bscale,
        scaling_start_dmg: p.kb_scaling_starting_dmg.unwrap_or(0.0),
        hit_pause_time: p.hit_pause_time.unwrap_or(0.0),
        hit_stun_mult: bhs,
        charge_kb_mult: ckb,
        charge_hit_pause_mult: 0.0,
        total_charged_time: 0.0,
        weight: weight(w, w.fighter(vic)),
    };
    let pre = combo_decay_pre(w, Some(att), vic, &dh, bdmg, from_hitbox);
    // Comp_Armor CalculateTempHealthDamage: the victim's temp health packs take their share first.
    let taken = crate::combat::defense::absorb_temp_health(w, vic, pre.dmg);
    let post = taken + w.fighter(vic).stats.damage; // CurrentDamage after ApplyDamage, a float32
    let (mut k, kd) = knock_pair(&hp, post, t.hit_pause);
    // VacuumInfo.UseVacuumKnockBack: as in apply_hit, from the dealer actor.
    let vac = match &p.vacuum_info {
        Some(v) if v.use_vacuum_knock_back.unwrap_or(false) => Some(actor_vacuum(w, pr, vic, v)),
        _ => None,
    };
    let alpha = p.vacuum_info.as_ref().and_then(|v| v.vacuum_strength_alpha).unwrap_or(0.0);
    if let Some(v) = &vac {
        k.dist += (v.dist - k.dist) * alpha;
        k.hitstun = fh::knockback_to_hitstun(k.dist, hp.hit_stun_mult);
        let t = fh::kb_stun_times(k.dist, hp.hit_stun_mult);
        set_kb_stun(&mut k, t);
    }
    let fr = combo_decay_frames(&pre, k.hitstun);
    // The hit's direction: the vacuum, else the taken pass's.
    let V2 { x: dx, y: dy } = match &vac {
        Some(v) => V2 { x: v.vx, y: v.vy },
        None => taken_dir(&t, d0),
    };
    let (nx, ny) = fh::knockback_dir(dx, dy);
    let mut vx = nx * k.speed;
    let mut vy = ny * k.speed;
    if vy == 0.0 && vx != 0.0 {
        vy = 0.001;
    }
    (vx, vy) = keep_pending_launch(w, vic, vx, vy);
    (vx, vy) = buff::apply_knock_back_multipliers(w, vic, vx, vy, true);
    vx *= k.tier;
    vy *= k.tier;
    if let Some(v) = &vac {
        vx += (v.vx - vx) * alpha;
        vy += (v.vy - vy) * alpha;
    }
    let light = ar.kb <= 0.0 && bhs <= 0.0;
    fh::record_damage(w, vic, taken, Some(inst));
    crate::world::flow::update_ffa_team_damage(w, vic, taken);
    // DamageAndLaunch's ApplyDamage (@2591): OnTakeAnyDamage after Comp_Actor_Damage's own handler; the causer is the
    // spawned actor whose hit box hit (its own Comp_Pawn_HitBoxHandler's GetOwner).
    let causer = pr.a.or(pr.owner.map(ActorRef::Fighter));
    crate::world::flow::broadcast_take_any_damage(w, ActorRef::Fighter(vic), pre.dmg, causer);
    if light {
        fh::receive_light_hit(w, vic, taken, crate::real(k.hitpause_frames), k.hitpause_sec);
    } else {
        let kbi = if p.ignore_kbi.unwrap_or(false) { 0.0 } else { p.kbi_multiplier.unwrap_or(1.0) };
        let r = crate::fighter::tick::hit_rumble(enum_real(p.hit_reaction));
        crate::fighter::tick::controller_rumble(w, vic, r); // Calculate Knock Back @4461, as in apply_hit
        let o = fh::ReceiveHitO {
            damage: taken,
            vx,
            vy,
            hitpause_frames: crate::real(k.hitpause_frames),
            hitpause_sec: Some(k.hitpause_sec),
            hitstun_frames: crate::real(fr.hitstun_frames),
            hitstun_sec: Some(fr.hitstun_sec),
            kb_stun_frames: crate::real(k.kb_stun_frames),
            kb_stun_sec: Some(k.kb_stun_sec),
            kbi,
            dir_x: Some(dx),
            dir_y: Some(dy),
            att: Some(inst),
            dealer: pr.a,
            raw_damage: Some(hp.damage),
            base_kb: Some(ar.kb),
            kb_dist: k.dist,
            hit_stun_mult: bhs,
            hit_pause_time: k.hit_pause_time,
        };
        fh::receive_hit(w, vic, &o);
        push_hit_vfx(w, vic, &p, t.effect, at, vx, vy, k.dist);
    }
    // DamageAndLaunch @3723: the DamageDealer is the spawned actor.
    let hb = light_packet(if light { fr.dmg } else { hp.damage }, ar.kb, bhs);
    buff::hit_by_enemy(w, vic, None, hb_dmg(&hb), Some(&hb), pr.a, Some(V2 { x: dx, y: dy }));
    if !light {
        crate::fighter::control::replenish_evade(w, vic, C::EVADE_HIT_REFUND);
    }
    // Hit false (Light Projectile Armor): pfgDoDamage stops at @15812.
    if failed {
        return false;
    }
    if let (Some(a), None) = (pr.a, lit) {
        crate::combat::spawned_hits::dealer_hit_pause(w, a, &p, kd.hitpause_sec); // @16860-17442
    }
    if dealer_p(w, pr, lit).is_some_and(|q| truthy(q, "ApplyHitPauseToOwner")) {
        fh::owner_hit_pause(w, att, crate::real(kd.hitpause_frames), kd.hitpause_sec, p.hit_pause_time.unwrap_or(0.0));
    }
    // HitBoxHandler OnHitBuffs (after the hitpause), then the rows that go to the Instigator on a hit.
    let mult = held.map_or(1.0, |c| c.buff);
    hit_buffs_hit(w, att, vic, buffs, mult, stage_item(w, pr));
    if !light {
        charge_hit_sound(w, vic, charge_percent);
    }
    if let Some(ev) = p.on_hit_audio {
        let zf = if light { None } else { Some(vic_ay + fighter::capsule_half(w, vic) * 0.5) };
        let hit = SoundHit { victim: w.fighter(vic).id, z_floor: zf, charge: None, surface: None, instigator: None };
        w.events.push(Event::Sound {
            fighter: w.fighter(att).id,
            ev,
            x: Some(at.x),
            y: Some(at.y),
            hit: Some(hit),
            pooled: false,
            key: None,
        });
    }
    // ShaggySandwichProjectile HitPlayer: HitEnemyPlayer, then PF_Post_VO_Event(Instigator, ability_sandwich_hit,
    // the hit player as context actor, 0.2).
    if w.names.str(pr.cls).contains("SandwichProjectile") {
        let alias = crate::name!("ability_sandwich_hit");
        crate::fighter::match_side::post_vo(w, att, alias, Some(vic), 0.2);
    }
    push_hit_event(w, w.fighter(att).id, vic, &fr, &p, &k, light, vx, vy);
    if p.hit_only_one_fighter.unwrap_or(false) {
        pr.set_has_hit_a_fighter(w); // @16069-16144
    }
    // pfgDoDamage @22407-22653: BFHitPlayer on the handler's owner's owner, ahead of HitAnything: for a buff actor its
    // wearer (SpawnBuffActor @5 LocalBuffOwner = Owner, @710), else the fighter that owns the actor.
    // Unverified: an actor owned by another spawned actor is taken as owned by its fighter (the Dealer's owner).
    // A dealer no list holds: a buff actor on a spawned actor of the owner's (DistanceShot's ball), or the owner's own
    // spawned actor (the KO bounce explosion).
    let owner = match pr.a {
        Some(ActorRef::Buff(b)) => b.fighter,
        _ => att,
    };
    crate::world::catch_up::hit_player(w, owner);
    match (pr.a, lit) {
        (Some(a), _) => hit_anything(w, a, ActorRef::Fighter(vic)),
        // `hitAnything` of the literal: no buff list, no class hook, and DestroyedOnHit is in neither class's data
        // as true.
        (None, Some(l)) if truthy(&l.p, "DestroyedOnHit") => {
            crate::fighter::common::warn_once(w, crate::name!("a PerkDealer with DestroyedOnHit: not ported"));
        }
        _ => {}
    }
    true
}

/// The hit packet `hitByEnemy` passes (BuffSignalArg.hit): Damage, BaseKnockBack and HitStunMult.
fn light_packet(damage: Real, bkb: Real, hs: Real) -> HitPacket {
    HitPacket { damage: Some(damage), base_knock_back: Some(bkb), hit_stun_mult: Some(hs), ..HitPacket::default() }
}

/// The Damage HitByEnemy's packet carries to the team's pawns (Comp_C017_Hover_Rage AllyHitRageBuild): the light
/// path's damage after the mods and the clamp (@2499), a launching hit's LocalUnmodifiedDamage (@8777, set @7398
/// ahead of the charge multiplier and CalculateArmor).
fn hb_dmg(hb: &HitPacket) -> Real {
    hb.damage.unwrap_or(0.0)
}

fn set_kb_stun(k: &mut fh::KnockResult, t: fh::KbStunTimesOut) {
    // Object.assign(k, kbStunTimes(..)): the frames are a timer count (an integer)
    k.kb_stun_frames = crate::math::to_int32(t.kb_stun_frames);
    k.kb_stun_sec = t.kb_stun_sec;
}

/// The `hit` event (HitReactionMulticast's view side, or LightHitReactionMulticast's).
#[allow(clippy::too_many_arguments, reason = "the call carries every field of the hit event")]
fn push_hit_event(
    w: &mut World,
    attacker: i32,
    vic: FighterId,
    fr: &DecayFrames,
    p: &HitPacket,
    k: &fh::KnockResult,
    light: bool,
    vx: Real,
    vy: Real,
) {
    let v = w.fighter(vic);
    let hp = crate::real(k.hitpause_frames);
    let e = Event::Hit {
        attacker,
        victim: v.id,
        damage: fr.dmg,
        // PlayHitSound: the light path passes the applied damage, the launch path the hit's own
        pain_damage: if light { fr.dmg } else { p.damage.unwrap_or(0.0) },
        speed: if light { 0.0 } else { ue::size2d([vx, vy]) },
        hitpause: hp,
        lockout: hp + if light { 0.0 } else { crate::real(fr.hitstun_frames) },
        x: v.body.x,
        y: v.body.y,
        light,
    };
    w.events.push(e);
}

/// `applyLevelHit`: a level actor's own pfgDoDamage(null, vic, p, null, elementals) with
/// no Instigator behind it: DamageAndLaunch with the actor as DamageDealer, with what a fighter behind the dealer
/// would add left out (no RegisterHitFromFighter decay, no attack decay, no DamageDealt, no dealt multipliers, no
/// OnHitBuffs, no dealer hit pause). `dir`: the packet's direction in world terms; `at`: the hit location. True:
/// Hit. Then the packet's ElementalBuffs: each class's ElementalHitEnemy on the victim.
/// Unverified: Buff_Fire's Instigator is taken as none; only Buff_Elemental_Fire is ported here.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn apply_level_hit(
    w: &mut World,
    dealer: ActorRef,
    cls: Name,
    vic: FighterId,
    p: &HitPacket,
    at: V2,
    dir: V2,
    elementals: &[Name],
) -> bool {
    // Not ported: the Lab reset's pass (as apply_projectile_hit).
    let bkb = p.base_knock_back.unwrap_or(0.0);
    let pdmg = p.damage.unwrap_or(0.0);
    let ar = crate::combat::defense::calculate_armor(
        w,
        vic,
        pdmg,
        bkb,
        p.hit_stun_mult.unwrap_or(1.0),
        1.0,
        p.armor_pierce.unwrap_or(false),
        None,
        Some(at),
    );
    let dt = enum_int(p.attack_decay_type);
    let dh = DecayHit {
        attack_id: p.attack_id.unwrap_or(0),
        decay_type: dt,
        decay_name: if [1, 2, 3, 6].contains(&dt) { p.attack_decay_name.unwrap_or(crate::name!("None")) } else { cls },
        combo_name: cls,
        base_knock_back: bkb,
        launch_knock_back: Some(ar.kb),
        additive_kb_mode: p.additive_kb_mode,
        knock_back_scaling: Some(p.knock_back_scaling.unwrap_or(0.0)),
        hit_stun_mult: Some(ar.hs),
    };
    fh::register_level_hit(w, vic, dealer, &dh);
    let x = taken_in(ar.damage, p.knock_back_scaling.unwrap_or(0.0), ar.hs, p.hit_pause_time.unwrap_or(0.0), dir, p);
    let t = buff::apply_taken_hit_mods(w, vic, x, None, false, Some(dealer), Some(p));
    let (bdmg, bscale, bhs) = (t.damage, t.scaling, t.hit_stun);
    let dir = taken_dir(&t, dir);
    let hp = fh::HitParams {
        damage: pdmg,
        base_knock_back: ar.kb,
        knock_back_scaling: bscale,
        scaling_start_dmg: p.kb_scaling_starting_dmg.unwrap_or(0.0),
        hit_pause_time: p.hit_pause_time.unwrap_or(0.0),
        hit_stun_mult: bhs,
        charge_kb_mult: 1.0,
        charge_hit_pause_mult: 0.0,
        total_charged_time: 0.0,
        weight: weight(w, w.fighter(vic)),
    };
    let pre = combo_decay_pre(w, None, vic, &dh, bdmg, false);
    let taken = crate::combat::defense::absorb_temp_health(w, vic, pre.dmg);
    let post = taken + w.fighter(vic).stats.damage;
    let (k, _) = knock_pair(&hp, post, t.hit_pause);
    let fr = combo_decay_frames(&pre, k.hitstun);
    let (nx, ny) = fh::knockback_dir(dir.x, dir.y);
    let mut vx = nx * k.speed;
    let mut vy = ny * k.speed;
    if vy == 0.0 && vx != 0.0 {
        vy = 0.001;
    }
    (vx, vy) = keep_pending_launch(w, vic, vx, vy);
    (vx, vy) = buff::apply_knock_back_multipliers(w, vic, vx, vy, true);
    vx *= k.tier;
    vy *= k.tier;
    let light = ar.kb <= 0.0 && bhs <= 0.0;
    fh::record_damage(w, vic, taken, None);
    crate::world::flow::update_ffa_team_damage(w, vic, taken);
    crate::world::flow::broadcast_take_any_damage(w, ActorRef::Fighter(vic), pre.dmg, Some(dealer));
    if light {
        fh::receive_light_hit(w, vic, taken, crate::real(k.hitpause_frames), k.hitpause_sec);
    } else {
        let kbi = if p.ignore_kbi.unwrap_or(false) { 0.0 } else { p.kbi_multiplier.unwrap_or(1.0) };
        let r = crate::fighter::tick::hit_rumble(enum_real(p.hit_reaction));
        crate::fighter::tick::controller_rumble(w, vic, r);
        let o = fh::ReceiveHitO {
            damage: taken,
            vx,
            vy,
            hitpause_frames: crate::real(k.hitpause_frames),
            hitpause_sec: Some(k.hitpause_sec),
            hitstun_frames: crate::real(fr.hitstun_frames),
            hitstun_sec: Some(fr.hitstun_sec),
            kb_stun_frames: crate::real(k.kb_stun_frames),
            kb_stun_sec: Some(k.kb_stun_sec),
            kbi,
            dir_x: Some(dir.x),
            dir_y: Some(dir.y),
            att: None,
            dealer: Some(dealer),
            raw_damage: Some(hp.damage),
            base_kb: Some(ar.kb),
            kb_dist: k.dist,
            hit_stun_mult: bhs,
            hit_pause_time: k.hit_pause_time,
        };
        fh::receive_hit(w, vic, &o);
        push_hit_vfx(w, vic, p, t.effect, at, vx, vy, k.dist);
    }
    let hb = light_packet(if light { fr.dmg } else { hp.damage }, ar.kb, bhs);
    buff::hit_by_enemy(w, vic, None, hb_dmg(&hb), Some(&hb), Some(dealer), Some(dir));
    if !light {
        crate::fighter::control::replenish_evade(w, vic, C::EVADE_HIT_REFUND);
    }
    push_hit_event(w, -1, vic, &fr, p, &k, light, vx, vy);
    let fire = crate::name!("Buff_Elemental_Fire_C");
    for &e in elementals {
        if e != fire {
            crate::fighter::common::warn_once(w, crate::name!("level actor hit: ElementalBuffs not ported"));
        } else {
            let cls = crate::name!("Buff_Fire_C");
            let stacks = crate::fighter::common::buff_def(w, ActorRef::Fighter(vic), fire)
                .and_then(|d| d.get("SpawnedStackCount"))
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            // spawnBuff(vic, 'Buff_Fire_C', 0, stacks): no Instigator, no caller
            buff::spawn_buff(w, vic, cls, 0.0, Some(crate::math::dbl::to_int32(stacks)), None, false, None);
        }
    }
    true
}

/// `hitAnything`: the dealer's handler broadcasts HitAnything (pfgDoDamage @22208) once a
/// hit went through: what a buff child of the actor has bound (Buff_SlowProjectile ends, the projectile buffs that
/// bound it), then the actor's own OnHitAnything: the class's override, else the parent's (@18379: with
/// DestroyedOnHit, Delay(0.033) -> DestroySpawnedActor(false)), which the override may call first.
/// The Delay counts the hit's frame when the hit came ahead of the actor's tick, not when it came from the world's
/// test behind a projectile's tick.
/// Unverified: an item's or a World.actors actor's Delay counts from its next tick whatever the hit's place in the
/// frame; the order between the buff children's binds and the actor's own on the one delegate is not read.
pub fn hit_anything(w: &mut World, a: ActorRef, victim: ActorRef) {
    let (has_elem, dead, cls, age) = match a {
        ActorRef::Projectile(id) => w
            .actors
            .projectiles
            .get(id)
            .map_or((false, true, Name::NONE, 0), |p| (p.elem.is_some(), p.dead, p.cls, p.age)),
        ActorRef::Item(id) => {
            w.actors.items.get(id).map_or((false, true, Name::NONE, 0), |i| (i.elem.is_some(), i.dead, i.cls, i.age))
        }
        ActorRef::Actor(id) => {
            w.actors.actors.get(id).map_or((false, true, Name::NONE, 0), |x| (x.elem.is_some(), x.dead, x.cls, 0))
        }
        ActorRef::Fighter(_) => return,
        // A buff actor's own handler (DealerActor.buffActor): no buff list, no class override; the data of the
        // classes ported (`buff_actor_dealer`) has no DestroyedOnHit.
        ActorRef::Buff(b) => {
            if crate::combat::buffs::buff(w, b).is_some_and(|x| !buff_actor_dealer(x.cls)) {
                crate::unported!("hits: hitAnything of this buff actor class", w, a, victim);
            }
            return;
        }
        // Morty's marker and mortar aim: no own hook (their classes have no DestroyedOnHit either).
        ActorRef::Marker(id) => {
            w.actors.markers.get(id).map_or((false, true, Name::NONE, 0), |x| (x.elem.is_some(), x.dead, x.cls, x.age))
        }
        ActorRef::Mortar(id) => {
            w.actors.mortars.get(id).map_or((false, true, Name::NONE, 0), |x| (x.elem.is_some(), x.dead, x.cls, x.age))
        }
        _ => crate::unported!("hits: hitAnything of World.actors or a typed list", w, a, victim),
    };
    if has_elem {
        elem::slow_hit_anything(w, a);
        elem::actor_buff_signal(w, a, ActorBuffSignal::HitAnything, None);
    }
    let dead = dead
        || match a {
            ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_none_or(|p| p.dead),
            ActorRef::Item(id) => w.actors.items.get(id).is_none_or(|i| i.dead),
            ActorRef::Actor(id) => w.actors.actors.get(id).is_none_or(|x| x.dead),
            ActorRef::Marker(id) => w.actors.markers.get(id).is_none_or(|x| x.dead),
            ActorRef::Mortar(id) => w.actors.mortars.get(id).is_none_or(|x| x.dead),
            _ => false,
        };
    if dead {
        return;
    }
    // The class's own OnHitAnything: a World actor's, a projectile's or an item's kind hook. A projectile or item
    // class listed in OWN_HIT_ANYTHING overrides it and has no hook yet.
    let kind = w.registry.actors.get(w.names.str(cls));
    let own = kind.and_then(|e| match &e.hooks {
        ActorHooks::World(h) => h.on_hit_anything.map(OwnHitAnything::World),
        ActorHooks::Projectile(h) => h.hit_anything.map(OwnHitAnything::Projectile),
        ActorHooks::Item(h) => h.hit_anything.map(OwnHitAnything::Item),
    });
    if own.is_none()
        && matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_))
        && OWN_HIT_ANYTHING.contains(&w.names.str(cls))
    {
        crate::unported!("hits: hitAnything: a projectile or item kind's onHitAnything", w, a, victim)
    }
    let calls_parent = kind.is_some_and(|e| e.calls_parent_hit_anything);
    let owner = match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).and_then(|p| p.owner.or(p.lookup_fighter)),
        ActorRef::Item(id) => w.actors.items.get(id).and_then(|i| i.owner.or(i.lookup_fighter)),
        ActorRef::Actor(id) => w.actors.actors.get(id).map(|x| x.owner),
        ActorRef::Marker(id) => w.actors.markers.get(id).map(|x| x.owner),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).map(|x| x.owner),
        _ => None,
    };
    let destroyed_on_hit = class_def(w, owner, cls).is_some_and(|d| truthy(&d.p, "DestroyedOnHit"));
    if (own.is_none() || calls_parent) && destroyed_on_hit {
        let behind = matches!(a, ActorRef::Projectile(_)) && !w.overlap_mid_frame && age > 0;
        let frames = crate::fighter::common::latent_frames(0.033) - if behind { 1.0 } else { 0.0 };
        crate::world::spawning::destroy_spawned_actor(w, a, frames);
    }
    match (own, a) {
        (Some(OwnHitAnything::World(h)), ActorRef::Actor(id)) => h(w, id, victim),
        (Some(OwnHitAnything::Projectile(h)), ActorRef::Projectile(id))
            if w.actors.projectiles.get(id).is_some_and(|p| !p.dead) =>
        {
            h(w, id, victim);
        }
        (Some(OwnHitAnything::Item(h)), ActorRef::Item(id)) if w.actors.items.get(id).is_some_and(|i| !i.dead) => {
            h(w, id, victim);
        }
        _ => {}
    }
}

/// A kind's own OnHitAnything, by the family of its hooks.
#[derive(Clone, Copy)]
enum OwnHitAnything {
    World(fn(&mut World, ActorId, ActorRef)),
    Projectile(fn(&mut World, ProjectileId, ActorRef)),
    Item(fn(&mut World, ItemId, ActorRef)),
}

/// The projectile and item classes whose OnHitAnything overrides SpawnedActorParent's (Dynamite and CantSplitDynamite, JerryCork).
const OWN_HIT_ANYTHING: [&str; 3] = ["Dynamite_SpawnedActor_C", "CantSplitDynamite_SpawnedActor_C", "JerryCork_C"];

/// `pushHitVfx`: HitReactionMulticast's PlayHitVFX inputs (knockback path only).
/// HitLocation is the overlapping hitbox sphere's centre (a projectile: its location), with Z raised to at least
/// actorZ + CapsuleHalfHeight * 0.5. The trail block: |launch| > 1000 and not ForceSmallHitEffects with
/// KnockBackDistance >= 3800 and the victim in hitstun enables HitSmokeTrail. `effect`: OnHitEffectOverride as the
/// taken pass left it (HitReactionMulticast @9984).
#[allow(clippy::too_many_arguments, reason = "the multicast's inputs")]
fn push_hit_vfx(
    w: &mut World,
    vic: FighterId,
    p: &HitPacket,
    effect: Option<Name>,
    at: V2,
    vx: Real,
    vy: Real,
    dist: Real,
) {
    let force_small = p
        .vacuum_info
        .as_ref()
        .is_some_and(|v| v.use_vacuum_knock_back.unwrap_or(false) && v.force_small_hit_effects.unwrap_or(false));
    let id = w.fighter(vic).id;
    if ue::size2d([vx, vy]) > 1000.0 && !force_small && dist >= 3800.0 && w.fighter(vic).hit.hitstun > 0.0 {
        w.events.push(Event::SmokeTrail { fighter: id, on: true });
        let ev = crate::name!("sfx_shared_combat_smoketrail_start");
        w.events.push(Event::Sound { fighter: id, ev, x: None, y: None, hit: None, pooled: false, key: None });
    }
    if !p.play_on_hit_effects.unwrap_or(true) {
        return;
    }
    let z_floor = fighter::actor_y(w, vic) + fighter::capsule_half(w, vic) * 0.5;
    let v = w.fighter(vic);
    let decay_stacks = v.attackers.last_decay_stacks;
    let override_vfx = effect.filter(|n| *n != Name::NONE && !w.names.str(*n).is_empty());
    w.events.push(Event::HitVfx {
        victim: id,
        x: at.x,
        y: at.y,
        z_floor,
        vx,
        vy,
        dist,
        decay_stacks,
        override_vfx,
        force_small,
    });
}

/// `actorVacuum`: CalculateVacuumKnockback with a spawned actor as the dealer
/// (VacuumTowardsInstigatingFighter off): target = the actor location + (0, offset X, offset Y) [+ its velocity x
/// VacuumDuration with AccountForAttackerVelocity]. With the flag the Instigator's fighter rule (vacuum_knockback).
fn actor_vacuum(w: &mut World, pr: &Dealer, vic: FighterId, v: &VacuumInfo) -> VacuumKnockbackOut {
    if v.vacuum_towards_instigating_fighter.unwrap_or(false) {
        if let Some(o) = pr.owner {
            return vacuum_knockback(w, pr.inst.unwrap_or(o), vic, v, None); // DamageDealer.Instigator
        }
        crate::fighter::common::warn_once(w, crate::name!("vacuum toward a missing instigator"));
    }
    let dur = v.vacuum_duration.unwrap_or(0.0);
    let (ox, oy) = v.vacuum_socket_offset.map_or((0.0, 0.0), |o| (o.x.unwrap_or(0.0), o.y.unwrap_or(0.0)));
    let mut tx = pr.x - ox;
    let mut ty = pr.y + oy;
    if v.account_for_attacker_velocity.unwrap_or(false) {
        tx += pr.vx * dur;
        ty += pr.vy * dur;
    }
    let vb = &w.fighter(vic).body;
    let (ex, ey) = (tx - vb.x, ty - vb.y);
    VacuumKnockbackOut {
        dist: ue::size2d([ex, ey]),
        vx: if dur != 0.0 { ex / dur } else { 0.0 },
        vy: if dur != 0.0 { ey / dur } else { 0.0 },
    }
}

/// `comboDecayPre`: GetHitStunDecay (DamageAndLaunch @2059, before ApplyDamage): the
/// applied damage x DamageDecayMod when the mod is below 1; then Damage = FClamp(Damage, 0, 10000) before ApplyDamage
/// @2591. `from_hitbox` false: a buff actor's handler (no attack decay, no HitDecayUIMulticast); `att` None: a level
/// actor's own hit.
pub fn combo_decay_pre(
    w: &mut World,
    att: Option<FighterId>,
    vic: FighterId,
    h: &DecayHit,
    damage: Real,
    from_hitbox: bool,
) -> ComboDecayPreOut {
    let penalty = w.flow.same_attack_penalty;
    let m = fh::hit_stun_decay(w, vic, att, h, from_hitbox && penalty);
    // DestroyDodgePopTextMulticast @3208, behind ApplyDamage @2591: sent from here, the one place both
    // DamageAndLaunch ports pass.
    let vid = w.fighter(vic).id;
    w.events.push(Event::PopText { fighter: vid, kind: PopKind::DodgedOff, stacks: 0.0, gain: None, params: None });
    // HitDecayUIMulticast (mod below 1): with AttackDecayEnabled and the same attack penalty and at least 4 stacks,
    // PopText on the source fighter.
    let stacks = w.fighter(vic).attackers.last_decay_stacks;
    if let Some(a) = att
        && from_hitbox
        && m < 1.0
        && penalty
        && stacks >= 4.0
    {
        let aid = w.fighter(a).id;
        w.events.push(Event::PopText { fighter: aid, kind: PopKind::AttackDecay, stacks, gain: None, params: None });
    }
    // Multiply_FloatFloat(Damage, DamageDecayMod) @2309 and the FClamp @2499 are float32.
    let d = if m < 1.0 { damage * C::DAMAGE_DECAY_MOD } else { damage };
    ComboDecayPreOut { dmg: min(max(d, 0.0), 10000.0), modifier: m }
}

/// `comboDecayFrames`'s answer.
#[derive(Clone, Copy, Debug)]
struct DecayFrames {
    dmg: Real,
    hitstun_frames: i32,
    /// The float AddDecayedHitStun compares with a running HitstunTimer's remaining time.
    hitstun_sec: Real,
}

fn combo_decay_frames(pre: &ComboDecayPreOut, hitstun: Real) -> DecayFrames {
    let sec = hitstun * min(pre.modifier, 1.0);
    DecayFrames { dmg: pre.dmg, hitstun_frames: fh::timer_count(sec), hitstun_sec: sec }
}

/// Calculate Knock Back, both paths: per axis the new launch only replaces a pending KnockbackLaunchVelocity (a hit
/// inside the victim's hitpause) when its magnitude is larger.
fn keep_pending_launch(w: &World, vic: FighterId, vx: Real, vy: Real) -> (Real, Real) {
    match pending_launch(w.fighter(vic)) {
        None => (vx, vy),
        Some((px, py)) => (if vx.abs() > px.abs() { vx } else { px }, if vy.abs() > py.abs() { vy } else { py }),
    }
}

/// The room of [`socket_key`]'s buffer.
const SOCKET_KEY: usize = 128;

/// The box data's key of a socket name, `socket:` and the name in lower case, written to `buf`; `None` for a name
/// that is not ASCII or does not fit (the caller builds the key then).
fn socket_key<'b>(name: &str, buf: &'b mut [u8; SOCKET_KEY]) -> Option<&'b str> {
    const P: &[u8] = b"socket:";
    let n = P.len() + name.len();
    if n > SOCKET_KEY || !name.is_ascii() {
        return None;
    }
    buf[..P.len()].copy_from_slice(P);
    for (d, c) in buf[P.len()..n].iter_mut().zip(name.bytes()) {
        *d = c.to_ascii_lowercase();
    }
    std::str::from_utf8(&buf[..n]).ok()
}

/// `vacuumSocket`: the point a vacuum pulls to: the dealer's Mesh socket when it exists,
/// else its actor location. None: no socket. Root: its `socket:Root` point of the pose where the box data samples
/// it, the mesh origin where it does not. Another name: its `socket:<name>` point of the pose the fighter is in at
/// the hit (`open`: the pose the Hitframe opened in). A name the data does not hold warns and takes the actor
/// location.
/// Unverified: no native read backs the socket half of the `open` rule; it follows the shape test's.
pub fn vacuum_socket(w: &World, att: FighterId, name: Option<Name>, what: &str, open: Option<&OpenPose>) -> V2 {
    let f = w.fighter(att);
    let raw = name.map_or("None", |n| w.names.str(n));
    // an FName: no case
    let mut buf = [0u8; SOCKET_KEY];
    let built;
    let key = match socket_key(raw, &mut buf) {
        Some(k) => k,
        None => {
            built = format!("socket:{}", raw.to_lowercase());
            built.as_str()
        }
    };
    let sock = &key["socket:".len()..];
    if sock == "none" {
        return V2 { x: f.body.x, y: f.body.y };
    }
    let bs = &w.fixed.samplers[&f.ch];
    let cd = char_data(w, f);
    let i = bs.by_lower.get(key).copied();
    if sock == "root" && i.is_none() {
        return V2 { x: mesh_wx(f), y: mesh_wy(f) + crate::math::narrow(cd.boxes.mesh_z) };
    }
    // (a shape marked missing: no socket or bone has the name, DoesSocketExist is false)
    if let Some(i) = i
        && cd.boxes.shapes[i as usize].missing.unwrap_or(false)
    {
        return V2 { x: f.body.x, y: f.body.y };
    }
    let Some(i) = i else {
        let _ = what;
        crate::fighter::common::warn_once(w, crate::name!("socket not modelled"));
        return V2 { x: f.body.x, y: f.body.y };
    };
    let o = bs.offsets[i as usize] as usize;
    let pose = match open {
        // att.poseBoxes(open.anim, open.time, w.obuf), else att.sampleBoxes()
        Some(op) => crate::fighter::anim::posed_boxes(w, att, op.anim, op.time),
        None => crate::fighter::anim::sampled_boxes(w, att),
    };
    let pt = pose.map(|p| {
        let pt = (p[o], p[o + 1]);
        crate::fighter::anim::put_pose(w, p);
        pt
    });
    let Some((px, py)) = pt else {
        crate::fighter::common::warn_once(w, crate::name!("socket not modelled"));
        return V2 { x: f.body.x, y: f.body.y };
    };
    V2 { x: mesh_wx(f) + crate::real(px) * f.body.facing_dir, y: mesh_wy(f) + crate::real(py) }
}

/// `socketExists`: the dealer's active mesh DoesSocketExist(name) (1429b12c0): a socket
/// or a bone of the mesh. None is neither; Root and every name the box data samples exist, but a shape the data
/// marks missing does not.
/// Unverified: a name the box data does not sample is taken as missing (warned): the port holds no full bone list.
fn socket_exists(w: &World, att: FighterId, name: Option<Name>) -> bool {
    let raw = name.map_or("None", |n| w.names.str(n));
    let mut buf = [0u8; SOCKET_KEY];
    let built;
    let key = match socket_key(raw, &mut buf) {
        Some(k) => k,
        None => {
            built = format!("socket:{}", raw.to_lowercase());
            built.as_str()
        }
    };
    let sock = &key["socket:".len()..];
    if sock == "none" {
        return false;
    }
    if sock == "root" {
        return true;
    }
    let f = w.fighter(att);
    if let Some(&i) = w.fixed.samplers[&f.ch].by_lower.get(key) {
        return !char_data(w, f).boxes.shapes[i as usize].missing.unwrap_or(false);
    }
    crate::fighter::common::warn_once(w, crate::name!("socket not modelled (taken as missing)"));
    false
}

/// `vacuumKnockback`: DamageManager CalculateVacuumKnockback (melee: the dealer is the
/// fighter). Target = the dealer's mesh socket VacuumTargetSocket + VacuumSocketOffset (X toward the dealer's
/// facing, Y up) [+ dealer velocity x VacuumDuration with AccountForAttackerVelocity]; the vacuum velocity covers
/// victim -> target in VacuumDuration. Divide_VectorFloat by 0 returns a zero vector.
pub fn vacuum_knockback(
    w: &World,
    att: FighterId,
    vic: FighterId,
    v: &VacuumInfo,
    open: Option<&OpenPose>,
) -> VacuumKnockbackOut {
    let at = vacuum_socket(w, att, v.vacuum_target_socket, "vacuum", open);
    let a = w.fighter(att);
    let (ox, oy) = v.vacuum_socket_offset.map_or((0.0, 0.0), |o| (o.x.unwrap_or(0.0), o.y.unwrap_or(0.0)));
    let mut tx = at.x + ox * a.body.facing_dir;
    let mut ty = at.y + oy;
    let dur = v.vacuum_duration.unwrap_or(0.0);
    if v.account_for_attacker_velocity.unwrap_or(false) {
        tx += a.body.vx * dur;
        ty += a.body.vy * dur;
    }
    let vb = &w.fighter(vic).body;
    let (ex, ey) = (tx - vb.x, ty - vb.y);
    VacuumKnockbackOut {
        dist: ue::size2d([ex, ey]),
        vx: if dur != 0.0 { ex / dur } else { 0.0 },
        vy: if dur != 0.0 { ey / dur } else { 0.0 },
    }
}

/// `blocked`: SpawnedActorParent Blocked(BlockingActor): without DestroyOnBlocked
/// nothing; with it Delay(0.0) then DestroySpawnedActor(false). A class with its own Blocked runs that. The
/// Delay(0.0) of a projectile ends in its next tick (blocked_destroy).
pub fn blocked(w: &mut World, pr: ActorRef, by: ActorRef) {
    match pr {
        ActorRef::Projectile(id) => {
            let (cls, owner) =
                w.actors.projectiles.get(id).map(|p| (p.cls, p.owner.or(p.lookup_fighter))).expect("a live projectile");
            let own = w.registry.actors.get(w.names.str(cls)).and_then(|e| match &e.hooks {
                ActorHooks::Projectile(h) => h.blocked,
                _ => None,
            });
            if let Some(h) = own {
                h(w, id, by);
            } else if class_def(w, owner, cls).is_some_and(|d| truthy(&d.p, "DestroyOnBlocked")) {
                w.actors.projectiles.get_mut(id).expect("a live projectile").blocked_destroy = true;
            }
        }
        ActorRef::Actor(id) => {
            let cls = w.actors.actors.get(id).expect("a live actor").cls;
            let own = w.registry.actors.get(w.names.str(cls)).and_then(|e| match &e.hooks {
                ActorHooks::World(h) => h.blocked,
                _ => None,
            });
            match own {
                Some(h) => h(w, id, by),
                None => {
                    // the parent's Delay(0.0) and DestroySpawnedActor
                    let x = w.actors.actors.get(id).expect("a live actor");
                    let (cls, owner) = (x.cls, x.owner);
                    if class_def(w, Some(owner), cls).is_some_and(|d| truthy(&d.p, "DestroyOnBlocked")) {
                        let frames = crate::fighter::common::latent_frames(0.0);
                        crate::world::spawning::destroy_spawned_actor(w, pr, frames);
                    }
                }
            }
        }
        ActorRef::Accelerator(id) => {
            let (cls, owner) = w.actors.accelerators.get(id).map(|a| (a.cls, a.owner)).expect("a live portal");
            if class_def(w, Some(owner), cls).is_some_and(|d| truthy(&d.p, "DestroyOnBlocked")) {
                let frames = crate::fighter::common::latent_frames(0.0);
                crate::world::spawning::destroy_spawned_actor(w, pr, frames);
            }
        }
        ActorRef::Item(id) => {
            let (cls, owner) =
                w.actors.items.get(id).map(|i| (i.cls, i.owner.or(i.lookup_fighter))).expect("a live item");
            if class_def(w, owner, cls).is_some_and(|d| truthy(&d.p, "DestroyOnBlocked")) {
                let frames = crate::fighter::common::latent_frames(0.0);
                crate::world::spawning::destroy_spawned_actor(w, pr, frames);
            }
        }
        ActorRef::Marker(_) | ActorRef::Mortar(_) => {
            let (cls, owner) = match pr {
                ActorRef::Marker(id) => w.actors.markers.get(id).map(|x| (x.cls, x.owner)).expect("a live marker"),
                ActorRef::Mortar(id) => w.actors.mortars.get(id).map(|x| (x.cls, x.owner)).expect("a live mortar"),
                _ => unreachable!("a marker or a mortar aim"),
            };
            if class_def(w, Some(owner), cls).is_some_and(|d| truthy(&d.p, "DestroyOnBlocked")) {
                let frames = crate::fighter::common::latent_frames(0.0);
                crate::world::spawning::destroy_spawned_actor(w, pr, frames);
            }
        }
        _ => crate::unported!("hits: blocked: a typed list's actor", w, pr, by),
    }
}

/// `hitBuffsCall`: pfgDoDamage's OnHitBuffs rows at the entry of every call (@842-1265,
/// @7142-7380): a row with ApplyBuffSelf?, unless it has ApplyBuffHit? and RequireHitToApplyBuffToSelf, goes to the
/// Instigator; then (@1465-2821) a row with ApplyBuffAlly? and without ApplyBuffHit? goes to every other fighter of
/// the Instigator's TeamIndex.
/// Deviation: the build runs these two per call, one call per begin-overlap event of a pair of components; the port
/// has one call per Hitframe and actor (its hit set), made ahead of the dealer's hitstun and HitOnlyOneFighter
/// gates, and for an invulnerable enemy once per begin-overlap the dodge record sees.
pub fn hit_buffs_call(w: &mut World, src: FighterId, rows: &[OnHitBuffRow]) {
    for e in rows {
        if e.apply_buff_self && !(e.apply_buff_hit && e.require_hit_to_apply_buff_to_self) {
            buff::spawn_buff(
                w,
                src,
                e.applied_buffs,
                e.buff_duration.unwrap_or(0.0),
                Some(e.stack_count.unwrap_or(1)),
                Some(src),
                false,
                Some(src),
            );
        }
    }
    for e in rows {
        if e.apply_buff_hit || !e.apply_buff_ally {
            continue;
        }
        let team = w.fighter(src).team.team;
        let mut nf = 0;
        while let Some(o) = fighter_at(w, nf) {
            nf += 1;
            if o != src && w.fighter(o).team.team == team {
                buff::spawn_buff(
                    w,
                    o,
                    e.applied_buffs,
                    e.buff_duration.unwrap_or(0.0),
                    Some(e.stack_count.unwrap_or(1)),
                    Some(src),
                    false,
                    Some(src),
                );
            }
        }
    }
}

/// Round(StackCount x the multiplier) as `spawnBuff` is given its stack count (the Round node: RoundToInt).
pub(crate) fn stacks_times(e: &OnHitBuffRow, mult: Real) -> i32 {
    ue::round_to_int(crate::real(e.stack_count.unwrap_or(1)) * mult)
}

/// `hitBuffsAlly`: behind the gates and ahead of DamageAndLaunch (@5931-6835): a row with
/// ApplyBuffHit? and ApplyBuffAlly? goes to a hit actor of the Instigator's team that is not the Instigator, with
/// Round(StackCount x GetChargeBuffStackMultiplier) stacks.
pub fn hit_buffs_ally(w: &mut World, src: FighterId, vic: FighterId, rows: &[OnHitBuffRow], mult: Real) {
    if vic == src || w.fighter(vic).team.team != w.fighter(src).team.team {
        return;
    }
    for e in rows {
        if e.apply_buff_hit && e.apply_buff_ally {
            buff::spawn_buff(
                w,
                vic,
                e.applied_buffs,
                e.buff_duration.unwrap_or(0.0),
                Some(stacks_times(e, mult)),
                Some(src),
                false,
                Some(src),
            );
        }
    }
}

/// `hitBuffsHit`: after DamageAndLaunch returned Hit and the hit pause, the teams
/// differing: every row with ApplyBuffHit? and ApplyBuffEnemy? goes to the hit actor (@18293-19121); then every row
/// with ApplyBuffHit?, ApplyBuffSelf? and RequireHitToApplyBuffToSelf goes to the Instigator (@23297-24169).
/// `stage_item`: the dealer is an item the stage dropped, whose rows the port's own Dodgeball rule does not spare.
fn hit_buffs_hit(w: &mut World, src: FighterId, vic: FighterId, rows: &[OnHitBuffRow], mult: Real, stage_item: bool) {
    if w.fighter(vic).team.team == w.fighter(src).team.team {
        return;
    }
    let spawn = if stage_item { buff::spawn_buff_unspared } else { buff::spawn_buff };
    for e in rows {
        if e.apply_buff_hit && e.apply_buff_enemy {
            spawn(
                w,
                vic,
                e.applied_buffs,
                e.buff_duration.unwrap_or(0.0),
                Some(stacks_times(e, mult)),
                Some(src),
                false,
                Some(src),
            );
        }
    }
    for e in rows {
        if e.apply_buff_hit && e.apply_buff_self && e.require_hit_to_apply_buff_to_self {
            buff::spawn_buff(
                w,
                src,
                e.applied_buffs,
                e.buff_duration.unwrap_or(0.0),
                Some(e.stack_count.unwrap_or(1)),
                Some(src),
                false,
                Some(src),
            );
        }
    }
}

/// The knockback target of a legacy vacuum or a socket step: the socket + the offset (X toward the facing) [+ the
/// fighter's velocity x 0.5 through ClampVectorSize(.., -800, 800) (@12759 / @13840)].
fn socket_target(
    w: &World,
    att: FighterId,
    p: &HitPacket,
    account_vel: bool,
    what: &str,
    open: Option<&OpenPose>,
) -> (Real, Real) {
    let at = vacuum_socket(w, att, p.knockback_target_socket, what, open);
    let a = w.fighter(att);
    let (ox, oy) = p.knockback_socket_offset.map_or((0.0, 0.0), |o| (o.x.unwrap_or(0.0), o.y.unwrap_or(0.0)));
    let mut tx = at.x + ox * a.body.facing_dir;
    let mut ty = at.y + oy;
    if account_vel {
        let h = crate::math::ue::clamped_to_size([0.0, a.body.vx * 0.5, a.body.vy * 0.5], -800.0, 800.0);
        tx += h[1];
        ty += h[2];
    }
    (tx, ty)
}

/// `legacyVacuum`: pfgDoDamage's legacy vacuum (@13721-15048), ahead of DamageAndLaunch
/// and after the facing flip and KnockBackMirror: target = the active mesh's socket, + the offset, + the fighter's
/// velocity x 0.5 clamped to 800 when the hit accounts for it (the notify's CDO has it set).
/// KnockBackDirectionVector = (Y, FClamp(Z, 0, 1)) of the unit vector to the target, BaseKnockBack = FMax(the
/// distance x 1.5, 500) and KnockBackScaling = 1.
/// Not ported: an owner that is no fighter (@20717).
pub fn legacy_vacuum(
    w: &World,
    att: FighterId,
    vic: FighterId,
    p: &HitPacket,
    open: Option<&OpenPose>,
) -> LegacyVacuumOut {
    let acc = p.account_for_velocity_vacuum_notify.unwrap_or(true);
    let (tx, ty) = socket_target(w, att, p, acc, "legacy vacuum", open);
    let vb = &w.fighter(vic).body;
    let (ex, ey) = (tx - vb.x, ty - vb.y);
    // @14628 GetDirectionUnitVector (GetSafeNormal of the difference: zero when the two are at one place), @14859
    // VSize of the difference.
    let [ux, uy, _] = ue::direction_unit_vector([vb.x, vb.y, 0.0], [tx, ty, 0.0]);
    let dist = ue::size2d([ex, ey]);
    LegacyVacuumOut { dx: ux, dy: min(max(uy, 0.0), 1.0), bkb: max(dist * 1.5, 500.0) }
}

/// `directSocketDir`: pfgDoDamage's DirectKnockbackTowardsSocket step (@12130-13685): the
/// (Y, Z) of the unit vector from the victim to the socket + the offset [+ the velocity term], Z not clamped.
fn direct_socket_dir(
    w: &World,
    att: FighterId,
    vic: FighterId,
    p: &HitPacket,
    account_vel: bool,
    open: Option<&OpenPose>,
) -> V2 {
    let (tx, ty) = socket_target(w, att, p, account_vel, "direct socket", open);
    let vb = &w.fighter(vic).body;
    // @13547 GetDirectionUnitVector.
    let [x, y, _] = ue::direction_unit_vector([vb.x, vb.y, 0.0], [tx, ty, 0.0]);
    V2 { x, y }
}

/// `directHit`: a fighter's pfgDoDamage called by a notify with its own packet and no
/// hit box: no charged time, the packet's AttackID and AttackTag None, at the victim.
pub fn direct_hit(w: &mut World, att: FighterId, vic: FighterId, p: &HitPacket) {
    let at = V2 { x: w.fighter(vic).body.x, y: w.fighter(vic).body.y };
    let none = crate::name!("None");
    apply_hit(w, att, vic, p, 0.0, p.attack_id.unwrap_or(0), none, at, false, None);
}

/// `handlerHit`: a fighter's pfgDoDamage through another actor's hit box component (the
/// driver's handler with the car's HitCollision): the fighter is the dealer, with a hit box, no charged time, the
/// packet's AttackID, at `at`.
pub fn handler_hit(w: &mut World, att: FighterId, vic: FighterId, p: &HitPacket, name: Name, at: V2) {
    w.fighter_mut(att).frame.hit_this_frame = true;
    w.fighter_mut(vic).frame.hit_this_frame = true;
    apply_hit(w, att, vic, p, 0.0, p.attack_id.unwrap_or(0), name, at, true, None);
}

/// `refusedAllyHit`: a fighter's pfgDoDamage with its own packet and no hit box on a team
/// mate: DamageAndLaunch's refused path (refused_damage_and_launch), then NotifyAllyOfHit (the ally's HitByAlly and
/// the dealer's HitAlly). `wall_bounce`: pfgDoDamage's IsWallBounce; `dealer`: the DamageDealer actor when it is not
/// `att`.
pub fn refused_ally_hit(
    w: &mut World,
    att: FighterId,
    vic: FighterId,
    p: &HitPacket,
    wall_bounce: bool,
    dealer: ActorRef,
) {
    if invulnerable(w.fighter(vic)) {
        crate::fighter::common::warn_once(w, crate::name!("refusedAllyHit: an invulnerable ally is not ported"));
        return;
    }
    let p = buff::apply_dealt_damage_mods(w, att, ActorRef::Fighter(vic), p, false);
    let at = V2 { x: w.fighter(vic).body.x, y: w.fighter(vic).body.y };
    refused_damage_and_launch(w, att, vic, &p, at, false, None, wall_bounce, dealer);
    // NotifyAllyOfHit needs Victim != Instigator (@21344).
    if vic == att {
        return;
    }
    w.fighter_mut(att).decay.recent_hit_actor = Some(ActorRef::Fighter(vic)); // @433
    if buff::hit_by_ally_bound(w, vic) {
        let arg = BuffSignalArg { hit: Some(Box::new(p.as_ref().clone())), dealer: Some(att), ..Default::default() };
        buff::buff_signal(w, vic, BuffSignal::HitByAlly, arg);
    }
    // The HitAlly broadcast @886 of att's own handler: its Comp_Actor_VO, then the pawn's listeners.
    if dealer == ActorRef::Fighter(att) {
        crate::fighter::match_side::set_help_vo_tags(w, att, vic);
    }
    if let Some(h) = w.fighter(att).hooks.char.hit_ally {
        h(w, att, vic);
    }
}

/// `refusedDamageAndLaunch`: DamageAndLaunch @15630 of a fighter's handler on a victim
/// CanBeHitBy refuses (@718 -> @8047), with the packet after the dealt mods: CalculateArmor @7560, the
/// DamageManager's HitByAlly broadcast @8047, then with ForceKnockback @8093 the taken pass and the knockback path
/// @3222; without, nothing more (pop_flow @8140).
/// Not ported: VacuumInfo is not taken here (warned; no such packet reaches a team mate).
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn refused_damage_and_launch(
    w: &mut World,
    att: FighterId,
    vic: FighterId,
    p: &HitPacket,
    at: V2,
    hit_box: bool,
    open: Option<&OpenPose>,
    wall_bounce: bool,
    dealer: ActorRef,
) {
    let lv = if p.vacuum_knockback.unwrap_or(false) { Some(legacy_vacuum(w, att, vic, p, open)) } else { None };
    let mut q = p.clone();
    if let Some(l) = &lv {
        q.base_knock_back = Some(l.bkb);
        q.knock_back_scaling = Some(1.0);
    }
    let p = &q;
    let ca = &w.fighter(att).charge;
    let charge_dmg_mult = if wall_bounce { 1.0 } else { ca.charge_dmg_mult };
    let charge_mult = if wall_bounce { 1.0 } else { ca.charge_mult };
    let ar = crate::combat::defense::calculate_armor(
        w,
        vic,
        p.damage.unwrap_or(0.0) * charge_dmg_mult,
        p.base_knock_back.unwrap_or(0.0),
        p.hit_stun_mult.unwrap_or(1.0),
        charge_mult,
        p.armor_pierce.unwrap_or(false),
        Some(att),
        Some(at),
    );
    // @8047: the DamageManager's HitByAlly, LocalHitInformation as CalculateArmor left it.
    if buff::hit_by_ally_bound(w, vic) {
        let mut hit = p.clone();
        hit.damage = Some(ar.damage);
        hit.base_knock_back = Some(ar.kb);
        hit.hit_stun_mult = Some(ar.hs);
        let d = if dealer == ActorRef::Fighter(att) { Some(att) } else { None };
        let by = crate::fighter::common::actor_by(w, dealer, Some(att));
        let arg = BuffSignalArg { hit: Some(Box::new(hit)), dealer: d, by: Some(by), ..Default::default() };
        buff::buff_signal(w, vic, BuffSignal::DamageHitByAlly, arg);
    }
    if !p.force_knockback.unwrap_or(false) {
        return;
    }
    if p.vacuum_info.as_ref().is_some_and(|v| v.use_vacuum_knock_back.unwrap_or(false)) {
        crate::fighter::common::warn_once(w, crate::name!("refusedDamageAndLaunch: a VacuumInfo packet is not ported"));
    }
    // pfgDoDamage's direction, as apply_hit's: HitBoxDamage names it KnockBackDirectionVector, a Hitframe
    // KnockBackDirection.
    let d = p.knock_back_direction_vector.or(p.knock_back_direction);
    let lvd = lv.as_ref().map(|l| (l.dx, l.dy));
    let d0 = fighter_hit_dir(w, att, vic, p, d, at, hit_box, lvd, open);
    // No charged time here: HitPauseTime is the packet's.
    let x = taken_in(ar.damage, p.knock_back_scaling.unwrap_or(0.0), ar.hs, p.hit_pause_time.unwrap_or(0.0), d0, p);
    let t = self_taken_mods(w, vic, att, x, hit_box, p, dealer);
    let (bscale, bhs) = (t.scaling, t.hit_stun);
    let hp = fh::HitParams {
        damage: p.damage.unwrap_or(0.0),
        base_knock_back: ar.kb,
        knock_back_scaling: bscale,
        scaling_start_dmg: p.kb_scaling_starting_dmg.unwrap_or(0.0),
        hit_pause_time: p.hit_pause_time.unwrap_or(0.0),
        hit_stun_mult: bhs,
        charge_kb_mult: charge_mult,
        charge_hit_pause_mult: p.charge_hit_pause_mult.unwrap_or(0.0),
        total_charged_time: 0.0,
        weight: weight(w, w.fighter(vic)),
    };
    let (k, _) = knock_pair(&hp, w.fighter(vic).stats.damage, t.hit_pause);
    let V2 { x: dx, y: dy } = taken_dir(&t, d0);
    let light = ar.kb <= 0.0 && bhs <= 0.0;
    if light {
        fh::receive_light_hit(w, vic, 0.0, crate::real(k.hitpause_frames), k.hitpause_sec);
    } else {
        let (nx, ny) = fh::knockback_dir(dx, dy);
        let mut vx = nx * k.speed;
        let mut vy = ny * k.speed;
        if vy == 0.0 && vx != 0.0 {
            vy = 0.001;
        }
        (vx, vy) = keep_pending_launch(w, vic, vx, vy);
        (vx, vy) = buff::apply_knock_back_multipliers(w, att, vx, vy, false);
        (vx, vy) = buff::apply_knock_back_multipliers(w, vic, vx, vy, true);
        vx *= k.tier;
        vy *= k.tier;
        let kbi = if p.ignore_kbi.unwrap_or(false) { 0.0 } else { p.kbi_multiplier.unwrap_or(1.0) };
        let r = crate::fighter::tick::hit_rumble(enum_real(p.hit_reaction));
        crate::fighter::tick::controller_rumble(w, vic, r);
        let hs = fh::timer_count(k.hitstun);
        let o = fh::ReceiveHitO {
            damage: 0.0,
            vx,
            vy,
            hitpause_frames: crate::real(k.hitpause_frames),
            hitpause_sec: Some(k.hitpause_sec),
            hitstun_frames: crate::real(hs),
            hitstun_sec: Some(k.hitstun),
            kb_stun_frames: crate::real(k.kb_stun_frames),
            kb_stun_sec: Some(k.kb_stun_sec),
            kbi,
            dir_x: Some(dx),
            dir_y: Some(dy),
            att: Some(att),
            dealer: Some(ActorRef::Fighter(att)),
            raw_damage: Some(hp.damage),
            base_kb: Some(ar.kb),
            kb_dist: k.dist,
            hit_stun_mult: bhs,
            hit_pause_time: k.hit_pause_time,
        };
        fh::receive_hit(w, vic, &o);
        if wall_bounce {
            if let Some(l) = &mut w.fighter_mut(vic).hit.launch {
                l.wall = true; // KnockbackFromWallBounce @8872
            }
        } else {
            push_hit_vfx(w, vic, p, t.effect, at, vx, vy, k.dist);
            crate::fighter::control::replenish_evade(w, vic, C::EVADE_HIT_REFUND);
            let pct = w.fighter(att).charge.charge_pct;
            charge_hit_sound(w, vic, pct);
        }
        // HitReactionMulticast's view side (the hit flash, PlayHitSound's pain line), as apply_hit's event.
        let hp_f = crate::real(k.hitpause_frames);
        let (aid, v) = (w.fighter(att).id, w.fighter(vic));
        let e = Event::Hit {
            attacker: aid,
            victim: v.id,
            damage: 0.0,
            pain_damage: hp.damage,
            speed: ue::size2d([vx, vy]),
            hitpause: hp_f,
            lockout: hp_f + crate::real(hs),
            x: v.body.x,
            y: v.body.y,
            light: false,
        };
        w.events.push(e);
    }
    crate::fighter::tick::controller_rumble(w, att, p.controller_rumble);
    let hb = light_packet(if light { 0.0 } else { hp.damage }, ar.kb, bhs);
    buff::hit_by_enemy(w, vic, Some(att), 0.0, Some(&hb), Some(ActorRef::Fighter(att)), Some(V2 { x: dx, y: dy }));
}

/// `chargeHitSound`: Comp_Pawn_DamageManager HitReactionMulticast @2560-2981:
/// ChargePercent >= 1.0: the large sound behind DoOnce gate 2 (@4584); else > 0.5: the small one behind gate 1
/// (@2827), each reopened by Delay(1.0); posted at the victim's Mesh component.
fn charge_hit_sound(w: &mut World, vic: FighterId, pct: Real) {
    let tick = crate::real(w.flow.tick);
    let ev = if pct >= 1.0 {
        if tick < w.fighter(vic).charge.charge_hit_large_at {
            return;
        }
        let t = tick + crate::fighter::common::latent_frames(1.0);
        w.fighter_mut(vic).charge.charge_hit_large_at = t;
        crate::name!("sfx_shared_combat_charge_hit_large")
    } else if pct > 0.5 {
        if tick < w.fighter(vic).charge.charge_hit_small_at {
            return;
        }
        let t = tick + crate::fighter::common::latent_frames(1.0);
        w.fighter_mut(vic).charge.charge_hit_small_at = t;
        crate::name!("sfx_shared_combat_charge_hit_small")
    } else {
        return;
    };
    let v = w.fighter(vic);
    let (x, y) = (mesh_wx(v), mesh_wy(v) + crate::math::narrow(char_data(w, v).boxes.mesh_z));
    let id = v.id;
    w.events.push(Event::Sound { fighter: id, ev, x: Some(x), y: Some(y), hit: None, pooled: true, key: None });
}

/// `fighterHitDir`: pfgDoDamage's direction for a fighter's handler. `d`: the packet's
/// direction; `at`, `hit_box`: LocalHitLocation and the call's HitBox; `lv`: the legacy vacuum's direction.
/// @10359-11499: X x LocalLeftRightModifier (the facing); IgnoreFacing takes -1 for either facing. Then
/// DirectionalKnockback, KnockBackMirror, the socket direction and the legacy vacuum, each overwriting the one
/// before.
/// Deviation: a capsule hit shape's `at` is its first end, not the component's centre (no capsule hit shape of the
/// ported data has the DirectionalKnockback flag).
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
fn fighter_hit_dir(
    w: &World,
    att: FighterId,
    vic: FighterId,
    p: &HitPacket,
    d: Option<crate::combat::DataVec>,
    at: V2,
    hit_box: bool,
    lv: Option<(Real, Real)>,
    open: Option<&OpenPose>,
) -> V2 {
    let (ddx, ddy) = d.map_or((1.0, 0.0), |d| (d.x.unwrap_or(Real::NAN), d.y.unwrap_or(Real::NAN)));
    let a = w.fighter(att);
    let mut dx = ddx * if p.ignore_facing.unwrap_or(false) { -1.0 } else { a.body.facing_dir };
    let mut dy = ddy;
    if p.directional_knockback.unwrap_or(false) {
        // @8405-8996: the victim's root capsule location minus LocalHitLocation in the (Y, Z) plane; without a HitBox
        // the vector is zero.
        dx = if hit_box { w.fighter(vic).body.x - at.x } else { 0.0 };
        dy = if hit_box { fighter::actor_y(w, vic) - at.y } else { 0.0 };
    }
    if p.knock_back_mirror.unwrap_or(false) {
        // The horizontal direction flips when the victim is behind the attacker.
        let vx = w.fighter(vic).body.x;
        let behind = if a.body.facing_dir < 0.0 { vx >= a.body.x } else { vx <= a.body.x };
        if behind {
            dx = -dx;
        }
    }
    // @12462-12649: "Direct Knockback Toward Socket" with VacuumKnockback off and DoesSocketExist.
    let direct = p.direct_knockback_toward_socket.or(p.direct_knockback_towards_socket).unwrap_or(false);
    if !p.vacuum_knockback.unwrap_or(false) && direct && socket_exists(w, att, p.knockback_target_socket) {
        let acc = p.account_for_velocity_vacuum_notify.or(p.account_for_velocity_with_vacuum_knockback).unwrap_or(true);
        let ds = direct_socket_dir(w, att, vic, p, acc, open);
        dx = ds.x;
        dy = ds.y;
    }
    if let Some((lx, ly)) = lv {
        dx = lx;
        dy = ly;
    }
    V2 { x: dx, y: dy }
}

/// `selfTakenMods`: DamageAndLaunch's taken pass behind GetOwner() != DamageDealer: a hit
/// whose DamageDealer is the victim itself keeps its LocalHitInformation.
fn self_taken_mods(
    w: &mut World,
    vic: FighterId,
    att: FighterId,
    x: buff::TakenHit,
    hit_box: bool,
    p: &HitPacket,
    dealer: ActorRef,
) -> buff::TakenHit {
    if ActorRef::Fighter(vic) == dealer {
        return x;
    }
    buff::apply_taken_hit_mods(w, vic, x, Some(att), hit_box, Some(ActorRef::Fighter(att)), Some(p))
}

/// DamageAndLaunch's LocalHitInformation as its taken pass reads it: after CalculateArmor (Damage, HitStunMult), with
/// pfgDoDamage's direction (`dir`, the port's terms: x along the world's sim x; UE's KnockBackDirectionVector.X is
/// the world's Y = -sim x) and the hit's HitPauseTime (`pause`: the Hitframe's charged sum) and OnHitEffectOverride.
fn taken_in(damage: Real, scaling: Real, hs: Real, pause: Real, dir: V2, p: &HitPacket) -> buff::TakenHit {
    buff::TakenHit {
        damage,
        scaling,
        hit_stun: hs,
        hit_pause: pause,
        dir: (-dir.x, dir.y),
        effect: p.on_hit_effect_override,
    }
}

/// The two floats are one value (NaN and NaN alike, -0 and +0 alike).
fn same(a: Real, b: Real) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

/// The taken pass's KnockBackDirectionVector back in the port's terms (`taken_in`); the hit's own `dir` as it was when
/// the pass left the value alone (an Add of zero writes +0 for -0, which the port does not carry into the launch).
fn taken_dir(t: &buff::TakenHit, dir: V2) -> V2 {
    if same(t.dir.0, -dir.x) && same(t.dir.1, dir.y) {
        return dir;
    }
    V2 { x: -t.dir.0, y: t.dir.1 }
}

/// Calculate Knock Back's result for the victim (DamageAndLaunch @4559: the hit pause of the taken pass's
/// HitPauseTime, `pause`) and for the dealer (pfgDoDamage @16603-17442 reads its own LocalHitInformation, from before
/// DamageAndLaunch): one result when the pass left HitPauseTime as it was.
fn knock_pair(hp: &fh::HitParams, post: Real, pause: Real) -> (fh::KnockResult, fh::KnockResult) {
    let own = fh::compute_knock(hp, post);
    if same(pause, own.hit_pause_time) {
        return (own, own);
    }
    let v = fh::compute_knock(&fh::HitParams { hit_pause_time: pause, total_charged_time: 0.0, ..*hp }, post);
    (v, own)
}

/// `applyHit`: a fighter's pfgDoDamage: the entry rows (hit_buffs_call), then
/// apply_hit_behind_entry. `hit_box`: the call has a hit box component.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn apply_hit(
    w: &mut World,
    att: FighterId,
    vic: FighterId,
    p: &HitPacket,
    charged_time: Real,
    attack_id: i32,
    attack_name: Name,
    at: V2,
    hit_box: bool,
    open: Option<&OpenPose>,
) {
    // The port's own Dodgeball rule, behind the tests that pick the victim: a fighter's hit on an opponent is no hit.
    if crate::world::dodgeball::spares(w, att, vic) {
        return;
    }
    if !p.on_hit_buffs.is_empty() {
        hit_buffs_call(w, att, &p.on_hit_buffs);
    }
    apply_hit_behind_entry(w, att, vic, p, charged_time, attack_id, attack_name, at, hit_box, open);
}

/// `applyHitBehindEntry`: apply_hit behind pfgDoDamage's entry rows, for a caller that
/// ran them at the call's entry itself (the hit box overlap: the melee test).
/// Unverified: a buff an entry row has just given the dealer takes part in this same hit here.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn apply_hit_behind_entry(
    w: &mut World,
    att: FighterId,
    vic: FighterId,
    p: &HitPacket,
    charged_time: Real,
    attack_id: i32,
    attack_name: Name,
    at: V2,
    hit_box: bool,
    open: Option<&OpenPose>,
) {
    if crate::world::dodgeball::spares(w, att, vic) {
        return; // the port's own Dodgeball rule (as apply_hit)
    }
    // pfgDoDamage @5617-5904: the dealer's State Manager rewrites the hit (ApplyBuffDamageMods, DamageTaken false).
    let mut p = buff::apply_dealt_damage_mods(w, att, ActorRef::Fighter(vic), p, hit_box);
    // pfgDoDamage ahead of DamageAndLaunch: the legacy vacuum rewrites the packet's knockback.
    let lv = if p.vacuum_knockback.unwrap_or(false) { Some(legacy_vacuum(w, att, vic, &p, open)) } else { None };
    if let Some(l) = &lv {
        let q = p.to_mut();
        q.base_knock_back = Some(l.bkb);
        q.knock_back_scaling = Some(1.0);
    }
    let p = p;
    let (charge_dmg_mult, charge_mult, charge_buff_mult) = {
        let c = &w.fighter(att).charge;
        (c.charge_dmg_mult, c.charge_mult, c.charge_buff_mult)
    };
    // DamageAndLaunch: armor sees the charged damage; Calculate Knock Back uses the unmodified hit damage.
    let ar = crate::combat::defense::calculate_armor(
        w,
        vic,
        p.damage.unwrap_or(0.0) * charge_dmg_mult,
        p.base_knock_back.unwrap_or(0.0),
        p.hit_stun_mult.unwrap_or(1.0),
        charge_mult,
        p.armor_pierce.unwrap_or(false),
        Some(att),
        Some(at),
    );
    // CalculateArmor @7560, RegisterHitFromFighter @732 (the forced additive knockback test reads the armor's
    // HitStunMult), ApplyBuffDamageMods @1621-1960, GetHitStunDecay @2059.
    let dt = enum_int(p.attack_decay_type);
    let dh = DecayHit {
        attack_id,
        decay_type: dt,
        decay_name: if [1, 2, 3, 6].contains(&dt) {
            p.attack_decay_name.unwrap_or(crate::name!("None"))
        } else {
            attack_name
        },
        combo_name: attack_name,
        base_knock_back: p.base_knock_back.unwrap_or(0.0),
        launch_knock_back: Some(ar.kb * charge_mult),
        additive_kb_mode: p.additive_kb_mode,
        knock_back_scaling: Some(p.knock_back_scaling.unwrap_or(0.0)),
        hit_stun_mult: Some(ar.hs),
    };
    fh::register_hit_from(w, vic, att, &dh, true, att);
    // pfgDoDamage's direction (@10359-12649), in the hit ahead of DamageAndLaunch.
    let lvd = lv.as_ref().map(|l| (l.dx, l.dy));
    let d0 = fighter_hit_dir(w, att, vic, &p, p.knock_back_direction, at, hit_box, lvd, open);
    // The victim's taken mods (skipped when the victim is the DamageDealer itself).
    let pause = p.hit_pause_time.unwrap_or(0.0) + charged_time * p.charge_hit_pause_mult.unwrap_or(0.0);
    let x = taken_in(ar.damage, p.knock_back_scaling.unwrap_or(0.0), ar.hs, pause, d0, &p);
    let t = self_taken_mods(w, vic, att, x, hit_box, &p, ActorRef::Fighter(att));
    let (bdmg, bscale, bhs) = (t.damage, t.scaling, t.hit_stun);
    let hp = fh::HitParams {
        damage: p.damage.unwrap_or(0.0),
        base_knock_back: ar.kb,
        knock_back_scaling: bscale,
        // HitInfo.KBScalingStartingDmg (Calculate Knock Back @794-1116), the Hitframe's own property.
        scaling_start_dmg: p.kb_scaling_starting_dmg.unwrap_or(0.0),
        hit_pause_time: p.hit_pause_time.unwrap_or(0.0),
        hit_stun_mult: bhs,
        charge_kb_mult: charge_mult,
        charge_hit_pause_mult: p.charge_hit_pause_mult.unwrap_or(0.0),
        total_charged_time: charged_time,
        weight: weight(w, w.fighter(vic)),
    };
    let pre = combo_decay_pre(w, Some(att), vic, &dh, bdmg, true);
    // Comp_Armor CalculateTempHealthDamage: the victim's temp health packs take their share first.
    let taken = crate::combat::defense::absorb_temp_health(w, vic, pre.dmg);
    let post = taken + w.fighter(vic).stats.damage; // CurrentDamage after ApplyDamage, a float32
    let (mut k, kd) = knock_pair(&hp, post, t.hit_pause);
    let V2 { x: mut dx, y: mut dy } = taken_dir(&t, d0);
    // Calculate Knock Back with VacuumInfo.UseVacuumKnockBack: the vacuum velocity replaces the hit's direction, and
    // after the multipliers and the tier the distance (hitstun) and the launch are lerped toward the vacuum.
    let vac = match &p.vacuum_info {
        Some(v) if v.use_vacuum_knock_back.unwrap_or(false) => Some(vacuum_knockback(w, att, vic, v, open)),
        _ => None,
    };
    let alpha = p.vacuum_info.as_ref().and_then(|v| v.vacuum_strength_alpha).unwrap_or(0.0);
    if let Some(v) = &vac {
        dx = v.vx;
        dy = v.vy;
        k.dist += (v.dist - k.dist) * alpha;
        k.hitstun = fh::knockback_to_hitstun(k.dist, hp.hit_stun_mult);
        let t = fh::kb_stun_times(k.dist, hp.hit_stun_mult);
        set_kb_stun(&mut k, t);
    }
    let fr = combo_decay_frames(&pre, k.hitstun);
    let (nx, ny) = fh::knockback_dir(dx, dy);
    let mut vx = nx * k.speed;
    let mut vy = ny * k.speed;
    if vy == 0.0 && vx != 0.0 {
        vy = 0.001; // Calculate Knock Back: a flat launch gets Z 0.001
    }
    (vx, vy) = keep_pending_launch(w, vic, vx, vy);
    // Path A (the dealer has a DamageManager): attacker's dealt buff mods, victim's taken mods, then the tier.
    (vx, vy) = buff::apply_knock_back_multipliers(w, att, vx, vy, false);
    (vx, vy) = buff::apply_knock_back_multipliers(w, vic, vx, vy, true);
    vx *= k.tier;
    vy *= k.tier;
    if let Some(v) = &vac {
        vx += (v.vx - vx) * alpha;
        vy += (v.vy - vy) * alpha;
    }
    // Light path when both are zeroed (armor): damage and the scaled hitpause, no launch or hitstun.
    let light = ar.kb <= 0.0 && bhs <= 0.0;
    fh::record_damage(w, vic, taken, Some(att));
    crate::world::flow::update_ffa_team_damage(w, vic, taken);
    crate::world::flow::broadcast_take_any_damage(w, ActorRef::Fighter(vic), pre.dmg, Some(ActorRef::Fighter(att)));
    if light {
        fh::receive_light_hit(w, vic, taken, crate::real(k.hitpause_frames), k.hitpause_sec);
    } else {
        // The victim keeps its facing. KBI: RageUppercut's Hitframes set IgnoreKBI.
        let kbi = if p.ignore_kbi.unwrap_or(false) { 0.0 } else { p.kbi_multiplier.unwrap_or(1.0) };
        // Calculate Knock Back @4341-4461: the hit fighter's BPI_ControllerRumble(GetHitRumble(HitReaction)).
        let r = crate::fighter::tick::hit_rumble(enum_real(p.hit_reaction));
        crate::fighter::tick::controller_rumble(w, vic, r);
        let o = fh::ReceiveHitO {
            damage: taken,
            vx,
            vy,
            hitpause_frames: crate::real(k.hitpause_frames),
            hitpause_sec: Some(k.hitpause_sec),
            hitstun_frames: crate::real(fr.hitstun_frames),
            hitstun_sec: Some(fr.hitstun_sec),
            kb_stun_frames: crate::real(k.kb_stun_frames),
            kb_stun_sec: Some(k.kb_stun_sec),
            kbi,
            dir_x: Some(dx),
            dir_y: Some(dy),
            att: Some(att),
            dealer: Some(ActorRef::Fighter(att)),
            raw_damage: Some(hp.damage),
            base_kb: Some(ar.kb),
            kb_dist: k.dist,
            hit_stun_mult: bhs,
            hit_pause_time: k.hit_pause_time,
        };
        fh::receive_hit(w, vic, &o);
    }
    if !light {
        push_hit_vfx(w, vic, &p, t.effect, at, vx, vy, k.dist);
    }
    // DamageAndLaunch @3589-3668: the DamageDealer gets BPI_ControllerRumble(the packet's ControllerRumble).
    crate::fighter::tick::controller_rumble(w, att, p.controller_rumble);
    // DamageAndLaunch @3723, inside pfgDoDamage: ahead of the dealer's own hit pause.
    let hb = light_packet(if light { fr.dmg } else { hp.damage }, ar.kb, bhs);
    buff::hit_by_enemy(
        w,
        vic,
        Some(att),
        hb_dmg(&hb),
        Some(&hb),
        Some(ActorRef::Fighter(att)),
        Some(V2 { x: dx, y: dy }),
    );
    let queue = p.queue_hit_pause_until_pose.unwrap_or(false);
    fh::on_dealt_hit(w, att, crate::real(kd.hitpause_frames), kd.hit_pause_time, queue, kd.hitpause_sec);
    crate::combat::defense::dealt_rage(w, att, vic, hp.damage);
    // HitBoxHandler OnHitBuffs (after the hitpause): round(StackCount x GetChargeBuffStackMultiplier).
    if !p.on_hit_buffs.is_empty() {
        hit_buffs_hit(w, att, vic, &p.on_hit_buffs, charge_buff_mult, false);
    }
    // The dealer's handler's HitEnemyPlayer(HitActor, owner) @22770: the attacker's Comp_Actor_VO, then the
    // attacker's buffs that answer it.
    crate::fighter::match_side::set_attack_vo_tags(w, att, vic);
    // Not ported: BlockHitEnemyDispatch of the victim's DamageManager (pfgDoDamage @21741): no class writes it.
    buff::buff_signal(w, att, BuffSignal::HitEnemy, BuffSignalArg { victim: Some(vic), ..Default::default() });
    // Deviation: the build calls the listeners of one broadcast in bind order (ProcessMulticastDelegate
    // 14079aa20); the port runs the attacker's buffs first, then its character's own listeners.
    if let Some(h) = w.fighter(att).hooks.char.hit_enemy {
        h(w, att, vic);
    }
    // HitEnemyMulticastDispatch (attacker) and HitDispatchMulticast (victim, knockback path only) refund dodge meter.
    crate::fighter::control::replenish_evade(w, att, C::EVADE_HIT_REFUND);
    if !light {
        crate::fighter::control::replenish_evade(w, vic, C::EVADE_HIT_REFUND);
    }
    // HitEnemyMulticastDispatch @9653: HitEnemyMulticast(owner), an AnimNotify_LaunchSelf with LaunchOnHit.
    crate::fighter::moves::launch_on_hit(w, att);
    // HitEnemyMulticastDispatch @9619-9681: HitEnemyPlayerLight, light hit or not.
    crate::fighter::anim::cond_signal(w, att, CondSignal::HitEnemyLight);
    let pct = w.fighter(att).charge.charge_pct;
    if !light {
        charge_hit_sound(w, vic, pct);
    }
    if let Some(ev) = p.on_hit_sound
        && !in_hitstun(w.fighter(att))
    {
        let zf = if light { None } else { Some(fighter::actor_y(w, vic) + fighter::capsule_half(w, vic) * 0.5) };
        let v = w.fighter(vic);
        let hit = SoundHit { victim: v.id, z_floor: zf, charge: Some(pct), surface: None, instigator: None };
        let aid = w.fighter(att).id;
        w.events.push(Event::Sound {
            fighter: aid,
            ev,
            x: Some(at.x),
            y: Some(at.y),
            hit: Some(hit),
            pooled: false,
            key: None,
        });
    }
    let aid = w.fighter(att).id;
    push_hit_event(w, aid, vic, &fr, &p, &k, light, vx, vy);
    crate::world::catch_up::hit_player(w, att); // pfgDoDamage @22038: BFHitPlayer on the handler's owner
    fighter_hit_anything(w, att, ActorRef::Fighter(vic), &p, hit_box); // @22208
}

/// `fighterHitAnything`: the dealer fighter's handler broadcasts HitAnything(HitBoxInfo,
/// HitActor) (pfgDoDamage @22088-22208) once its hit went through: MostRecentHitActor = HitActor (@21532) first, the
/// pawn's binds, the buffs', then a running tutorial actor's.
pub fn fighter_hit_anything(w: &mut World, att: FighterId, victim: ActorRef, hit: &HitPacket, hit_box: bool) {
    w.fighter_mut(att).decay.recent_hit_actor = Some(victim);
    if let Some(h) = w.fighter(att).hooks.char.hit_anything {
        h(w, att, victim, hit, hit_box);
    }
    if !w.fighter(att).buffs.buffs.is_empty() {
        let arg =
            BuffSignalArg { hit_actor: Some(victim), damage: Some(hit.damage.unwrap_or(0.0)), ..Default::default() };
        buff::buff_signal(w, att, BuffSignal::HitAnything, arg);
    }
    // A running tutorial actor's OnHitAnything (bound after the pawn's BeginPlay, so last).
    if crate::modes::tutorial::running(w) {
        crate::modes::tutorial::run::hit_anything(w, att);
    }
}

// ---- the call surface of pfgDoDamage

/// `doDamage`: pfgDoDamage of a spawned dealer on a HitActor that is no fighter,
/// routed by the two kinds: a spawned actor's hit on an item or an owned projectile (spawned_hit), a dealer's on a
/// projectile, an item, a mortar aim or a World.actors actor. Another pair warns and does nothing.
pub fn do_damage(w: &mut World, dealer: ActorRef, victim: ActorRef, hit: &HitPacket, ctx: &DamageCall) -> bool {
    let spawned = matches!(dealer, ActorRef::Projectile(_) | ActorRef::Item(_));
    let owned_projectile = match victim {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some_and(|p| p.owner.is_some()),
        _ => false,
    };
    if spawned && (matches!(victim, ActorRef::Item(_)) || owned_projectile) {
        return crate::combat::spawned_hits::spawned_hit(w, dealer, victim, ctx.clank, hit, ctx.rows);
    }
    if !matches!(dealer, ActorRef::Fighter(_)) {
        use crate::combat::spawned_hits as sh;
        match victim {
            ActorRef::Projectile(id) if !spawned => return sh::damage_spawned_actor_call(w, dealer, id, hit, ctx),
            ActorRef::Item(id) => return sh::damage_item(w, dealer, id, hit, ctx),
            ActorRef::Mortar(id) => return sh::damage_mortar(w, dealer, id, hit, ctx),
            ActorRef::Actor(id) => return sh::damage_world_actor(w, dealer, id, hit, None, ctx),
            _ => {}
        }
    }
    crate::fighter::common::warn_once(w, crate::name!("doDamage: a dealer on this HitActor is not routed"));
    false
}

// ---- a hit-box dealer over the actors that are no fighter

/// `HitShape`: the dealer's hit shape in the play plane.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HitShape {
    Sphere { x: Real, y: Real, r: Real },
    Box { x: Real, y: Real, hx: Real, hy: Real, depth: Real },
}

/// A dealer's actor is gone or dead (`a.dead`), for the kinds a hit box can overlap.
fn actor_gone(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_none_or(|p| p.dead),
        ActorRef::Item(id) => w.actors.items.get(id).is_none_or(|i| i.dead),
        ActorRef::Actor(id) => w.actors.actors.get(id).is_none_or(|x| x.dead),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).is_none_or(|m| m.dead),
        ActorRef::Marker(id) => w.actors.markers.get(id).is_none_or(|m| m.dead),
        ActorRef::Ship(id) => w.actors.ships.get(id).is_none_or(|s| s.dead),
        ActorRef::AimPunch(id) => w.actors.aim_punches.get(id).is_none_or(|s| s.dead),
        ActorRef::Accelerator(id) => w.actors.accelerators.get(id).is_none_or(|s| s.dead),
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => false,
    }
}

/// `hitBoxActorHits`: Comp_Pawn_HitBoxHandler OnComponentBeginOverlap_Event @7466 ->
/// pfgDoDamage @7502: a spawned actor's or a buff's hit box deals to whatever it begins to overlap. The bodies are the
/// trace gather's of the object types the hit box's own template does not ignore (`types`), kept where the body's
/// template does not ignore the hit box's type; the fighters are left out (their hurt shapes are the dealer's own
/// fighter test).
/// `over`: the actors the box overlaps now (an overlap that goes on deals once), cleared with the box off; `once`: the
/// handler's TrackActorsHitByTag list for HitEachActorOnlyOnce (@2883, @5290), cleared by the dealer's HitBoxesOff.
/// The caller takes both sets out of its state for the call and puts them back.
// Deviation: tested at the dealer's tick in the play plane, after its fighter test; the actors in the gather's order.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn hit_box_actor_hits(
    w: &mut World,
    dealer: ActorRef,
    hit: &HitPacket,
    types: &[Real],
    shape: Option<HitShape>,
    over: &mut indexmap::IndexSet<ActorRef>,
    once: &mut indexmap::IndexSet<ActorRef>,
) {
    /// `OBJ.HitBox`.
    const OBJ_HIT_BOX: Real = 7.0;
    let Some(shape) = shape.filter(|_| !types.is_empty() && !actor_gone(w, dealer)) else {
        over.clear();
        return;
    };
    let ignore = [Some(dealer)];
    let (found, x, y) = match shape {
        HitShape::Sphere { x, y, r } => {
            (crate::fighter::traces::sphere_overlap_actors(w, x, y, r, types, &ignore, OBJ_HIT_BOX), x, y)
        }
        HitShape::Box { x, y, hx, hy, depth } => {
            (crate::fighter::traces::box_overlap_actors(w, x, y, hx, hy, depth, types, &ignore, OBJ_HIT_BOX), x, y)
        }
    };
    over.retain(|o| found.contains(o));
    let each_once = hit.hit_each_actor_only_once.unwrap_or(false);
    for v in found {
        if matches!(v, ActorRef::Fighter(_)) || over.contains(&v) || actor_gone(w, v) {
            continue;
        }
        over.insert(v);
        if each_once && once.contains(&v) {
            continue;
        }
        // DoLOSCheck @3298 (ProjectileExplode from the box to the actor), ahead of the track @5290
        if hit.do_los_check.unwrap_or(false)
            && let Some((vx, vy, _)) = crate::actors::world_actor::actor_place(w, v)
            && crate::combat::phases::los_blocked(w, x, y, vx, vy)
        {
            continue;
        }
        if each_once {
            once.insert(v);
        }
        do_damage(w, dealer, v, hit, &DamageCall { hit_box: true, comp: None, clank: false, rows: None });
        if actor_gone(w, dealer) {
            return;
        }
    }
}

// ---- the call surface's argument and return types

/// What `getChargeValue` gives.
#[derive(Clone, Copy, Debug)]
pub struct GetChargeValueOut {
    pub pct: Real,
    pub has: bool,
}

/// What `projectileHitDir` gives.
#[derive(Clone, Copy, Debug)]
pub struct ProjectileHitDirOut {
    pub x: Real,
    pub y: Real,
    pub bkb: Real,
}

/// What `comboDecayPre` gives.
#[derive(Clone, Copy, Debug)]
pub struct ComboDecayPreOut {
    pub dmg: Real,
    pub modifier: Real,
}

/// What `vacuumKnockback` gives.
#[derive(Clone, Copy, Debug)]
pub struct VacuumKnockbackOut {
    pub dist: Real,
    pub vx: Real,
    pub vy: Real,
}

/// What `legacyVacuum` gives.
#[derive(Clone, Copy, Debug)]
pub struct LegacyVacuumOut {
    pub dx: Real,
    pub dy: Real,
    pub bkb: Real,
}
