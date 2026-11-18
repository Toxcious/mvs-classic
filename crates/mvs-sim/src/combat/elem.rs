//! The state a spawned actor carries for combat: its charge values and its
//! Comp_State_Manager's elemental buffs.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::BuffClass;
use crate::fighter::FighterId;
use crate::math;
use crate::names::Name;
use crate::registry::entries::ActorBuffHooks;
use crate::registry::signals::ActorBuffArg;
use crate::registry::signals::ActorBuffSignal;
use crate::world::World;
use mvs_data::value::{Props, Value};

/// `ChargeVals`.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct ChargeVals {
    /// DamageMultiplier (GetChargeValue 0).
    pub dmg: Real,
    /// KnockBackMultiplier (1).
    pub kb: Real,
    /// BuffStackMultiplier (2).
    pub buff: Real,
    /// PercentCharged (4).
    pub pct: Real,
}

/// `ElemBuff`: one buff on an actor's Comp_State_Manager.
#[derive(State, Debug)]
pub struct ElemBuff {
    /// The entry's serial in its list (not the entry itself): what a caller keeps across a call that may
    /// change the list.
    pub serial: u32,
    /// `cls`.
    pub class: BuffClass,
    /// CurrentStackAmount.
    pub stacks: i32,
    /// The looping 0.5 s timer's ticks left (an owner with a box collision only).
    pub timer: Option<i32>,
    /// Spawned by the owner's Comp_Pawn_PassiveBuffManager (HasPassiveBuff).
    pub passive: bool,
    /// `mod`: Buff_SlowProjectile's ActiveSpeedModifier; `None` while it is the default 0.
    pub speed_mod: Option<Real>,
    /// `life`: BuffParentActor's LifeTimer as the ticks left (`None`: no timer).
    pub life: Option<i32>,
    /// `stack`: the stack life timer's ticks left.
    pub stack: Option<i32>,
    /// `gate`: MinStackPreventedGate's timer ticks left (`None`: the gate open).
    pub gate: Option<i32>,
    /// `addAt`: a NewStackApplyBuffer class's TimeSinceLastAdd as the world tick of the last AddStack it let through.
    pub add_at: Option<i32>,
    /// `ext`: the class graph's own variables, a variant per class.
    pub ext: ElemExt,
}

/// `ElemBuff.ext`: the class graph's own variables, a variant per buff class that has some. A class ported later
/// adds its variant.
#[allow(
    clippy::large_enum_variant,
    reason = "the Sing Sphere's fixed key set (phases::SPHERE_KEYS) is the one big variant; the enum is Copy state"
)]
#[derive(State, Copy, PartialEq, Debug, Default)]
pub enum ElemExt {
    #[default]
    None,
    /// A perk buff class's own variables (`combat::perks`).
    Perk(crate::combat::perks::PerkActorExt),
    /// Buff_Slow30PercentProjectile: the binds made (`bound`) and NewProjectileScale where its begin set it.
    Slow30 { bound: bool, scale: Option<Real> },
    /// Buff_Polymorph_ProjectileEnlarge_WithHieghtChange: the Offset its begin raised the owner by.
    HeightChange { off: Real },
    /// Buff_ProjectileApplyBuff's children: the HitEnemyPlayer bind made, and Active from the last dealt pass.
    ApplyBuff { bound: bool, active: bool },
    /// Buff_Invisibility (and its Fog child) on a follower (`actors::smoke_bomb`): CurrentlyInvisible (`hidden`), the
    /// Comp_Follower delegates bound (`bound`) and the ticks left of HidePlayer's Delay(0.2) (`delay`, 0: none).
    Follower { hidden: bool, bound: bool, delay: Real },
    /// DestroyAfterApplied written on the instance (Buff_IgnitedBasketballPass @714).
    DestroyAfterApplied(bool),
    /// C021_Buff_ElectricChainVictim on a spawned actor (`chars::black_adam::chain`): the world tick of the next
    /// damage tick (`at`) and the Instigator (`by`, kept by its id).
    ChainVictim { at: Real, by: i32 },
    /// Buff_C021_SlowReflectProjectile (`chars::black_adam::field`): the hurt sphere's radius (`r`) and the reflect
    /// made (`refl`).
    SlowReflect { r: Real, refl: bool },
    /// Buff_GarnetStopProjectile (`chars::garnet::stop_projectile`).
    GarnetStopProjectile(crate::chars::garnet::stop_projectile::StopExt),
    /// Buff_C015HitProjectile (`chars::taz::projectile_eat`).
    C015HitProjectile(crate::chars::taz::projectile_eat::HitProjExt),
    /// Buff_ApplyTasty on a spawned actor (`chars::taz::tasty`).
    ApplyTasty(crate::chars::taz::tasty::ApplyTastyActor),
    /// Buff_Tasty on a spawned actor (`chars::taz::tasty`).
    Tasty(crate::chars::taz::tasty::TastyActor),
    /// Buff_TastyOnHit on a spawned actor (`chars::taz::tasty`).
    TastyOnHit(crate::chars::taz::tasty::TastyOnHitActor),
    /// Buff_ProjectileThroughTunnel (`actors::tunnel_carry`, Bugs Bunny's tunnel).
    ThroughTunnel(crate::actors::tunnel_carry::ThroughTunnelExt),
    /// C020_Buff_Silenced on a spawned actor (`chars::rick`).
    C020Silenced(crate::chars::rick::SilencedActorExt),
    /// Buff_C018BubbledProjectile (`chars::marvin::bubble`).
    C018Bubbled(crate::chars::marvin::bubble::BubbledExt),
    /// Buff_C018HomingProjectile and Buff_C018PerkHomingProjectile (`chars::marvin::homing`).
    C018HomingProjectile(crate::chars::marvin::homing::HomingProjExt),
    /// Buff_C018ReverseProjectile (`chars::marvin::reverse`).
    C018ReverseProjectile(crate::chars::marvin::reverse::ReverseExt),
    /// Buff_C018_PerkProjectileSpeed (`chars::marvin::perks`).
    C018PerkProjectileSpeed(crate::chars::marvin::perks::SpeedExt),
    /// Buff_SnowballHead on a spawned actor (`items::snowball_head`): the head's transform, kept for the view.
    SnowballHead { x: Real, y: Real, scale: Real },
    /// Buff_GrowWithDamageProjectile (`combat::grow_with_damage`): the scale and damage its growth call wrote.
    GrowProjectile { size: Real, damage: Real },
}

impl ElemExt {
    /// CanOverride where the class graph wrote it on the instance (Buff_GarnetStopProjectile @2301).
    pub fn can_override(self) -> Option<bool> {
        match self {
            ElemExt::GarnetStopProjectile(e) => e.can_override,
            _ => None,
        }
    }

    /// The tunnel carry's keys on a kind's extension (`actors::tunnel_carry`): the classes that carry an actor.
    pub fn carry(&self) -> Option<&crate::actors::tunnel_carry::CarryExt> {
        match self {
            ElemExt::GarnetStopProjectile(e) => Some(&e.carry),
            ElemExt::C015HitProjectile(e) => Some(&e.carry),
            ElemExt::ThroughTunnel(e) => Some(&e.carry),
            ElemExt::C018Bubbled(e) => Some(&e.carry),
            _ => None,
        }
    }

    /// `carry`, to write.
    pub fn carry_mut(&mut self) -> Option<&mut crate::actors::tunnel_carry::CarryExt> {
        match self {
            ElemExt::GarnetStopProjectile(e) => Some(&mut e.carry),
            ElemExt::C015HitProjectile(e) => Some(&mut e.carry),
            ElemExt::ThroughTunnel(e) => Some(&mut e.carry),
            ElemExt::C018Bubbled(e) => Some(&mut e.carry),
            _ => None,
        }
    }

    /// DestroyAfterApplied where a graph wrote it on the instance (Buff_IgnitedBasketballPass @714).
    pub fn destroy_after_applied(self) -> Option<bool> {
        match self {
            ElemExt::DestroyAfterApplied(b) => Some(b),
            _ => None,
        }
    }
}

/// The class graph's variables of the actor's buff with this serial, replaced whole by `ext`
/// (a caller passes every variable its class keeps).
pub fn set_elem_ext(w: &mut World, a: ActorRef, serial: u32, ext: ElemExt) {
    if let Some(b) = at_mut(w, a, serial) {
        b.ext = ext;
    }
}

/// `ElemState`: the actor's Comp_State_Manager buff list, with what its class answers to BPI_Elemental_System and
/// BPI_CanBePooled (`ElemOpt`; SpawnedActorParent's defaults: no box, no overlap, not pooled).
#[derive(State, Debug, Default)]
pub struct ElemState {
    /// BPI_GetBoxCollision is valid: the 0.5 s timer runs.
    pub bx: bool,
    /// BPI_ShouldActorOverlap.
    pub overlap: bool,
    /// A SpawnedActorParent with CanBePooled: DestroyAfterApplied leaves the buff (Fire @15).
    pub pooled: bool,
    /// BPI_GetSphereCollision is valid (GarnetElectricZone's Sphere): Buff_Elemental_Electric's overlap timer runs.
    pub sphere: bool,
    pub buffs: Vec<ElemBuff>,
    /// The next entry's serial.
    pub next_serial: u32,
    /// The buffs whose EndPlay runs now, out of `buffs` already (the hook's index is `ENDING` + the place here);
    /// empty between calls.
    pub ending: Vec<ElemBuff>,
    /// Counts the list's removals: a walk that began before one runs over the list as it was, so it
    /// sees no entry spawned after it.
    pub removals: u32,
}

// ---- the call surface of the actor charge reads
//
// SpawnedActorParent (ReceiveBeginPlay @18445 -> @8847 sequence -> @11459-12608), Comp_Spawner (ActivateActors
// @3433-3759, SetChargeVariables @0-1458), Comp_Pawn_ChargeAttack (GetChargeValue @0-646), Comp_Pawn_HitBoxHandler
// (the getters its hit reads). SpawnedActorParent has a Comp_Pawn_ChargeAttack of its own (the CDO's
// DamageMultiplier, KnockBackMultiplier and BuffStackMultiplier 1, PercentCharged 0) and
// ReceiveChargeValuesFromInstigator true by default. The actor's hit box handler reads that component:
// GetChargeDamageMultiplier and GetChargeKnockBackMultiplier (GetChargeValue 0 and 1) into DamageAndLaunch @15630,
// GetChargePercent (4) into it too, GetChargeBuffStackMultiplier for the OnHitBuffs stack counts @6482 / @18768.
// GetChargeValue on a component whose owner is no fighter returns the stored value at once (@85 -> @236). An actor
// holds `charge` only while a copy gave it values other than the defaults (1, 1, 1, 0). The copies of
// ChargeStartTime and ChargeTotalTime (@12395, @12537; SetChargeVariables @1116, @1289) are not kept: nothing reads
// them on an actor.

/// GetChargeValue's test @99-222: the charge's AttackMontage is the active instance: the grouped instance filed
/// with the charge, else the main montage.
fn charge_montage_active(w: &World, f: FighterId) -> bool {
    use crate::fighter::anim::MontageSlot;
    let Some(a) = crate::fighter::anim::active_instance(w, f) else { return false };
    let filed = w.fighter(f).montages.grouped.as_ref().and_then(|(l, _)| l.iter().position(|e| e.charge));
    a == match filed {
        Some(i) => MontageSlot::Grouped(u32::try_from(i).unwrap_or(u32::MAX)),
        None => MontageSlot::Main,
    }
}

/// `instigatorCharge`: the instigator's GetChargeValue(1), (0), (2) and (4); `None` for
/// the defaults. GetChargeValue @99-528: the stored values only while the charge's AttackMontage is the active
/// instance, else 1, 1, 1, 0.
pub fn instigator_charge(w: &World, f: FighterId) -> Option<ChargeVals> {
    let c = &w.fighter(f).charge;
    if c.charge_dmg_mult == 1.0 && c.charge_mult == 1.0 && c.charge_buff_mult == 1.0 && c.charge_pct == 0.0 {
        return None;
    }
    if !charge_montage_active(w, f) {
        return None;
    }
    Some(ChargeVals { dmg: c.charge_dmg_mult, kb: c.charge_mult, buff: c.charge_buff_mult, pct: c.charge_pct })
}

/// `itemSetChargeVariables`: Comp_ItemManager SetChargeVariables @0-1262 (ActivateItem
/// @724, ahead of ActivateObject): the knockback, damage and buff stack multipliers from the owner's GetChargeValue;
/// PercentCharged is not copied, the item keeps its own.
pub fn item_set_charge_variables(w: &mut World, a: ActorRef, c: Option<ChargeVals>) {
    let Some(slot) = crate::actors::world_actor::actor_charge_mut(w, a) else { return };
    let pct = slot.map_or(0.0, |h| h.pct);
    let (dmg, kb, buff) = c.map_or((1.0, 1.0, 1.0), |c| (c.dmg, c.kb, c.buff));
    *slot = if dmg == 1.0 && kb == 1.0 && buff == 1.0 && pct == 0.0 {
        None
    } else {
        Some(ChargeVals { dmg, kb, buff, pct })
    };
}

/// `chargeBuff`: GetChargeBuffStackMultiplier.
pub fn charge_buff(w: &World, a: ActorRef) -> Real {
    held(w, a).map_or(1.0, |c| c.buff)
}

/// `chargeDmg`: GetChargeDamageMultiplier.
pub fn charge_dmg(w: &World, a: ActorRef) -> Real {
    held(w, a).map_or(1.0, |c| c.dmg)
}

/// `chargeKb`: GetChargeKnockBackMultiplier.
pub fn charge_kb(w: &World, a: ActorRef) -> Real {
    held(w, a).map_or(1.0, |c| c.kb)
}

/// `chargePct`: GetChargePercent.
pub fn charge_pct(w: &World, a: ActorRef) -> Real {
    held(w, a).map_or(0.0, |c| c.pct)
}

/// The actor's Comp_Pawn_ChargeAttack values, none where it holds none (`pr.charge`).
pub fn charge_vals(w: &World, a: ActorRef) -> Option<ChargeVals> {
    held(w, a)
}

fn held(w: &World, a: ActorRef) -> Option<ChargeVals> {
    crate::actors::world_actor::actor_charge(w, a).copied().flatten()
}

fn hold(w: &mut World, a: ActorRef, c: Option<ChargeVals>) {
    if let Some(slot) = crate::actors::world_actor::actor_charge_mut(w, a) {
        *slot = c;
    }
}

/// `receiveCharge`: SpawnedActorParent ReceiveBeginPlay ->
/// GetChargeValuesFromInstigator @11459-12608 (from FinishSpawningActor, before the spawner's BPI_UpdateHitbox): with
/// ReceiveChargeValuesFromInstigator, a valid Instigator and both components, the four values. PercentCharged is not
/// clamped here. An actor another actor spawns has the owning fighter as its Instigator (Comp_Spawner @9238 / @9683).
pub fn receive_charge(w: &mut World, a: ActorRef, c: Option<ChargeVals>) {
    let p = crate::actors::world_actor::actor_p(w, a);
    if !crate::actors::projectile::truthy(p.get("ReceiveChargeValuesFromInstigator")) {
        return;
    }
    hold(w, a, c);
}

/// `setChargeVariables`: Comp_Spawner ActivateActors with UpdateCharge
/// (AnimNotify_ActivateSpawned): per actor, SetChargeVariables(the actor, GetOwner) @3727 and then BPI_ActivateObject
/// @3610. PercentCharged is clamped to [0, 1] @1360.
pub fn set_charge_variables(w: &mut World, a: ActorRef, c: Option<ChargeVals>) {
    let c = c.map(|c| {
        if c.pct < 0.0 || c.pct > 1.0 { ChargeVals { pct: math::min(math::max(c.pct, 0.0), 1.0), ..c } } else { c }
    });
    hold(w, a, c);
}

/// `chargeMovement`: GetChargeValue(3), MovementMultiplier: 0 unless the charge's
/// montage is the active one; stored as 1 + (MaxMovementMultiplier - 1) x the fraction charged (UpdateChargeValues),
/// read with the charge notify's MaxMovementMultiplier.
pub fn charge_movement(w: &World, f: FighterId) -> Real {
    let c = &w.fighter(f).charge;
    if c.charge_notify < 0 || !charge_montage_active(w, f) {
        return 0.0;
    }
    let Some(r) = crate::fighter::anim::resolve(w, f, crate::real(c.charge_notify)) else { return 0.0 };
    let m = notify_p(w, f, r.n).and_then(|p| p.get("MaxMovementMultiplier")).and_then(Value::as_f64).unwrap_or(1.0);
    crate::math::narrow(1.0 + (m - 1.0) * f64::from(c.charge_pct))
}

/// A notify's properties (`n.p`), from the fighter's own montages (or a montage its second meshes play).
fn notify_p(w: &World, f: FighterId, n: crate::events::NotifyRef) -> Option<&Props> {
    crate::fighter::anim::montages::any_notify_in(&w.data, w, f, n).map(|x| &x.p)
}

// ---- the actor's buff list: the shared reads (elemental and projectile buffs)
//
// A buff child of a spawned actor's Comp_State_Manager is an entry of the actor's `elem.buffs` (ElemBuff); a class of
// the actor-side path runs its graph through its `BUFFS` entry's `actor` hooks, which take
// the entry's index in the list at the call (`ENDING` + its place in `ElemState::ending` for one whose EndPlay runs:
// [`elem_buff`] resolves both).

/// The index bit of an ending buff in a hook's call: the entry is `ElemState::ending[i & !ENDING]`.
pub const ENDING: u32 = 1 << 31;

const ELEM_FIRE: BuffClass = BuffClass::BuffElementalFire;
const ELEM_ICE: BuffClass = BuffClass::BuffElementalIce;
const FIREPROOF: BuffClass = BuffClass::BuffElementalFireproof;
const ICEPROOF: BuffClass = BuffClass::BuffElementalIceproof;
const KINDS: [BuffClass; 4] = [ELEM_FIRE, ELEM_ICE, FIREPROOF, ICEPROOF];
const SLOWS: [BuffClass; 2] = [BuffClass::BuffSlowProjectile, BuffClass::BuffIceSlowProjectile];
/// K2_SetTimerDelegate(ApplyFireOverTime / ApplyIceOverTime, 0.5, looping): Fire @7657, Ice @8021.
const TICK: Real = 0.5;

static NO_PROPS: std::sync::LazyLock<Props> = std::sync::LazyLock::new(Props::new);

fn num(p: Option<&Props>, k: &str) -> Option<Real> {
    p.and_then(|p| p.get(k)).and_then(Value::as_f64).map(crate::math::narrow)
}

fn flag(p: Option<&Props>, k: &str) -> bool {
    crate::actors::projectile::truthy(p.and_then(|p| p.get(k)))
}

fn obj<'a>(p: Option<&'a Props>, k: &str) -> Option<&'a Props> {
    p.and_then(|p| p.get(k)).and_then(Value::as_obj)
}

fn arr<'a>(p: Option<&'a Props>, k: &str) -> &'a [Value] {
    p.and_then(|p| p.get(k)).and_then(Value::as_arr).unwrap_or(&[])
}

/// A tick count kept as a number (whole, finite where it is stored).
fn ticks(r: Real) -> i32 {
    math::to_int32(r)
}

/// A stack timer of Infinity ticks (a rate of 0) is no timer.
fn timer_or_none(t: Real) -> Option<i32> {
    if t == Real::INFINITY { None } else { Some(ticks(t)) }
}

fn idx(i: usize) -> u32 {
    u32::try_from(i).expect("an actor holds fewer than 2^31 buffs")
}

fn state(w: &World, a: ActorRef) -> Option<&ElemState> {
    crate::actors::world_actor::actor_elem(w, a)?.as_ref()
}

fn state_mut(w: &mut World, a: ActorRef) -> Option<&mut ElemState> {
    crate::actors::world_actor::actor_elem_mut(w, a)?.as_mut()
}

/// The entry a hook was called with (its index in the list, or `ENDING` + its place among the ending ones).
pub fn elem_buff(w: &World, a: ActorRef, i: u32) -> Option<&ElemBuff> {
    let st = state(w, a)?;
    if i & ENDING != 0 {
        st.ending.get(usize::try_from(i & !ENDING).ok()?)
    } else {
        st.buffs.get(usize::try_from(i).ok()?)
    }
}

/// `elem_buff`, to write.
pub fn elem_buff_mut(w: &mut World, a: ActorRef, i: u32) -> Option<&mut ElemBuff> {
    let st = state_mut(w, a)?;
    if i & ENDING != 0 {
        st.ending.get_mut(usize::try_from(i & !ENDING).ok()?)
    } else {
        st.buffs.get_mut(usize::try_from(i).ok()?)
    }
}

/// The place in the list of the entry with this serial (`list.includes(b)`).
fn pos(w: &World, a: ActorRef, serial: u32) -> Option<usize> {
    state(w, a)?.buffs.iter().position(|b| b.serial == serial)
}

fn at(w: &World, a: ActorRef, serial: u32) -> Option<&ElemBuff> {
    state(w, a)?.buffs.iter().find(|b| b.serial == serial)
}

fn at_mut(w: &mut World, a: ActorRef, serial: u32) -> Option<&mut ElemBuff> {
    state_mut(w, a)?.buffs.iter_mut().find(|b| b.serial == serial)
}

fn serials(w: &World, a: ActorRef) -> Vec<u32> {
    state(w, a).map_or_else(Vec::new, |s| s.buffs.iter().map(|b| b.serial).collect())
}

/// The fighter an actor's class data is read through (`lookupOwner`).
fn data_owner(w: &World, a: ActorRef) -> FighterId {
    crate::fighter::common::lookup_owner(w, a)
}

/// Comp_Actor_Destructible.TeamIndex of a spawned actor (`ownerTeam`): its owner's, -1 with none.
fn team_of(w: &World, a: ActorRef) -> i32 {
    crate::actors::world_actor::actor_owner(w, a).map_or(-1, |f| w.fighter(f).team.team)
}

/// The class's `ActorDef` through the fighter its data is read through (`actorDef(lookupOwner(a), a.cls)`).
fn class_def(w: &World, a: ActorRef) -> Option<&mvs_data::fighter::ActorDef> {
    let cls = crate::actors::world_actor::actor_cls(w, a)?;
    crate::fighter::common::actor_class_def(w, data_owner(w, a), cls)
}

fn supers_has(d: Option<&mvs_data::fighter::ActorDef>, sup: &str) -> bool {
    d.and_then(|d| d.supers.as_deref()).is_some_and(|s| s.split(' ').any(|x| x == sup))
}

/// The buff class's `BUFFS` entry.
fn entry(w: &World, c: BuffClass) -> Option<&'static crate::registry::BuffEntry> {
    let r: &'static crate::registry::Registry = w.registry;
    r.buffs.get(c.name(&w.names))
}

fn hooks(w: &World, c: BuffClass) -> Option<ActorBuffHooks> {
    entry(w, c).and_then(|e| e.actor)
}

/// `buffDef(lookupOwner(a), cls)`: the class's defaults (`None`: no fighter of the match has the class).
pub(crate) fn bdef(w: &World, a: ActorRef, c: BuffClass) -> Option<&Props> {
    let n = w.names.get(c.name(&w.names))?;
    crate::fighter::common::buff_def(w, ActorRef::Fighter(data_owner(w, a)), n)
}

/// The data's name of a buff class (`Name::NONE` where no data names it).
fn class_name(w: &World, c: BuffClass) -> Name {
    w.names.get(c.name(&w.names)).unwrap_or(Name::NONE)
}

/// The actor's list, empty, with what its class answers: BPI_Elemental_System from its kind (`ActorKind.elemental`)
/// over SpawnedActorParent's defaults, CanBePooled from its class data (`newState`).
fn new_state(w: &World, a: ActorRef) -> ElemState {
    let k = crate::actors::world_actor::actor_entry(w, a).and_then(|e| e.elemental).unwrap_or_default();
    ElemState {
        bx: k.box_,
        overlap: k.overlap,
        pooled: flag(class_def(w, a).map(|d| &d.p), "CanBePooled"),
        sphere: k.sphere,
        ..ElemState::default()
    }
}

/// `state(a)` / `a.elem ??= newState(a)`: the list made on the first call that names it.
fn ensure(w: &mut World, a: ActorRef) {
    if !matches!(crate::actors::world_actor::actor_elem(w, a), Some(None)) {
        return;
    }
    let s = new_state(w, a);
    if let Some(slot) = crate::actors::world_actor::actor_elem_mut(w, a) {
        *slot = Some(s);
    }
}

/// `newElemBuff` pushed onto the list: its serial.
fn push(w: &mut World, a: ActorRef, class: BuffClass, stacks: i32) -> Option<u32> {
    let st = state_mut(w, a)?;
    let serial = st.next_serial;
    st.next_serial = st.next_serial.wrapping_add(1);
    st.buffs.push(ElemBuff {
        serial,
        class,
        stacks,
        timer: None,
        passive: false,
        speed_mod: None,
        life: None,
        stack: None,
        gate: None,
        add_at: None,
        ext: ElemExt::None,
    });
    Some(serial)
}

/// The entry out of the list (`elemDestroy`: a filter).
fn take(w: &mut World, a: ActorRef, serial: u32) -> Option<ElemBuff> {
    let st = state_mut(w, a)?;
    let i = st.buffs.iter().position(|b| b.serial == serial)?;
    st.removals = st.removals.wrapping_add(1);
    Some(st.buffs.remove(i))
}

/// `elemDestroy`: K2_DestroyActor on a buff child of the actor, for the classes of other
/// modules that live in this list.
pub fn elem_destroy(w: &mut World, a: ActorRef, serial: u32) {
    take(w, a, serial);
}

/// NewBuffSpawned(NewBuff) on the actor's state manager, for a run-time binder.
fn new_buff_spawned(w: &mut World, a: ActorRef, cls: BuffClass) {
    use crate::world::flow::{ActorList, DelegateCall, DelegateList, broadcast_bound};
    broadcast_bound(w, DelegateList::Actor(a, ActorList::NewBuffSpawned), &DelegateCall::NewBuffSpawned { cls });
}

/// A walk over the list as `for (i = 0; i < list.length; i++)` does over `a.elem.buffs`: the array it began
/// with, which a push extends until a removal makes the actor's list a new array. Refreshed while no removal was
/// made since the walk began.
// Unverified: an entry spawned in the same call as a removal, after it, is seen by the walk only if the push came
// first; the order within one hook is not followed.
struct Walk {
    list: Vec<u32>,
    removals: u32,
    i: usize,
}

impl Walk {
    fn new(w: &World, a: ActorRef) -> Option<Walk> {
        let st = state(w, a)?;
        Some(Walk { list: st.buffs.iter().map(|b| b.serial).collect(), removals: st.removals, i: 0 })
    }

    fn next(&mut self, w: &World, a: ActorRef) -> Option<u32> {
        if let Some(st) = state(w, a)
            && st.removals == self.removals
        {
            self.list.clear();
            self.list.extend(st.buffs.iter().map(|b| b.serial));
        }
        let s = self.list.get(self.i).copied();
        self.i += 1;
        s
    }
}

// ---- the call surface of the elemental buffs
//
// Read: Buff_ElementalParent, Buff_Elemental_Fire, Buff_Elemental_Ice, Comp_Pawn_PassiveBuffManager,
// Comp_State_Manager (SpawnBuffActor, SweepBuffState, RemoveBuffStack, CheckHasBuff).
//
// These buffs live on spawned actors (a SpawnedActorParent has a Comp_State_Manager and a
// Comp_Pawn_PassiveBuffManager), not on fighters. What they do to a fighter goes through the fighter's own buffs
// (Buff_Fire, Buff_Ice). The parent binds its handlers in BeginPlay (@362: authority, not OnHitElementOnly, a valid
// owner) to the owner's Comp_Pawn_HitBoxHandler (HitAnything @691, HitAlly @826, HitEnemyPlayer @890, HitNonPlayer
// @1025), its sphere (@1345 / @1409), box (@1674 / @1738) and non-root capsule (@2384 / @2448) from
// BPI_Elemental_System, the Comp_Pawn_DamageManager's HitByEnemy / HitByAlly (@1929 / @2064), the actor's own begin /
// end overlap when BPI_ShouldActorOverlap (@2684 / @2768) and SpawnedActorParent.DirectlyApplyElementals (@2964). The
// children fill in ElementalHitEnemy, ElementalHitAlly, the overlaps and a 0.5 s timer; every other handler is empty.
// Not here: TeamIndex -1 (a neutral owner: every branch that needs one is named where it is skipped), the buffs'
// emitters and sounds (`elem_shows` says when the fire emitter is on), the on-hit elementals of a hit packet
// (pfgDoDamage SpawnOnHitElementalBuffs @296: no ported hit passes any).

/// `spawnPassiveBuffs`: Comp_Pawn_PassiveBuffManager's BeginPlay on a SpawnedActorParent
/// child (@2273 -> @1108 -> @633) -> SpawnPassiveBuffs: each row with BuffSelf goes to the actor's own state manager,
/// SpawnBuffActor(Buff, actor, actor, StartingStacks, Duration) @1259. The four elemental classes are spawned by
/// `elem_spawn`, any other through its actor-side entry (`spawn_actor_buff`: SpawnBuffActor @440-842 takes a non-pawn
/// Instigator's owner). Called by every spawn route of the World ahead of the parent's ReceiveBeginPlay.
// Deviation: the manager waits for TeamIndexWasSetOnSpawn (@738, else the bind @1527); here the buffs are there from
// the spawn (the 0.5 s timer's first tick is counted from it).
// Not ported: a row with BuffAllies or BuffOpponents (@1592 / @1768): no class of the build's characters has one
// (warned).
pub fn spawn_passive_buffs(w: &mut World, a: ActorRef) {
    struct Row {
        buff: Option<Name>,
        text: Option<String>,
        cls: Option<BuffClass>,
        self_: bool,
        n: i32,
        duration: Real,
    }
    let rows: Vec<Row> = {
        let d = class_def(w, a);
        let list = arr(obj(d.map(|d| &d.components), "Comp_Pawn_PassiveBuffManager"), "Buffs");
        list.iter()
            .map(|r| {
                let r = r.as_obj();
                if flag(r, "BuffAllies") || flag(r, "BuffOpponents") {
                    crate::actors::world_actor::warn("a passive row for allies or opponents not ported");
                }
                let s = r.and_then(|r| r.get("Buff")).and_then(Value::as_str);
                Row {
                    buff: s.and_then(|s| w.names.get(s)),
                    text: s.map(str::to_owned),
                    cls: s.and_then(|s| w.registry.buffs.get(s)).map(|e| e.class),
                    self_: flag(r, "BuffSelf"),
                    n: ticks(num(r, "StartingStacks").unwrap_or(0.0)),
                    duration: num(r, "Duration").unwrap_or(0.0),
                }
            })
            .collect()
    };
    if rows.is_empty() {
        return;
    }
    ensure(w, a);
    let owner = crate::actors::world_actor::actor_owner(w, a);
    for r in rows {
        if !r.self_ {
            continue;
        }
        let b = match r.cls {
            Some(c) if KINDS.contains(&c) => elem_spawn(w, a, c, r.n),
            _ => match r.buff {
                Some(n) => spawn_actor_buff(w, a, n, r.n, r.duration, true, owner),
                None => {
                    // A Buff the match never interned is no class of the data: refused, as spawnActorBuff refuses
                    // a class it has no def of.
                    let cls = crate::actors::world_actor::actor_cls(w, a).map_or("?", |c| w.names.str(c));
                    let msg = format!("{cls}: passive buff {} not in the data", r.text.as_deref().unwrap_or("(none)"));
                    crate::fighter::common::warn_text(w, &msg);
                    None
                }
            },
        };
        if let Some(b) = b
            && let Some(e) = at_mut(w, a, b)
        {
            e.passive = true;
        }
    }
}

/// `elemOf`: Comp_State_Manager.CheckHasBuff on the actor, a live buff actor of the
/// class. A read: the making of the list on the way is [`ensure`]'s, at the callers that write.
pub fn elem_of(w: &World, a: ActorRef, c: BuffClass) -> Option<&ElemBuff> {
    state(w, a)?.buffs.iter().find(|b| b.class == c)
}

/// The StateInfo lists a spawn reads of the new class.
#[derive(Default)]
struct StateInfo {
    blocker_groups: Vec<Value>,
    blocker_buffs: Vec<Value>,
    break_groups: Vec<Value>,
    break_buffs: Vec<Value>,
}

impl StateInfo {
    fn of(d: &Props) -> Option<StateInfo> {
        let si = obj(Some(d), "StateInfo")?;
        Some(StateInfo {
            blocker_groups: arr(Some(si), "BlockerGroups").to_vec(),
            blocker_buffs: arr(Some(si), "BlockerBuffs").to_vec(),
            break_groups: arr(Some(si), "BreakGroups").to_vec(),
            break_buffs: arr(Some(si), "BreakBuffs").to_vec(),
        })
    }

    /// (blocks, breaks) for a live buff of group `g` and class `cls`.
    fn meets(&self, g: Option<&Value>, cls: &str) -> (bool, bool) {
        let named = |l: &[Value]| l.iter().any(|v| v.as_str() == Some(cls));
        let grouped = |l: &[Value]| g.is_some_and(|g| l.contains(g));
        (
            grouped(&self.blocker_groups) || named(&self.blocker_buffs),
            grouped(&self.break_groups) || named(&self.break_buffs),
        )
    }
}

/// A live buff's StateInfo.Group (`None`: none).
fn group_of(w: &World, a: ActorRef, c: BuffClass) -> Option<Value> {
    obj(bdef(w, a, c), "StateInfo").and_then(|s| s.get("Group")).cloned()
}

/// `elemSpawn`: Comp_State_Manager.SpawnBuffActor on the actor (fn @0-2460) with
/// SweepBuffState: a live buff of the class with CanOverride is destroyed and replaced; without it the spawn is
/// refused and, with CanStack and not PreventStacksWhenReapplied, the live one takes AddStack(StackCount) @1626
/// (below MaxStacks: the count, or 1 for 0, clamped to MaxStacks). Then StateInfo: a live buff named in BlockerBuffs
/// (or of a group in BlockerGroups) refuses it, one named in BreakBuffs (BreakGroups) is destroyed. BeginPlay:
/// CurrentStackAmount = SpawnedStackCount with no clamp (BuffParentActor @1207); Fire @7657 and Ice @7671-8021 start
/// the looping 0.5 s timer when the owner's BPI_GetBoxCollision is valid. The new entry's serial; `None`: refused.
pub fn elem_spawn(w: &mut World, a: ActorRef, c: BuffClass, stacks: i32) -> Option<u32> {
    ensure(w, a);
    let d = bdef(w, a, c)?;
    let can_override = flag(Some(d), "CanOverride");
    let max = num(Some(d), "MaxStacks").unwrap_or(1.0);
    let can_stack = flag(Some(d), "CanStack") && !flag(Some(d), "PreventStacksWhenReapplied");
    let si = StateInfo::of(d);
    let old = elem_of(w, a, c).map(|b| (b.serial, b.stacks));
    if let Some((s, n)) = old
        && !can_override
    {
        if can_stack && crate::real(n) < max {
            let add = if stacks == 0 { 1 } else { stacks };
            let v = math::min(math::max(crate::real(n) + crate::real(add), 0.0), max);
            if let Some(b) = at_mut(w, a, s) {
                b.stacks = ticks(v);
            }
        }
        return None;
    }
    if let Some((s, _)) = old {
        take(w, a, s);
    }
    if let Some(si) = si {
        let mut blocked = false;
        for s in serials(w, a) {
            let Some(b) = at(w, a, s) else { continue };
            let bc = b.class;
            let g = group_of(w, a, bc);
            let (blocks, breaks) = si.meets(g.as_ref(), bc.name(&w.names));
            blocked |= blocks;
            if breaks {
                take(w, a, s);
            }
        }
        if blocked {
            return None;
        }
    }
    let bx = state(w, a).is_some_and(|s| s.bx);
    let serial = push(w, a, c, stacks)?;
    if (c == ELEM_FIRE || c == ELEM_ICE) && bx {
        let t = ticks(crate::fighter::common::timer_frames(Some(TICK)));
        if let Some(b) = at_mut(w, a, serial) {
            b.timer = Some(t);
        }
    }
    // Unverified: NewBuffSpawned is broadcast at the end of SpawnBuffActor, behind the buff's BeginPlay.
    new_buff_spawned(w, a, c);
    Some(serial)
}

/// `elemRemoveStack`: Comp_State_Manager.RemoveBuffStack(class, n) on the actor:
/// RemoveStack(n) on each buff of the class (BuffParentActor @652-923: the count less n, or 1 for 0; destroyed at 0
/// or less).
pub fn elem_remove_stack(w: &mut World, a: ActorRef, c: BuffClass, n: i32) {
    ensure(w, a);
    let Some(st) = state_mut(w, a) else { return };
    let n = if n == 0 { 1 } else { n };
    for b in st.buffs.iter_mut().filter(|b| b.class == c) {
        b.stacks = b.stacks.wrapping_sub(n);
    }
    st.buffs.retain(|b| b.class != c || b.stacks > 0);
    st.removals = st.removals.wrapping_add(1);
}

/// `elemShows`: ElementalBuffVisuals (Fire @9470, Ice @8893): no emitter on an owner with
/// the proof buff of the element, nor when the elemental is the owner's own passive. For the drawing side.
pub fn elem_shows(w: &World, a: ActorRef, c: BuffClass) -> bool {
    let proof = if c == ELEM_FIRE { FIREPROOF } else { ICEPROOF };
    elem_of(w, a, c).is_some_and(|b| !b.passive) && elem_of(w, a, proof).is_none()
}

/// `elemTick`: the actor's tick, the buffs' looping timers. `fighters`: the BaseFighters
/// GetOverlappingActors gives (the owner actor's own overlaps with BPI_ShouldActorOverlap, else its box's, or its
/// sphere's), allies and the actor's own fighter included. `actors`: the SpawnedActorParents its box or sphere
/// overlaps (read by Buff_Elemental_Electric alone), the host itself left out. A timer of another class is that
/// class's (`ActorBuffHooks::overlap_timer`): its handler runs when the count reaches 0 and sets the count again.
// Deviation: the timers are counted in the host's tick, not in the world's timer pass (as Fire's and Ice's).
pub fn elem_tick(w: &mut World, a: ActorRef, fighters: &[FighterId], actors: &[ActorRef]) {
    // The World's buffer, taken out for the walk (a nested walk finds it empty and grows its own).
    let mut list = std::mem::take(&mut w.scratch.elem_serials);
    list.clear();
    if let Some(st) = state(w, a) {
        list.extend(st.buffs.iter().map(|b| b.serial));
    }
    for &s in &list {
        let Some(b) = at_mut(w, a, s) else { continue };
        let Some(t) = b.timer else { continue };
        let t = t - 1;
        b.timer = Some(t);
        if t > 0 {
            continue;
        }
        let c = b.class;
        if c == ELEM_FIRE || c == ELEM_ICE {
            // looping: set again inside the timer tick
            b.timer = Some(ticks(crate::combat::hit::timer_ticks(Some(TICK))));
            over_time(w, a, fighters, c == ELEM_FIRE);
            continue;
        }
        let Some(k) = hooks(w, c).and_then(|h| h.overlap_timer) else {
            crate::actors::world_actor::warn("a timer with no overlapTimer entry");
            if let Some(b) = at_mut(w, a, s) {
                b.timer = None;
            }
            continue;
        };
        let Some(i) = pos(w, a, s) else { continue };
        k(w, a, idx(i), fighters, actors);
    }
    w.scratch.elem_serials = list;
}

/// Buff_Elemental_Fire.ApplyFireOverTime (fn @0-7000): another team @1298 gets SpawnBuffActor(Buff_Fire, fighter,
/// the owner actor, 1, 0.0) @1647; the same team loses 20 stacks of Buff_Ice @3443. Buff_Elemental_Ice.ApplyIceOverTime,
/// the same shape: another team @1814 gets Buff_Ice @2245; the same team loses 20 stacks of Buff_Fire @3720.
// Not ported: with a TeamIndex of -1 on either side the enemy branch goes on: Fire posts Audio_Ignite_Start and
// removes 20 stacks of Buff_Ice @2267-2606; Ice removes 20 stacks of Buff_Ice, the element it has just given, with no
// sound @2865-3190. Nor the actors' half (Fire @3552-6872, Ice @4280-7118: it acts only where either team is -1).
fn over_time(w: &mut World, a: ActorRef, fighters: &[FighterId], fire: bool) {
    let (give, take) = if fire {
        (crate::name!("Buff_Fire_C"), crate::name!("Buff_Ice_C"))
    } else {
        (crate::name!("Buff_Ice_C"), crate::name!("Buff_Fire_C"))
    };
    let team = team_of(w, a);
    let owner = crate::actors::world_actor::actor_owner(w, a);
    for &vic in fighters {
        if w.fighter(vic).team.team != team {
            crate::combat::buffs::spawn_buff(w, vic, give, 0.0, Some(1), owner, true, None);
        } else {
            crate::combat::buffs::remove_buff_stack(w, vic, take, 20);
        }
    }
}

/// `elemHitEnemy`: HitEnemyPlayer(HitActor, the dealer), once the hit went through
/// (pfgDoDamage @22770). Buff_Elemental_Fire.ElementalHitEnemy @8429: an owner with BPI_ShouldActorOverlap against a
/// victim that has Buff_Fire tops it up to the elemental's stacks (SpawnBuffActor(Buff_Fire, stacks - the victim's,
/// 0.0) @9062 when that is above 0); otherwise SpawnBuffActor(Buff_Fire, CurrentStackAmount, 0.0) @9225, and a spawn
/// that returns no actor destroys the elemental @9432. A spawn that returned one goes to @15: DestroyAfterApplied
/// destroys the elemental unless the owner is a pooled SpawnedActorParent. Buff_Elemental_Ice.ElementalHitEnemy
/// @8588: SpawnBuffActor(Buff_Ice, 1, 20.0) @8761, then the elemental of an owner that is no fighter @8855.
pub fn elem_hit_enemy(w: &mut World, a: ActorRef, vic: FighterId) {
    let Some(st) = state(w, a) else { return };
    let (overlap, pooled) = (st.overlap, st.pooled);
    let list: Vec<(u32, BuffClass, i32, ElemExt)> =
        st.buffs.iter().map(|b| (b.serial, b.class, b.stacks, b.ext)).collect();
    let owner = crate::actors::world_actor::actor_owner(w, a);
    let fire = crate::name!("Buff_Fire_C");
    for (s, c, stacks, ext) in list {
        if c == ELEM_FIRE {
            let has = crate::combat::buffs::buff_of(w, vic, fire).and_then(|r| fighter_buff_stacks(w, r));
            if overlap && let Some(hs) = has {
                let diff = crate::real(stacks) - hs;
                if diff <= 0.0
                    || crate::combat::buffs::spawn_buff(w, vic, fire, 0.0, Some(ticks(diff)), owner, true, None)
                        .is_none()
                {
                    continue;
                }
            } else if crate::combat::buffs::spawn_buff(w, vic, fire, 0.0, Some(stacks), owner, true, None).is_none() {
                elem_destroy(w, a, s);
                continue;
            }
            let dab = ext.destroy_after_applied().unwrap_or_else(|| flag(bdef(w, a, ELEM_FIRE), "DestroyAfterApplied"));
            if dab && !pooled {
                elem_destroy(w, a, s);
            }
        } else if c == ELEM_ICE {
            crate::combat::buffs::spawn_buff(w, vic, crate::name!("Buff_Ice_C"), 20.0, Some(1), owner, true, None);
            elem_destroy(w, a, s);
        }
    }
}

/// A fighter buff's CurrentStackAmount.
fn fighter_buff_stacks(w: &World, r: crate::combat::BuffRef) -> Option<Real> {
    w.fighter(r.fighter).buffs.buffs.iter().find(|b| b.serial == r.serial).map(|b| b.stacks)
}

/// `elemHitAlly`: HitAlly: Fire @7944 takes 99 stacks of Buff_Ice and 1 of
/// Buff_Elemental_Ice (@8272) from the ally, Ice @8091 takes 2 of Buff_Fire and 2 of Buff_Elemental_Fire (@8429).
// The build's second RemoveBuffStack of each pair is made too.
pub fn elem_hit_ally(w: &mut World, a: ActorRef, ally: FighterId) {
    use crate::combat::buffs::remove_buff_stack;
    let Some(st) = state(w, a) else { return };
    let list: Vec<BuffClass> = st.buffs.iter().map(|b| b.class).collect();
    for c in list {
        if c == ELEM_FIRE {
            remove_buff_stack(w, ally, crate::name!("Buff_Ice_C"), 99);
            remove_buff_stack(w, ally, crate::name!("Buff_Elemental_Ice_C"), 1);
        } else if c == ELEM_ICE {
            remove_buff_stack(w, ally, crate::name!("Buff_Fire_C"), 2);
            remove_buff_stack(w, ally, crate::name!("Buff_Elemental_Fire_C"), 2);
        }
    }
}

/// `elemOverlap`: the begin of an overlap of the owner with another spawned actor: the
/// actor's own begin overlap with BPI_ShouldActorOverlap (Fire @14501), the box's without it (@12718).
/// ElementalFireOverlap(other): the same team: a SpawnedActorParent with no Fireproof gets SpawnBuffActor(Elemental_Fire,
/// other, other, CurrentStackAmount, 0.0) @1949. ElementalIceOverlap(other) (no Iceproof @507): another team @807: an
/// `other` that has Elemental_Fire loses 2 stacks of it @1170; the same team gets Elemental_Ice @1375.
// Unverified: which pairs of actors overlap (the collision channels and responses of each class's components) is
// not read: the caller decides, and no ported class calls this yet.
// Not ported: an `other` of team -1 in Fire's branch (@1031-1666); Ice's is `ice_overlap`'s.
pub fn elem_overlap(w: &mut World, a: ActorRef, other: ActorRef) {
    let Some(st) = state(w, a) else { return };
    if other == a || (!st.overlap && !st.bx) {
        return;
    }
    let list: Vec<(BuffClass, i32)> = st.buffs.iter().map(|b| (b.class, b.stacks)).collect();
    let same = team_of(w, other) == team_of(w, a);
    // `elemOf` makes the other's list here: the fire branch with the same team, the ice branch.
    for (c, stacks) in list {
        if c == ELEM_FIRE {
            if !same {
                continue;
            }
            ensure(w, other);
            if elem_of(w, other, FIREPROOF).is_none() {
                // Buff_Elemental_Fire ElementalFireOverlap @1859
                crate::world::flow::tutorial_event(w, crate::fighter::match_side::TutorialEvent::ElementalFireBuff);
                elem_spawn(w, other, ELEM_FIRE, stacks);
            }
        } else if c == ELEM_ICE {
            ice_overlap(w, other, same);
        }
    }
}

/// ElementalIceOverlap(other) past its fighter test @65 (the callers pass no fighter): no Iceproof @507-666; another
/// team @807: an `other` that has Elemental_Fire loses 2 stacks of it @1170; the same team gets
/// SpawnBuffActor(Elemental_Ice, other, the elemental's owner, 1, 0.0) @1375 (the spawn takes no instigator here).
// Not ported: another team without Elemental_Fire whose TeamIndex is -1 goes to the spawn @1464.
fn ice_overlap(w: &mut World, other: ActorRef, same: bool) {
    ensure(w, other);
    if elem_of(w, other, ICEPROOF).is_some() {
        return;
    }
    if same {
        elem_spawn(w, other, ELEM_ICE, 1);
    } else if elem_of(w, other, ELEM_FIRE).is_some() {
        elem_remove_stack(w, other, ELEM_FIRE, 2);
    }
}

/// The begin of an overlap of the owner's BPI_GetSphereCollision component with another actor: Buff_ElementalParent's
/// BeginPlay binds ElementalOnSphereBeginOverlap to a valid sphere whatever BPI_ShouldActorOverlap answers
/// (@1260-1432). Ice's event runs ElementalIceOverlap(other) (@12034 -> @12098); Fire's runs nothing (@13219). A
/// fighter `other` returns at the cast @65.
pub fn elem_sphere_overlap(w: &mut World, a: ActorRef, other: ActorRef) {
    let Some(st) = state(w, a) else { return };
    if other == a || matches!(other, ActorRef::Fighter(_)) {
        return;
    }
    // (an actor holds one Elemental_Ice at most: a second spawn stacks or replaces it)
    if !st.buffs.iter().any(|b| b.class == ELEM_ICE) {
        return;
    }
    let same = team_of(w, other) == team_of(w, a);
    ice_overlap(w, other, same);
}

// ---- the call surface of the projectile buffs
//
// Buffs a graph puts on another kit's spawned actors (a projectile, a thrown item), and what such a graph writes on
// their ProjectileMovement. Read: Buff_AlterProjectileSpeed, Buff_Slow30PercentProjectile; the caller that reaches
// them, Buff_C021_SetHomingTowardsTarget, passes per actor: SpawnBuffActor(Buff_AlterProjectileSpeed_C, actor, its
// owner, 0, DeBuffsDuration 9, BreakIfBlocked true, false) @807 / @2050, ProjectileHomingMulticast
// (bIsHomingProjectile, 20000, the buff owner's root) @2950-3134, UpdateMaxLifetimeWithAuthority(9) @1071.
// A class takes this path by its `BUFFS` entry's `actor` hooks; a class without them is refused (warned).

/// The place in the actor's buff list of the entry with this serial (`spawn_actor_buff` gives the serial; the
/// list's readers take the place).
pub fn buff_place(w: &World, a: ActorRef, serial: u32) -> Option<u32> {
    let i = crate::actors::world_actor::actor_elem(w, a)?.as_ref()?.buffs.iter().position(|b| b.serial == serial)?;
    u32::try_from(i).ok()
}

/// `spawnActorBuff`: Comp_State_Manager.SpawnBuffActor(cls, the actor, ..., stacks,
/// duration, BreakIfBlocked) on a spawned actor. SweepBuffState over the actor's live buffs in their order:
/// - one of the class @1364: CanOverride -> K2_DestroyActor at once @1452 and the loop goes on; else LocalBlocked
///   @1489, AddStack(StackCount) with CanStack and not PreventStacksWhenReapplied @1505-1626, and
///   RefreshBuff(duration) @1709-2041 when CanRefresh and ((its LifeTimer is active and (the new duration > what is
///   left, or <= 0)) or AlwaysRefresh); the loop goes on;
/// - another class @2087: its StateInfo.Group in the new class's BlockerGroups, or its class in BlockerBuffs ->
///   LocalBlocked @2549; in BreakGroups / BreakBuffs -> LocalBuffsToDestroy (AddUnique) @2885.
///
/// After the loop @842-1208: with !LocalBlocked || BreakIfBlocked each of LocalBuffsToDestroy is destroyed; the spawn
/// is refused while LocalBlocked. Then BuffParentActor's BeginPlay: SetLifeTimer(duration) (0: no timer), the stack
/// timers (@1376-1431 SetStackLifeTimer, @1454 SetMinStackTimer) and the class's own ReceiveBeginPlay (`from`: the
/// Instigator, for a class whose graph reads it). The new entry's serial; `None`: refused, or destroyed in its
/// BeginPlay.
pub fn spawn_actor_buff(
    w: &mut World,
    a: ActorRef,
    cls: Name,
    stacks: i32,
    duration: Real,
    break_if_blocked: bool,
    from: Option<FighterId>,
) -> Option<u32> {
    if cls.is_none() {
        return None; // a None class spawns no actor
    }
    let r: &'static crate::registry::Registry = w.registry;
    let Some((class, k)) = r.buffs.get(w.names.str(cls)).and_then(|e| Some((e.class, e.actor?))) else {
        crate::actors::world_actor::warn("a buff on a spawned actor: not ported (no actor-side entry)");
        return None;
    };
    let d = bdef(w, a, class)?; // warns for a class no fighter of the match has
    let si = StateInfo::of(d);
    let min_stack_speed = num(Some(d), "MinStackSpeed");
    let stack_begin = crate::combat::buffs::stack_life_begin(d);
    ensure(w, a);
    let mut blocked = false;
    let mut doomed: Vec<u32> = Vec::new();
    for s in serials(w, a) {
        let Some(o) = at(w, a, s) else { continue }; // destroyed inside the loop (IsActorBeingDestroyed @785)
        let (oc, oext, olife) = (o.class, o.ext, o.life);
        let od = bdef(w, a, oc);
        if oc == class {
            // CanOverride: the instance's where the class graph writes it, else the class default.
            if oext.can_override().unwrap_or_else(|| flag(od, "CanOverride")) {
                end_actor_buff(w, a, s);
                continue;
            }
            blocked = true;
            let (can_refresh, always) = (flag(od, "CanRefresh"), flag(od, "AlwaysRefresh"));
            if flag(od, "CanStack") && !flag(od, "PreventStacksWhenReapplied") && add_actor_stack(w, a, s, stacks) {
                end_actor_buff(w, a, s);
                continue;
            }
            if can_refresh {
                let tf = crate::fighter::common::timer_frames(Some(duration));
                if ((olife.is_some_and(|l| tf > crate::real(l) || duration <= 0.0)) || always)
                    && let Some(o) = at_mut(w, a, s)
                {
                    o.life = if duration > 0.0 { Some(ticks(tf)) } else { None };
                }
            }
            continue;
        }
        let Some(si) = &si else { continue };
        let g = group_of(w, a, oc);
        let (blocks, breaks) = si.meets(g.as_ref(), oc.name(&w.names));
        blocked |= blocks;
        if breaks && !doomed.contains(&s) {
            doomed.push(s);
        }
    }
    if !blocked || break_if_blocked {
        for s in doomed {
            end_actor_buff(w, a, s);
        }
    }
    if blocked {
        return None;
    }
    let s = push(w, a, class, stacks)?;
    let life = (duration > 0.0).then(|| ticks(crate::fighter::common::timer_frames(Some(duration))));
    let gate = min_stack_speed
        .filter(|&m| m != 0.0 && !m.is_nan())
        .map(|m| ticks(crate::fighter::common::timer_frames(Some(m))));
    if let Some(b) = at_mut(w, a, s) {
        b.life = life;
        b.stack = timer_or_none(stack_begin);
        b.gate = gate;
    }
    if let Some(begin) = k.begin {
        let i = pos(w, a, s).map(idx)?;
        if begin(w, a, i, from) {
            end_actor_buff(w, a, s);
            return None;
        }
    }
    actor_size_update(w, a); // SpawnBuffActor @1382 UpdatePlayerStatus(the new buff) -> ApplySizeBuffs
    new_buff_spawned(w, a, class);
    Some(s)
}

/// BuffParentActor AddStack(StackCount) on a buff of the actor (ubergraph @3954, authority), as a fighter's
/// addStack: below MaxStacks, nothing while MinStackPreventedGate is closed (@4036-4065), else the count +
/// (StackCount, or 1 for 0) clamped @428-620; at MaxStacks no count (@4022 -> @169). Past the gate SetStackLifeTimer
/// @169-288 and the owner's UpdateCurrentStates @311-382 -> ApplySizeBuffs. Then the class's own AddStack behind the
/// parent's call, whether or not the gate stopped it; true: it destroyed the buff. A class with NewStackApplyBuffer
/// (Buff_Ice's AddStack override @2476-2704): nothing at all unless GetGameTimeInSeconds - TimeSinceLastAdd >=
/// NewStackApplyBuffer; then TimeSinceLastAdd = now @2599-2628. On an actor TimeSinceLastAdd starts at its default 0
/// (the BeginPlay write @1198 is behind the fighter cast @1159).
// Not ported: the BuffStacksAdded broadcast @1671 on the actor's state manager (nothing an actor's buff binds).
fn add_actor_stack(w: &mut World, a: ActorRef, s: u32, n: i32) -> bool {
    let Some(i) = pos(w, a, s) else { return false };
    let Some(b) = at(w, a, s) else { return false };
    let (stacks, gate, stack, add_at, class) = (b.stacks, b.gate, b.stack, b.add_at, b.class);
    let di = actor_inst_def(w, a, idx(i));
    let di = di.as_deref().unwrap_or(&NO_PROPS);
    let buffer = num(Some(di), "NewStackApplyBuffer");
    let max = num(Some(di), "MaxStacks").unwrap_or(1.0);
    let cur = stack.map_or(Real::INFINITY, crate::real);
    let added = crate::combat::buffs::stack_life_added(di, cur);
    let now = w.flow.tick;
    if let Some(buf) = buffer {
        // TimeSeconds now less TimeSeconds at the last add (the world's float sum of DeltaSeconds; 0 unset).
        let dt = w.time_seconds - crate::combat::buffs::time_seconds_at(add_at.unwrap_or(0));
        if !(dt >= buf) {
            return false;
        }
    }
    let gated = crate::real(stacks) < max && gate.is_some();
    let mut capped = false;
    if let Some(b) = at_mut(w, a, s) {
        if buffer.is_some() {
            b.add_at = Some(now);
        }
        if !gated {
            if crate::real(b.stacks) < max {
                let add = if n == 0 { 1 } else { n };
                b.stacks = ticks(math::min(math::max(crate::real(b.stacks) + crate::real(add), 0.0), max));
                capped = crate::real(b.stacks) == max;
            }
            b.stack = timer_or_none(added);
        }
    }
    // BPI On Max Stacks @77-129 of the parent's AddStack, as the fighter side's (`buffs::add_stack`): a hook that
    // destroys the buff ends the call there.
    if capped
        && let Some(h) = hooks(w, class).and_then(|h| h.max_stacks)
        && let Some(i) = pos(w, a, s)
        && h(w, a, idx(i))
    {
        return true;
    }
    if !gated {
        actor_size_update(w, a);
    }
    let Some(f) = hooks(w, class).and_then(|h| h.stacks) else { return false };
    let Some(i) = pos(w, a, s) else { return false };
    f(w, a, idx(i), true)
}

/// `actorRemoveStack`: BuffParentActor RemoveStack(n) on a buff of the actor
/// (ubergraph @652-1196): the count less n (or 1 for 0) with no clamp @662-793, K2_DestroyActor at 0 or less
/// @875-923; else the owner's UpdateCurrentStates @1009, SetStackLifeTimer(StackDecayRate) @1054-1173 (from the stack
/// timer's own TimedRemoveStack @5570: `timed`) and the class's own RemoveStack behind the parent's.
pub fn actor_remove_stack(w: &mut World, a: ActorRef, s: u32, n: i32, timed: bool) {
    let Some(b) = at_mut(w, a, s) else { return };
    b.stacks = b.stacks.wrapping_sub(if n == 0 { 1 } else { n });
    if b.stacks <= 0 {
        end_actor_buff(w, a, s);
        return;
    }
    let class = b.class;
    actor_size_update(w, a);
    let Some(i) = pos(w, a, s) else { return };
    let cur = at(w, a, s).and_then(|b| b.stack).map_or(Real::INFINITY, crate::real);
    let d = actor_inst_def(w, a, idx(i));
    let d = d.as_deref().unwrap_or(&NO_PROPS);
    let t = if timed {
        crate::combat::buffs::stack_life_timed(d)
    } else {
        crate::combat::buffs::stack_life_removed(d, cur)
    };
    if let Some(b) = at_mut(w, a, s) {
        b.stack = timer_or_none(t);
    }
    let Some(f) = hooks(w, class).and_then(|h| h.stacks) else { return };
    if f(w, a, idx(i), false) {
        end_actor_buff(w, a, s);
    }
}

/// `endActorBuff`: K2_DestroyActor on one of these buffs: out of the list, its
/// EndPlay in the call (the class's `end`, which finds it among the ending ones), then BuffParentActor EndPlay ->
/// UpdateCurrentStates -> UpdatePlayerStatus(null).
pub fn end_actor_buff(w: &mut World, a: ActorRef, s: u32) {
    let Some(b) = take(w, a, s) else { return };
    let class = b.class;
    let Some(st) = state_mut(w, a) else { return };
    st.ending.push(b);
    let depth = idx(st.ending.len() - 1);
    if let Some(end) = hooks(w, class).and_then(|h| h.end) {
        end(w, a, ENDING | depth);
    }
    if let Some(st) = state_mut(w, a) {
        st.ending.truncate(usize::try_from(depth).unwrap_or(0));
    }
    actor_size_update(w, a);
}

/// `endActorChildren`: the dying actor's buff children (SpawnedActorParent
/// ReceiveEndPlay -> ubergraph @15507: @15512 Comp_Actor_Parent.DestroyChildrenWithIgnore(ActorsToIgnore), ahead of
/// the DeathVFX and the death spawn). Every buff SpawnBuffActor gave the actor is a child of its Comp_Actor_Parent;
/// DestroyChildrenWithIgnore (@0-682) destroys each whose class is not in Ignore, which holds the classes of the
/// actor's Comp_Pawn_PassiveBuffManager.Buffs rows (@19655-20071). A class of another path in the same list
/// (elemental, ice) leaves the list with no EndPlay of its own run.
// Unverified: the children's order is the list's (spawn order); the build's is Map_Keys(Children) (a TMap's,
// insertion order while nothing was removed).
pub fn end_actor_children(w: &mut World, a: ActorRef) {
    if state(w, a).is_none_or(|s| s.buffs.is_empty()) {
        return;
    }
    let keep: Vec<String> = {
        let d = class_def(w, a);
        let rows = arr(obj(d.map(|d| &d.components), "Comp_Pawn_PassiveBuffManager"), "Buffs");
        rows.iter().filter_map(|r| r.as_obj()?.get("Buff")?.as_str()).map(str::to_owned).collect()
    };
    for s in serials(w, a) {
        let Some(b) = at(w, a, s) else { continue };
        if keep.iter().any(|k| k == b.class.name(&w.names)) {
            continue;
        }
        end_actor_buff(w, a, s);
    }
}

/// `removeActorBuff`: Comp_State_Manager.RemoveBuff(cls, group 0) on the actor:
/// every live buff of the class is destroyed, its EndPlay in the call. True: one was.
pub fn remove_actor_buff(w: &mut World, a: ActorRef, c: BuffClass) -> bool {
    let mut any = false;
    for s in serials(w, a) {
        if at(w, a, s).is_some_and(|b| b.class == c) {
            end_actor_buff(w, a, s);
            any = true;
        }
    }
    any
}

// ---- the buffs a fighter's buff children pass on to its actors

/// GetBuffRemainingDuration selected against BuffDuration (SpawnedActorParent GrantBuffsFromInstigator @659-713,
/// Comp_State_Manager UpdateBuffsOnReflect @1714-1768): the life timer's time left when it is active, else the
/// actor's BuffDuration: 0 for one without a life timer.
// Deviation: the time left is the buff's whole ticks left on the timer manager's double clock, narrowed as
// GetTimerRemaining returns it (the timer's Rate past its last whole tick is not kept); no class of the data has a
// flag below with a life timer (the perk buffs are spawned with 0).
fn remaining_or_duration(frames: Real) -> Real {
    if frames == Real::INFINITY { 0.0 } else { math::narrow(crate::consts::clock(math::max(frames - 1.0, 0.0))) }
}

/// A fighter's live buffs as a walk takes them (`[...owner.buffs]`): serial, class, frames, stacks.
fn fighter_buffs(w: &World, f: FighterId) -> Vec<(u32, Name, Real, Real)> {
    w.fighter(f).buffs.buffs.iter().map(|b| (b.serial, b.cls, b.frames, b.stacks)).collect()
}

/// The flag `key` of each live buff of f (`instDef(owner, b)`: the entry's own values over its class's) passed on
/// to the actor.
fn pass_on(w: &mut World, a: ActorRef, f: FighterId, key: &str, from: Option<FighterId>) {
    for (serial, cls, frames, stacks) in fighter_buffs(w, f) {
        if frames <= 0.0 {
            continue;
        }
        let d = crate::combat::buffs::inst_def_of(w, crate::combat::BuffRef { fighter: f, serial });
        if !flag(d.as_deref(), key) {
            continue;
        }
        spawn_actor_buff(w, a, cls, ticks(stacks), remaining_or_duration(frames), true, from);
    }
}

/// `grantSpawnedBuffs`: SpawnedActorParent BeginPlay -> GrantBuffsFromInstigator
/// @9233 (authority; behind the team set-up @11027 and ahead of the parent's binds @9114 and of the child class's own
/// BeginPlay part): over Map_Keys of GetOwner()'s Comp_Actor_Parent.Children, each valid BuffParentActor with
/// GiveBuffToSpawnedActors (@627) gives the actor its class: SpawnBuffActor(class, the actor, the actor,
/// CurrentStackAmount, remaining-or-BuffDuration, true, false) @819.
// Unverified: the Children keys walk the slots in ascending order, a removed slot reused by the next new key; the
// port walks the fighter's buff list, spawn order with the ended ones taken out.
pub fn grant_spawned_buffs(w: &mut World, a: ActorRef, owner: FighterId) {
    pass_on(w, a, owner, "GiveBuffToSpawnedActors", None);
}

/// `grantActorBuffs`: GrantBuffsFromInstigator on an actor of the World's other
/// lists: every SpawnedActorParent child runs it in its BeginPlay (@9233). A class the data does not show as such a
/// child (its 'supers') takes none.
// Unverified: called at the actor's construction in the world's spawn, after the kind's own spawn code (its class's
// BeginPlay part comes after the parent's in the build).
pub fn grant_actor_buffs(w: &mut World, a: ActorRef) {
    let Some(o) = crate::actors::world_actor::actor_owner(w, a) else { return };
    if w.fighter(o).buffs.buffs.is_empty() {
        return;
    }
    let Some(cls) = crate::actors::world_actor::actor_cls(w, a) else { return };
    if !supers_has(crate::fighter::common::actor_class_def(w, o, cls), "SpawnedActorParent_C") {
        return;
    }
    grant_spawned_buffs(w, a, o);
}

/// `updateBuffsOnReflect`: SpawnedActorParent BPI_Reflected with ShouldPause
/// (@5073) -> the actor's Comp_State_Manager UpdateBuffsOnReflect(NewOwner): its own buff children with
/// DestroyBuffWhenOwnerReflected destroyed (@746-778), then over NewOwner's Children each with
/// GiveBuffToReflectedActors (@1682): SpawnBuffActor(class, the actor, NewOwner as Instigator, CurrentStackAmount,
/// remaining-or-BuffDuration, true, false) @1894.
pub fn update_buffs_on_reflect(w: &mut World, a: ActorRef, new_owner: FighterId) {
    for s in serials(w, a) {
        let Some(c) = at(w, a, s).map(|b| b.class) else { continue };
        if flag(bdef(w, a, c), "DestroyBuffWhenOwnerReflected") {
            end_actor_buff(w, a, s);
        }
    }
    pass_on(w, a, new_owner, "GiveBuffToReflectedActors", Some(new_owner));
}

/// `actorInstDef`: a buff's values on the actor, its class defaults under what its
/// graph wrote (`ActorBuffHooks::values`).
pub fn actor_inst_def(w: &World, a: ActorRef, i: u32) -> Option<std::borrow::Cow<'_, Props>> {
    let c = elem_buff(w, a, i)?.class;
    let d = bdef(w, a, c)?;
    let own = hooks(w, c).and_then(|h| h.values).and_then(|v| v(w, a, i));
    Some(match own {
        Some(own) => {
            let mut m = d.clone();
            for (k, v) in own {
                m.insert(k, v);
            }
            std::borrow::Cow::Owned(m)
        }
        None => std::borrow::Cow::Borrowed(d),
    })
}

/// A buff of the actor as the dealt pass walks it: its place, its class, and its damage mod struct (from its values
/// and stacks as the pass begins).
pub struct ActorDealtBuff {
    pub i: u32,
    pub class: BuffClass,
    pub mods: Option<crate::combat::buffs::DamageMod>,
}

/// `actorDealtDamageMods`: pfgDoDamage @5617-5904 on a spawned actor's handler:
/// the dealer actor's own Comp_State_Manager ApplyBuffDamageMods(hit, false, the victim, the actor, box)
/// (dealtModsOver) over the actor's buffs, each with its class's actor-side condition (`dealt_cond`, else
/// the parent's). A condition may write the World (Buff_ProjectileApplyBuff's Active, @64-188): it is called with
/// the World and the buff's place, as the taken pass's.
pub fn actor_dealt_damage_mods<'b>(
    w: &mut World,
    a: ActorRef,
    victim: ActorRef,
    p: &'b crate::combat::HitPacket,
    hit_box: bool,
) -> std::borrow::Cow<'b, crate::combat::HitPacket> {
    let Some(st) = state(w, a) else { return std::borrow::Cow::Borrowed(p) };
    if st.buffs.is_empty() {
        return std::borrow::Cow::Borrowed(p);
    }
    let wr: &World = w;
    let list: Vec<ActorDealtBuff> = st
        .buffs
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let def = actor_inst_def(wr, a, idx(i));
            let d = crate::combat::buffs::Def::of(def.as_deref());
            #[allow(clippy::cast_precision_loss, reason = "a stack count")]
            let n = b.stacks as Real;
            let mods = if d.some() { crate::combat::buffs::buff_damage_mod(wr, d, false, n) } else { None };
            ActorDealtBuff { i: idx(i), class: b.class, mods }
        })
        .collect();
    crate::combat::buffs::dealt_mods_over(
        &list,
        |b| b.mods.clone(),
        |b, view| {
            let view = view.packet();
            match hooks(w, b.class).and_then(|h| h.dealt_cond) {
                Some(f) => f(w, a, b.i, victim, view, hit_box),
                None => crate::combat::buffs::dealt_cond_default(w, class_name(w, b.class)),
            }
        },
        p,
    )
}

/// `actorHurtSphere`: the spheres the actor's buffs added (`hurt_sphere`) at its
/// root: the largest radius, 0 for none.
pub fn actor_hurt_sphere(w: &World, a: ActorRef) -> Real {
    let Some(st) = state(w, a) else { return 0.0 };
    let mut r = 0.0;
    for (i, b) in st.buffs.iter().enumerate() {
        if let Some(f) = hooks(w, b.class).and_then(|h| h.hurt_sphere) {
            r = math::max(r, f(w, a, idx(i)));
        }
    }
    r
}

/// `actorTakenDamage`: the taken pass of ApplyBuffDamageMods (Comp_State_Manager
/// ApplyDamageBuffsOfType for BuffModType 1, 2, 3 over the owner's buff actors, DamageTaken true) as DamageAndLaunch
/// @1878 makes it on a spawned actor hit by a fighter's hit box: the Damage field alone. Per pass: a buff whose
/// TakenDamageModCondition holds (`taken_cond`, the parent's true without one) and whose DamageModType is the pass's:
/// Override (3) writes at once; Multiply (1) and Add (2) add their change from the pass's input to an accumulator,
/// added at the pass's end; float32 steps.
// Unverified: the build's pass writes the packet's knockback and hitstun too, and the HitByEnemy broadcast behind it
// carries that modded packet; the port's hitBy gets the Hitframe's unmodified BaseKnockBack and direction.
// Not ported: a stacking class's per-stack struct is not read here (no ported actor buff stacks with mods: warned).
pub fn actor_taken_damage(w: &mut World, a: ActorRef, damage: Real, attacker: FighterId) -> Real {
    let Some(st) = state(w, a) else { return damage };
    if st.buffs.is_empty() {
        return damage;
    }
    let list: Vec<BuffClass> = st.buffs.iter().map(|b| b.class).collect();
    let mut x = damage;
    for t in [1.0, 2.0, 3.0] {
        let mut acc = 0.0;
        let mut hit = false;
        for (i, &c) in list.iter().enumerate() {
            let d = bdef(w, a, c);
            let Some(m) = obj(d, "TakenDamageMods") else { continue };
            if crate::combat::buffs::mod_type(m.get("DamageModType")) != t {
                continue;
            }
            if let Some(f) = hooks(w, c).and_then(|h| h.taken_cond)
                && !f(w, a, idx(i), attacker)
            {
                continue;
            }
            if flag(d, "CanStack") {
                crate::actors::world_actor::warn("a buff on a spawned actor: per-stack damage mods not ported");
            }
            let v = num(Some(m), "DamageMod").unwrap_or(0.0);
            if t == 3.0 {
                x = v;
            } else {
                hit = true;
                let r = if t == 1.0 { x * v } else { x + v };
                acc += r - x;
            }
        }
        if hit {
            x += acc;
        }
    }
    x
}

/// `actorBuffTick`: the actor's frame for its buffs (behind the actor's own tick):
/// each class's ReceiveTick, then its timers as a fighter's: MinStackPreventedGate's (TimedMinStackGate @5595 opens
/// it), the stack life timer's TimedRemoveStack @5570, then its LifeTimer (TimedDestroyActor at 0).
// Deviation: the timers are counted in the actor's pass, not in the world's timer pass (no class here does anything
// at their end that a later actor of the frame would read).
// Unverified: a stack timer and a life timer due on the same tick run in that order, as on a fighter.
pub fn actor_buff_tick(w: &mut World, a: ActorRef) {
    let Some(mut walk) = Walk::new(w, a) else { return };
    while let Some(s) = walk.next(w, a) {
        let Some(b) = at_mut(w, a, s) else { continue }; // destroyed by an earlier one's hooks
        if let Some(g) = b.gate {
            b.gate = if g - 1 <= 0 { None } else { Some(g - 1) };
        }
        let c = b.class;
        if let Some(tick) = hooks(w, c).and_then(|h| h.tick)
            && let Some(i) = pos(w, a, s)
            && tick(w, a, idx(i))
        {
            end_actor_buff(w, a, s);
            continue;
        }
        let Some(b) = at_mut(w, a, s) else { continue };
        if let Some(t) = b.stack {
            if t - 1 <= 0 {
                b.stack = None;
                actor_remove_stack(w, a, s, 1, true); // TimedRemoveStack @5570
                if pos(w, a, s).is_none() {
                    continue;
                }
            } else {
                b.stack = Some(t - 1);
            }
        }
        let Some(b) = at_mut(w, a, s) else { continue };
        if let Some(l) = b.life {
            b.life = Some(l - 1);
            if l - 1 <= 0 {
                end_actor_buff(w, a, s);
            }
        }
    }
}

/// `actorBuffSignal`: a delegate of the actor that a buff of this path binds: its
/// Comp_Pawn_HitBoxHandler's HitAnything, its ReflectedDispatch (SpawnedActorParent MulticastReflect @23918) and its
/// Comp_Pawn_DamageManager's HitByEnemy (with `arg`).
// Unverified: the order of the delegate's bindings against Buff_SlowProjectile's (`slow_hit_anything`, called first)
// and against the actor's own class's (OnHitByEnemy, called first) is not read.
pub fn actor_buff_signal(w: &mut World, a: ActorRef, s: ActorBuffSignal, arg: Option<&ActorBuffArg>) {
    let Some(mut walk) = Walk::new(w, a) else { return };
    while let Some(sr) = walk.next(w, a) {
        let Some(i) = pos(w, a, sr) else { continue };
        let c = state(w, a).map(|st| st.buffs[i].class);
        let Some(on) = c.and_then(|c| hooks(w, c)).and_then(|h| h.on) else { continue };
        if on(w, a, idx(i), s, arg) {
            end_actor_buff(w, a, sr);
        }
    }
}

/// `hitActorRows`: pfgDoDamage's rows for a HitActor that is no fighter once
/// DamageAndLaunch returned Hit (Comp_Pawn_HitBoxHandler @18293-19121): every row with ApplyBuffHit? and
/// ApplyBuffEnemy? @18559, the Instigator's TeamIndex other than the HitActor's @18676, goes to the HitActor:
/// SpawnBuffActor(AppliedBuffs, HitActor, Instigator, Round(StackCount x GetChargeBuffStackMultiplier) @18868-18923,
/// BuffDuration, true, false) @19011. `src`: the Instigator; `mult`: the dealing handler's charge stack multiplier.
// Unverified: a HitActor that ApplyDamage destroyed inside DamageAndLaunch gets no rows (the build's SpawnBuffActor on
// a pending-kill actor is not read).
pub fn hit_actor_rows(w: &mut World, src: FighterId, a: ActorRef, rows: Option<&[Props]>, mult: Real) {
    let Some(rows) = rows else { return };
    if w.fighter(src).team.team == team_of(w, a) {
        return;
    }
    for e in rows {
        if !(flag(Some(e), "ApplyBuffHit?") && flag(Some(e), "ApplyBuffEnemy?")) {
            continue;
        }
        if crate::actors::world_actor::actor_dead(w, a) {
            return;
        }
        let cls = e.get("AppliedBuffs").and_then(Value::as_str).and_then(|s| w.names.get(s)).unwrap_or(Name::NONE);
        let n = math::ue::round_to_int(num(Some(e), "StackCount").unwrap_or(1.0) * mult); // Round
        let d = num(Some(e), "BuffDuration").unwrap_or(0.0);
        spawn_actor_buff(w, a, cls, n, d, true, Some(src));
    }
}

/// `setDestroyBlocked`: BPI_SetDestroyEffectsBlocked(Blocked) /
/// BPI_SetDestroySpawnedActorBlocked(Blocked) of BPI_SpawnedActor on the actor (SpawnedActorParent's own: ubergraph
/// @5202 DestroyVFXBlocked, @5222 DestroySpawnedActorBlocked; no class overrides them). Callers in the build:
/// Buff_C015HitProjectile @1214 / @1251, GarnetElectricZone. A projectile and an item keep them (their EndPlay reads
/// them: `world::spawning::destroy_projectile`, `world::items::destroy_item`).
// Not ported: a World.actors actor given the call is refused with a warning. No caller makes it on one: Taz's eat
// leaves a world actor's flags alone, GarnetElectricZone keeps its own (`chars::garnet::zone`), and the world
// actors' ends that play the parent's death effects (HarleyJackInTheBox) read no flag.
pub fn set_destroy_blocked(w: &mut World, a: ActorRef, effects: bool, blocked: bool) {
    match a {
        ActorRef::Projectile(id) => {
            let Some(p) = w.actors.projectiles.get_mut(id) else { return };
            if effects {
                p.destroy_fx_blocked = Some(blocked);
            } else {
                p.destroy_spawn_blocked = Some(blocked);
            }
        }
        ActorRef::Item(id) => {
            let Some(it) = w.actors.items.get_mut(id) else { return };
            if effects {
                it.destroy_fx_blocked = blocked;
            } else {
                it.destroy_spawn_blocked = blocked;
            }
        }
        _ => crate::actors::world_actor::warn("BPI_SetDestroy*Blocked: not ported for a World.actors actor"),
    }
}

/// `setHoming`: ProjectileHomingMulticast's writes on the actor's
/// ProjectileMovement (Buff_C021_SetHomingTowardsTarget @2950-3134): bIsHomingProjectile = true,
/// HomingAccelerationMagnitude = mag, HomingTargetComponent = the root of `to`. The move's homing from then on in
/// place of the class's own; an item's own flag with it (`item_write_homing`).
pub fn set_homing(w: &mut World, a: ActorRef, to: ActorRef, mag: Real) {
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
            }
            crate::actors::item::item_write_homing(w, id, true);
        }
        _ => {}
    }
}

/// `clearHoming`: bIsHomingProjectile = false (a buff's graph, the actor's own
/// class, MulticastReflect @23793-23862): the class's own flag with it.
pub fn clear_homing(w: &mut World, a: ActorRef) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.buff_home = None;
                crate::actors::projectile::write_homing(p, false);
            }
        }
        ActorRef::Item(id) => {
            if let Some(it) = w.actors.items.get_mut(id) {
                it.buff_home = None;
            }
            crate::actors::item::item_write_homing(w, id, false);
        }
        _ => {}
    }
}

/// Normal(V, 0.0001) x speed (UKismetMathLibrary::Normal -> FVector::GetSafeNormal(Tolerance): a squared size of 1
/// returns V, one below the tolerance the zero vector; else V x FMath::InvSqrt), in float32. The UE X (depth) part
/// is 0.
fn normal_times(vx: Real, vy: Real, speed: Real) -> (Real, Real) {
    let n = math::ue::normal([vx, vy, 0.0], math::ue::KINDA_SMALL_NUMBER);
    ((n[0] * speed), (n[1] * speed))
}

/// The ProjectileMovement state the speed buffs write, of a projectile or an item.
struct Pmc {
    vx: Real,
    vy: Real,
    max_speed: Real,
    active: bool,
    physics: bool,
    gravity: Real,
}

fn pmc(w: &World, a: ActorRef) -> Option<Pmc> {
    match a {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id)?;
            Some(Pmc {
                vx: p.vx,
                vy: p.vy,
                max_speed: p.max_speed,
                active: crate::actors::projectile::movement_active(p),
                physics: false,
                gravity: crate::actors::projectile::gravity_scale(p),
            })
        }
        ActorRef::Item(id) => {
            let it = w.actors.items.get(id)?;
            Some(Pmc {
                vx: it.vx,
                vy: it.vy,
                max_speed: it.max_speed,
                active: crate::actors::item::item_movement_active(w, id),
                physics: crate::actors::item::item_simulating_physics(w, id),
                gravity: crate::actors::item::item_gravity_scale(w, id),
            })
        }
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
            if let Some(it) = w.actors.items.get_mut(id) {
                (it.vx, it.vy) = (vx, vy);
            }
        }
        _ => {}
    }
}

fn set_max_speed(w: &mut World, a: ActorRef, m: Real) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.max_speed = m;
            }
        }
        ActorRef::Item(id) => {
            if let Some(it) = w.actors.items.get_mut(id) {
                it.max_speed = m;
            }
        }
        _ => {}
    }
}

fn set_gravity(w: &mut World, a: ActorRef, g: Real) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                crate::actors::projectile::set_gravity_scale(p, g);
            }
        }
        ActorRef::Item(id) => crate::actors::item::item_set_gravity_scale(w, id, g),
        _ => {}
    }
}

/// The class defaults of the buff a hook was called with.
fn hook_def(w: &World, a: ActorRef, i: u32) -> Option<&Props> {
    bdef(w, a, elem_buff(w, a, i)?.class)
}

/// Buff_AlterProjectileSpeed's ReceiveBeginPlay (BuffParentActor child, no EndPlay, no tick): the parent's @839 ->
/// @29: the owner is no SpawnedActorParent -> K2_DestroyActor @10-114. ProjectileMovement.IsActive() @128: Velocity =
/// Normal(Velocity, 0.0001) x NewSpeed @206-338, MaxSpeed = NewSpeed @409. Else the root Capsule simulating physics
/// @485: its physics linear velocity = Normal(it, 0.0001) x NewSpeed @580-753. Else nothing. NewSpeed: the class
/// default 400. Nothing is given back at its end.
fn alter_begin(w: &mut World, a: ActorRef, i: u32, _from: Option<FighterId>) -> bool {
    let sp = num(hook_def(w, a, i), "NewSpeed").unwrap_or(400.0);
    let Some(m) = pmc(w, a) else {
        crate::actors::world_actor::warn("Buff_AlterProjectileSpeed_C: the owner is no spawned actor the port knows");
        return true;
    };
    let is_item = matches!(a, ActorRef::Item(_));
    // a projectile's root never simulates physics
    if !m.active && !(is_item && m.physics) {
        return false;
    }
    let (vx, vy) = normal_times(m.vx, m.vy, sp);
    set_v(w, a, vx, vy);
    if m.active {
        set_max_speed(w, a, sp);
    }
    false
}

/// Buff_Slow30PercentProjectile (BuffParentActor child; not Buff_SlowProjectile's family). SpeedReduction 0.3,
/// StateInfo BlockerGroups [CC Immune], EffectGravity false. ReceiveBeginPlay @2478: the parent's, the owner no
/// SpawnedActorParent -> K2_DestroyActor @1126. An active ProjectileMovement @2587: Velocity x SpeedReduction @2755,
/// MaxSpeed x SpeedReduction @2916, with EffectGravity NewProjectileScale = gravity x SpeedReduction and the gravity
/// set to it @3001-3118; then (@1649), with authority and a Comp_Pawn_HitBoxHandler, the binds: the handler's
/// HitAnything -> OnHitAnything and the owner's ReflectedDispatch -> OnReflected @2306-2393, both K2_DestroyActor. Not
/// active, the root simulating physics @3194: its physics velocity x SpeedReduction @3289-3420 and nothing more.
/// No graph spawns the class: Buff_C021_SlowReflectProjectile's End removes it by class (@628 RemoveBuff).
fn slow_begin(w: &mut World, a: ActorRef, i: u32, _from: Option<FighterId>) -> bool {
    let d = hook_def(w, a, i);
    let sr = num(d, "SpeedReduction").unwrap_or(0.3);
    let eg = flag(d, "EffectGravity");
    let Some(m) = pmc(w, a) else {
        crate::actors::world_actor::warn(
            "Buff_Slow30PercentProjectile_C: the owner is no spawned actor the port knows",
        );
        return true;
    };
    if m.active {
        set_v(w, a, m.vx * sr, m.vy * sr);
        set_max_speed(w, a, m.max_speed * sr);
        let scale = eg.then_some(m.gravity * sr);
        if let Some(b) = elem_buff_mut(w, a, i) {
            b.ext = ElemExt::Slow30 { bound: true, scale };
        }
        if let Some(n) = scale {
            set_gravity(w, a, n);
        }
        return false;
    }
    if matches!(a, ActorRef::Item(_)) && m.physics {
        set_v(w, a, m.vx * sr, m.vy * sr);
    }
    false
}

/// Buff_Slow30PercentProjectile ReceiveTick @1145: with EffectGravity and a gravity other than NewProjectileScale,
/// AdjustProjectileGravityMulticast sets it back @1340-1573 (NewProjectileScale stays 0 when the begin did not set
/// it).
fn slow_tick(w: &mut World, a: ActorRef, i: u32) -> bool {
    if !flag(hook_def(w, a, i), "EffectGravity") {
        return false;
    }
    let Some(m) = pmc(w, a) else { return false };
    let n = scale_of(w, a, i);
    if m.gravity != n {
        set_gravity(w, a, n);
    }
    false
}

fn scale_of(w: &World, a: ActorRef, i: u32) -> Real {
    match elem_buff(w, a, i).map(|b| b.ext) {
        Some(ElemExt::Slow30 { scale: Some(s), .. }) => s,
        _ => 0.0,
    }
}

/// Buff_Slow30PercentProjectile's OnHitAnything and OnReflected: K2_DestroyActor, bound by an active begin.
fn slow_on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, _arg: Option<&ActorBuffArg>) -> bool {
    matches!(s, ActorBuffSignal::HitAnything | ActorBuffSignal::Reflected)
        && matches!(elem_buff(w, a, i).map(|b| b.ext), Some(ElemExt::Slow30 { bound: true, .. }))
}

/// Buff_Slow30PercentProjectile ReceiveEndPlay @10: a valid SpawnedActorParent owner: Velocity x (1 / SpeedReduction)
/// @248-380 and MaxSpeed = GetMaxSpeed() x (1 / SpeedReduction) @451-615 whatever the component does now (the
/// reciprocal a float32 before the products), with EffectGravity and the gravity equal to NewProjectileScale it x (1
/// / SpeedReduction) @686-928.
// Deviation: an item's ProjectileMovement velocity while it lies (physics on) is not kept apart from its body's: the
// end's velocity write is skipped for an item that does not fly.
fn slow_end(w: &mut World, a: ActorRef, i: u32) {
    let d = hook_def(w, a, i);
    let sr = num(d, "SpeedReduction").unwrap_or(0.3);
    let eg = flag(d, "EffectGravity");
    let Some(m) = pmc(w, a) else { return };
    let inv = 1.0 / sr;
    if matches!(a, ActorRef::Projectile(_)) || m.active {
        set_v(w, a, m.vx * inv, m.vy * inv);
    }
    set_max_speed(w, a, m.max_speed * inv);
    if eg && m.gravity == scale_of(w, a, i) {
        set_gravity(w, a, inv * m.gravity);
    }
}

/// On a fighter (a state manager is a state manager): Buff_AlterProjectileSpeed's and Buff_Slow30PercentProjectile's
/// BeginPlay destroy a buff whose owner is no SpawnedActorParent (Alter @10-114, Slow30 @2573 -> @1126), inside the
/// spawn. No ported caller.
fn destroyed_on_fighter(w: &mut World, f: FighterId, b: crate::combat::BuffRef, _: Option<FighterId>, _: Option<Real>) {
    crate::combat::buffs::end_buff(w, f, b);
}

/// The elemental and ice classes on a fighter: no caller among the ported characters (warned).
fn warn_on_fighter(_: &mut World, _: FighterId, _: crate::combat::BuffRef, _: Option<FighterId>, _: Option<Real>) {
    crate::actors::world_actor::warn("an elemental or projectile-slow buff on a fighter: not ported (no caller)");
}

/// `actorSizeUpdate`: ApplySizeBuffs on a spawned actor's own Comp_State_Manager
/// (Comp_State_Manager @2461, from UpdatePlayerStatus @507): the actor's buffs' size mods by CalculateBuffValues'
/// rule (CalculateBuffValues' size loop, the list's order), and a value other than the actor's
/// Comp_SizeManager.CurrentSizeScale (within 1e-4) is handed to its SetSizeScale: with SizeCanScale (true in the
/// component's CDO) CurrentSizeScale = it and SetActorScale3D(it). The scale is absolute: a root spawned at another
/// scale is set to the mod, and back to 1 when the last such buff ends. A projectile, an item, or a world actor whose
/// kind has the size hooks; another kind is warned.
pub fn actor_size_update(w: &mut World, a: ActorRef) {
    let mut m = 1.0;
    if let Some(st) = state(w, a) {
        for (i, b) in st.buffs.iter().enumerate() {
            let d = actor_inst_def(w, a, idx(i));
            m = crate::combat::buffs::size_mod_step(m, d.as_deref().unwrap_or(&NO_PROPS), b.stacks);
        }
    }
    match a {
        ActorRef::Item(id) => {
            let Some(it) = w.actors.items.get(id) else { return };
            if (it.size_scale.unwrap_or(1.0) - m).abs() > 1e-4 {
                crate::actors::item::item_set_size_scale(w, id, m);
            }
        }
        ActorRef::Projectile(id) => {
            let Some(p) = w.actors.projectiles.get_mut(id) else { return };
            if (p.size_scale.unwrap_or(1.0) - m).abs() <= 1e-4 {
                return;
            }
            p.size_scale = Some(m);
            crate::actors::projectile::set_actor_scale(p, m);
        }
        _ => {
            let k = match (a, crate::actors::world_actor::actor_entry(w, a).map(|e| e.hooks)) {
                (ActorRef::Actor(id), Some(crate::registry::ActorHooks::World(h))) => h.size.map(|k| (id, k)),
                _ => None,
            };
            let Some((id, k)) = k else {
                if m != 1.0 {
                    crate::actors::world_actor::warn("size mods on an actor: not ported for this kind of actor");
                }
                return;
            };
            let Some(sa) = w.actors.actors.get(id) else { return };
            let old = sa.size_scale.unwrap_or(1.0);
            // `actorDef(sa.owner, sa.cls)`: through the actor's owner, not `lookupOwner`
            let def = crate::actors::world_actor::actor_owner(w, a)
                .and_then(|o| crate::fighter::common::actor_class_def(w, o, sa.cls));
            let off = obj(def.map(|d| &d.components), "Comp_SizeManager").and_then(|c| c.get("SizeCanScale"))
                == Some(&Value::Bool(false));
            if (old - m).abs() <= 1e-4 || off {
                return;
            }
            if let Some(sa) = w.actors.actors.get_mut(id) {
                sa.size_scale = Some(m);
            }
            if let Some(f) = k.scale {
                f(w, id, m, old);
            }
        }
    }
}

// ---- Buff_Enlarge's projectile pass and the two classes it gives

/// `enlargeSpawned`: Buff_Enlarge ReceiveBeginPlay (@34-317) binds the owner's
/// Comp_Spawner AfterSpawnedActor -> OnSpawnedActor (@1712), broadcast in HandleActorPostSpawn @12116 for each actor
/// its CompSpawnActor made: behind the actor's BeginPlay and ahead of its hit box update. OnSpawnedActor @1712-2361: a
/// ParentProjectilePhysics whose Comp_SizeManager.SizeCanScale and own PolymorphScaleUp? are both true gets, on its
/// own Comp_State_Manager, SpawnBuffActor(IsProjectile of its Comp_Actor_Destructible ?
/// Buff_Polymorph_ProjectileEnlarge_C : Buff_Polymorph_ProjectileEnlarge_WithHieghtChange_C, the actor, the
/// Enlarge's Instigator, 0, 6.0, true, false). @2366: not a ParentProjectilePhysics, a Steven_ShieldActor (or its
/// child) goes to @2032 with no SizeCanScale or PolymorphScaleUp? test.
// Deviation: the importer's GRAPH_BUFFS_SKIP leaves the two classes' defs out of every character's data but Rick's:
// another character's projectile finds no def and the buff is refused.
// Not ported: an item or another world actor is not passed here.
pub fn enlarge_spawned(w: &mut World, f: FighterId, pr: ActorRef) {
    let on = w.fighter(f).buffs.buffs.iter().any(|b| crate::combat::buffs::is_enlarge(b.cls) && b.frames > 0.0);
    if !on {
        return;
    }
    let Some(cls) = crate::actors::world_actor::actor_cls(w, pr) else { return };
    let cs = w.names.str(cls);
    let a = crate::fighter::common::actor_class_def(w, f, cls);
    let in_chain = |sup: &str| cs == sup || supers_has(a, sup);
    if in_chain("ParentProjectilePhysics_C") && matches!(pr, ActorRef::Projectile(_)) {
        let sm = obj(a.map(|d| &d.components), "Comp_SizeManager").and_then(|c| c.get("SizeCanScale"));
        let can = sm.is_none_or(|v| crate::actors::projectile::truthy(Some(v)) || v.is_null());
        if !can || !flag(Some(crate::actors::world_actor::actor_p(w, pr)), "PolymorphScaleUp?") {
            return;
        }
    } else if !in_chain("Steven_ShieldActor_C") {
        return;
    }
    let is_proj = flag(obj(a.map(|d| &d.components), "Comp_Actor_Destructible"), "IsProjectile");
    let c = if is_proj {
        crate::name!("Buff_Polymorph_ProjectileEnlarge_C")
    } else {
        crate::name!("Buff_Polymorph_ProjectileEnlarge_WithHieghtChange_C")
    };
    spawn_actor_buff(w, pr, c, 0, 6.0, true, None);
}

/// Buff_Polymorph_ProjectileEnlarge_WithHieghtChange ReceiveBeginPlay @10, the owner's root a CapsuleComponent
/// @82-147: Offset = Abs(CapsuleHalfHeight x GetSizeMod Z - CapsuleHalfHeight) @239-412 (the unscaled half height),
/// K2_SetActorLocation(location + (0, 0, Offset), no sweep, teleport) @907, ahead of the size write of
/// SpawnBuffActor's UpdatePlayerStatus. ReceiveEndPlay @974 calls the parent's first (the scale back to 1), then
/// K2_SetActorLocation(location - (0, 0, Offset)) @1210.
// Deviation: the class's end runs ahead of the parent's size update (`end_actor_buff`): the location is lowered
// before the scale goes back. Neither write reads the other, so the end state is the same.
fn height_begin(w: &mut World, a: ActorRef, i: u32, _from: Option<FighterId>) -> bool {
    let z = obj(obj(hook_def(w, a, i), "SizeMods"), "MultSizeMod").and_then(|m| num(Some(m), "Z")).unwrap_or(1.0);
    let size = match a {
        ActorRef::Actor(id) => match crate::actors::world_actor::actor_entry(w, a).map(|e| e.hooks) {
            Some(crate::registry::ActorHooks::World(h)) => {
                h.size.filter(|k| k.capsule_half.is_some() && k.move_z.is_some())
            }
            _ => None,
        }
        .map(|k| (id, k)),
        _ => None,
    };
    let h = match (a, size) {
        (ActorRef::Projectile(id), _) => w.actors.projectiles.get(id).map(|p| p.cap_h),
        (_, Some((id, k))) => k.capsule_half.map(|f| f(w, id)),
        _ => None,
    };
    let Some(h) = h else {
        crate::actors::world_actor::warn(
            "Buff_Polymorph_ProjectileEnlarge_WithHieghtChange_C: no capsule root the port knows",
        );
        return false;
    };
    let off = ((h * z) - h).abs();
    if let Some(b) = elem_buff_mut(w, a, i) {
        b.ext = ElemExt::HeightChange { off };
    }
    move_z(w, a, off, size.and_then(|(_, k)| k.move_z));
    false
}

fn move_z(w: &mut World, a: ActorRef, dz: Real, k: Option<fn(&mut World, crate::actors::ActorId, Real)>) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.y += dz;
                p.prev_y = p.y;
            }
        }
        ActorRef::Actor(id) => {
            if let Some(f) = k {
                f(w, id, dz);
            }
        }
        _ => {}
    }
}

fn height_end(w: &mut World, a: ActorRef, i: u32) {
    let Some(ElemExt::HeightChange { off }) = elem_buff(w, a, i).map(|b| b.ext) else { return };
    let k = match crate::actors::world_actor::actor_entry(w, a).map(|e| e.hooks) {
        Some(crate::registry::ActorHooks::World(h)) => h.size.and_then(|k| k.move_z),
        _ => None,
    };
    move_z(w, a, -off, k);
}

/// `projectileLike`: the test both graphs make of a spawned actor: its
/// Comp_Actor_Destructible valid and (its IsProjectile or it implements BPI_Projectiles) (ProjectileApplyBuff
/// @74-337, AfterGrounded HandleProjectileAttack @0-183). An item's IsProjectile is its own.
// Unverified: a Comp_Actor_Destructible (and, for ProjectileApplyBuff's bind, a Comp_Pawn_HitBoxHandler) is taken as
// on every spawned actor of the port.
pub fn projectile_like(w: &mut World, a: ActorRef) -> bool {
    if let ActorRef::Item(id) = a {
        return crate::actors::item::item_is_projectile(w, id)
            || format!(" {} ", class_def(w, a).and_then(|d| d.ifaces.as_deref()).unwrap_or(""))
                .contains(" BPI_Projectiles_C ");
    }
    let Some(cls) = crate::actors::world_actor::actor_cls(w, a) else { return false };
    let own = data_owner(w, a);
    crate::fighter::common::actor_is_projectile(w, ActorRef::Fighter(own), cls)
}

/// OnProjectileHitsEnemy's cast of the Actor to SpawnedActorParent (@1149): a projectile or an item, or an actor of
/// the other lists with an attack id whose class data has SpawnedActorParent among its supers.
fn spawned_actor_parent(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(_) | ActorRef::Item(_) => true,
        _ => {
            crate::actors::world_actor::actor_attack_id(w, a).is_some()
                && supers_has(class_def(w, a), "SpawnedActorParent_C")
        }
    }
}

// ---- Buff_ProjectileApplyBuff
//
// CDO: GiveBuffToSpawnedActors, GiveBuffToReflectedActors, DestroyBuffWhenOwnerReflected, CanStack, MaxStacks 2; its
// children (Buff_ProjectileToMeleeFire, Buff_IceProjectile) set Buff and the Unstacked / Stacked counts and durations.
// ReceiveBeginPlay @10 (the parent's first), authority: the owner's Comp_Actor_Destructible valid @125, (its
// IsProjectile or the owner implements BPI_Projectiles) @277, its Comp_Pawn_HitBoxHandler valid @422: bind the
// handler's HitEnemyPlayer -> OnProjectileHitsEnemy @465 / @559. DealtDamageModCondition: the parent's (true) @0,
// Active = BaseKnockBack > 0 && HitStunMult > 0 @64-188 (the hit as the pass has it: `actor_dealt_damage_mods`
// writes it), the parent's answer @207. Its mods are DoNothing. A child class enters these as its actor hooks.

/// GetHitStunDecay(Actor, Actor.HitBoxDamage, ComboResistanceEnabled) @1373 == 1.0 exactly @1507, after the hit's
/// RegisterHitFromFighter; the victim's lastDecayStacks (the hit's own, for the HUD) is put back.
// Unverified: Actor.HitBoxDamage of another list's actor is taken as its `hit`.
fn decay_one(w: &mut World, a: ActorRef, vic: FighterId) -> bool {
    let Some(cls) = crate::actors::world_actor::actor_cls(w, a) else { return false };
    let attack_id = crate::actors::world_actor::actor_attack_id(w, a).unwrap_or(0);
    let p = crate::actors::world_actor::actor_hit(w, a);
    // An AttackDecayType given by its enum name is no number of [1, 2, 3, 6].
    let dt = match p.and_then(|p| p.attack_decay_type) {
        Some(crate::combat::DataEnum::Value(v)) => v,
        Some(crate::combat::DataEnum::Name(_)) | None => 0,
    };
    let decay_name = if [1, 2, 3, 6].contains(&dt) {
        p.and_then(|p| p.attack_decay_name).unwrap_or(crate::name!("None"))
    } else {
        cls
    };
    let h = crate::combat::decay::DecayHit {
        attack_id,
        decay_type: dt,
        decay_name,
        combo_name: cls,
        base_knock_back: p.and_then(|p| p.base_knock_back).unwrap_or(0.0),
        launch_knock_back: None,
        additive_kb_mode: None,
        knock_back_scaling: None,
        hit_stun_mult: None,
    };
    let keep = w.fighter(vic).attackers.last_decay_stacks;
    let att = crate::actors::world_actor::actor_owner(w, a);
    let pen = w.flow.same_attack_penalty;
    let m = crate::combat::hit::hit_stun_decay(w, vic, att, &h, pen);
    w.fighter_mut(vic).attackers.last_decay_stacks = keep;
    m == 1.0
}

/// `projectileApplyBuffHit`: OnProjectileHitsEnemy @1115-1593 for the buff (by
/// its serial) on the actor, the victim hit: the decay and Active gates, then -> @605: the State Manager of
/// GetOwner().GetOwner() (the actor's fighter) valid @718; SpawnBuffActor(Buff, the HitPlayer, that fighter as
/// Instigator, CurrentStackAmount >= 2 ? Stacked : Unstacked count and duration) @1018.
pub fn projectile_apply_buff_hit(w: &mut World, a: ActorRef, s: u32, victim: FighterId) {
    let Some(b) = at(w, a, s) else { return };
    let (c, stacks) = (b.class, b.stacks);
    let ElemExt::ApplyBuff { bound: true, .. } = b.ext else { return };
    if !decay_one(w, a, victim) {
        return;
    }
    if !matches!(at(w, a, s).map(|b| b.ext), Some(ElemExt::ApplyBuff { active: true, .. })) {
        return;
    }
    let d = bdef(w, a, c);
    let two = stacks >= 2;
    let (dur, n) = if two {
        (num(d, "StackedBuffDuration"), num(d, "StackedBuffStacksToApply"))
    } else {
        (num(d, "UnstackedBuffDuration"), num(d, "UnstackedBuffStacksToApply"))
    };
    let buff = d.and_then(|d| d.get("Buff")).and_then(Value::as_str).and_then(|s| w.names.get(s)).unwrap_or(Name::NONE);
    let owner = crate::actors::world_actor::actor_owner(w, a);
    crate::combat::buffs::spawn_buff(
        w,
        victim,
        buff,
        dur.unwrap_or(0.0),
        Some(ticks(n.unwrap_or(0.0))),
        owner,
        true,
        None,
    );
}

/// Buff_ProjectileApplyBuff's begin (`projectileApplyBuffActor.begin`): the bind's tests.
pub fn apply_buff_begin(w: &mut World, a: ActorRef, i: u32, _from: Option<FighterId>) -> bool {
    let bound = projectile_like(w, a);
    if let Some(b) = elem_buff_mut(w, a, i) {
        b.ext = ElemExt::ApplyBuff { bound, active: false };
    }
    false
}

/// Buff_ProjectileApplyBuff's DealtDamageModCondition (@64-188): Active = BaseKnockBack > 0 AND HitStunMult > 0,
/// from the hit as the pass has it; true.
pub fn apply_buff_dealt_cond(
    w: &mut World,
    a: ActorRef,
    i: u32,
    _: ActorRef,
    hit: &crate::combat::HitPacket,
    _: bool,
) -> bool {
    let on = hit.base_knock_back.unwrap_or(0.0) > 0.0 && hit.hit_stun_mult.unwrap_or(0.0) > 0.0;
    if let Some(b) = elem_buff_mut(w, a, i)
        && let ElemExt::ApplyBuff { active, .. } = &mut b.ext
    {
        *active = on;
    }
    true
}

/// Buff_ProjectileApplyBuff's 'hitEnemyPlayerFirst' (bound in the parent's BeginPlay, ahead of the actor class's own
/// handler): the signal's actor is the victim.
pub fn apply_buff_on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, a_arg: Option<&ActorBuffArg>) -> bool {
    let arg = a_arg.map(|x| ActorRef::Fighter(x.by));
    let (ActorBuffSignal::HitEnemyPlayerFirst, Some(ActorRef::Fighter(by))) = (s, arg) else { return false };
    if !spawned_actor_parent(w, a) {
        return false;
    }
    let Some(serial) = elem_buff(w, a, i).map(|b| b.serial) else { return false };
    projectile_apply_buff_hit(w, a, serial, by);
    false
}

/// The actor hooks of a Buff_ProjectileApplyBuff child (`projectileApplyBuffActor`).
pub const PROJECTILE_APPLY_BUFF_ACTOR: ActorBuffHooks = ActorBuffHooks {
    begin: Some(apply_buff_begin),
    tick: None,
    overlap_timer: None,
    on: Some(apply_buff_on),
    end: None,
    stacks: None,
    max_stacks: None,
    hurt_sphere: None,
    hitbox_sphere: None,
    values: None,
    taken_cond: None,
    dealt_cond: Some(apply_buff_dealt_cond),
};

// ---- the call surface of the ice buffs
//
// What Superman's ice pellet leaves on the other team's spawned actors: Buff_SlowProjectile and its child
// Buff_IceSlowProjectile (a buff actor on a projectile; no graph of its own: class defaults only), and IceBlockItem.
// Read: Buff_SlowProjectile, Superman_IceBreath @1227-2475 for the caller. The buff sits in the elemental list and
// takes `elem_spawn`'s rules: StateInfo.BlockerBuffs (Buff_IceSlowProjectile: Buff_Elemental_Iceproof) and
// BlockerGroups refuse it, and a second one on a slowed projectile is refused.

/// `slowProjectile`: SpawnBuffActor(cls, the projectile, ...) and the buff's ReceiveBeginPlay
/// (ubergraph @770): ApplySpeedBuff(ProjectileMovement, owner): an active movement component gets Velocity x
/// SpeedReduction @147 and MaxSpeed x SpeedReduction @287, and ActiveSpeedModifier = SpeedReduction @359 (0.15, the
/// ice child 0.05). The buff binds the owner's handler's HitAnything and ReflectedDispatch @1862-1949
/// (`slow_hit_anything`). The pellet passes a duration of 0.0 (Superman_IceBreath @1870). False: no buff actor.
// Not ported: a movement component that is not active but whose capsule simulates physics (@868), Comp_RunAlongEdgeTrace's
// "Move Speed Multiplier" @554, EffectGravity (false in both classes: warned).
// An item takes the same two writes on its Velocity and MaxSpeed while its movement is active (in flight, not in a
// hand): Buff_SlowProjectile's BeginPlay casts the owner to SpawnedActorParent, which ItemActorParent is a child of
// (@800-879). A World.actors actor or a Marker is no ProjectileMovement owner here: false.
pub fn slow_projectile(w: &mut World, a: ActorRef, c: BuffClass) -> bool {
    if !matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_)) {
        return false;
    }
    let Some(d) = bdef(w, a, c) else { return false };
    if flag(Some(d), "EffectGravity") {
        crate::actors::world_actor::warn(
            "a projectile slow's EffectGravity is not ported (the gravity scale is left alone)",
        );
    }
    let m = num(Some(d), "SpeedReduction").unwrap_or(1.0);
    let Some(s) = elem_spawn(w, a, c, 0) else { return false };
    if scale(w, a, m, false)
        && let Some(b) = at_mut(w, a, s)
    {
        b.speed_mod = Some(m);
    }
    true
}

/// `scale_speed` on the projectile (its velocity and MaxSpeed are float32 already) or the item's movement; false when
/// the actor is gone or (unless `always`) its movement is not active.
fn scale(w: &mut World, a: ActorRef, m: Real, always: bool) -> bool {
    match a {
        ActorRef::Projectile(pr) => {
            w.actors.projectiles.get_mut(pr).is_some_and(|p| crate::actors::projectile::scale_speed(p, m, always))
        }
        ActorRef::Item(it) => w.actors.items.get_mut(it).is_some_and(|i| {
            if !always && !i.movement_active() {
                return false;
            }
            i.vx *= m;
            i.vy *= m;
            i.max_speed *= m;
            true
        }),
        _ => false,
    }
}

/// `slowHitAnything`: the buff's two ends that a live owner sees: OnHitAnything @2543 (the
/// owner's handler broadcast HitAnything: pfgDoDamage @22208) and OnReflected @2562, both K2_DestroyActor.
/// ReceiveEndPlay @10 with a valid owner: RemoveSpeedBuff, Velocity x (1 / ActiveSpeedModifier) @110 and MaxSpeed =
/// GetMaxSpeed() x (1 / ActiveSpeedModifier) @301 (the reciprocal a float32 before the products).
// Unverified: a buff spawned on a deactivated movement component keeps ActiveSpeedModifier 0, and RemoveSpeedBuff
// then multiplies by Divide_FloatFloat(1.0, 0.0) (the stock node returns 0, not read in the exe). Nothing is written
// here for it.
pub fn slow_hit_anything(w: &mut World, a: ActorRef) {
    let Some(st) = state(w, a) else { return };
    let list: Vec<(u32, BuffClass, Option<Real>)> = st.buffs.iter().map(|b| (b.serial, b.class, b.speed_mod)).collect();
    for (s, c, m) in list {
        if !SLOWS.contains(&c) {
            continue;
        }
        elem_destroy(w, a, s);
        if let Some(m) = m {
            scale(w, a, 1.0 / m, true);
        }
    }
}

/// `slowOf`: the slow class on the actor, for the drawing side and the tests.
pub fn slow_of(w: &World, a: ActorRef) -> Option<BuffClass> {
    state(w, a)?.buffs.iter().map(|b| b.class).find(|c| SLOWS.contains(c))
}

/// IceBlockItem (ItemActorParent): the item Superman's ice pellet leaves where it destroyed an enemy's spawned actor
/// (Superman_IceBreath @2349). Its own graph: ReceiveBeginPlay @430 binds its Comp_State_Manager's BuffEnding to
/// ABuffIsEnding; ABuffIsEnding @445: the ending buff's class is Buff_Elemental_Ice -> K2_DestroyActor @497. The
/// elemental is its passive buff (Comp_Pawn_PassiveBuffManager: Buff_Elemental_Ice, BuffSelf, 0 stacks), so the
/// thrown block's hit on an enemy (the elemental's ElementalHitEnemy destroys it) ends the block in the same call, and
/// anything else that ends the elemental does too: read once a tick here.
// Deviation: with Buff_Elemental_Ice missing from the data the block has no elemental and lives like a plain item.
fn ice_gone(w: &World, it: crate::actors::ItemId) -> bool {
    let a = ActorRef::Item(it);
    state(w, a).is_some() && elem_of(w, a, ELEM_ICE).is_none()
}

fn ice_block_begin(w: &mut World, it: crate::actors::ItemId) -> bool {
    let a = ActorRef::Item(it);
    if elem_of(w, a, ELEM_ICE).is_some() {
        return false; // its passive row (the World's spawn)
    }
    if let Some(slot) = crate::actors::world_actor::actor_elem_mut(w, a) {
        *slot = None;
    }
    crate::actors::world_actor::warn("IceBlockItem_C: no passive Buff_Elemental_Ice_C (not in the class data)");
    false
}

fn ice_block_step(w: &mut World, it: crate::actors::ItemId) -> bool {
    ice_gone(w, it)
}

fn ice_block_hit_enemy(w: &mut World, it: crate::actors::ItemId, vic: FighterId) -> bool {
    let a = ActorRef::Item(it);
    if state(w, a).is_none() {
        return false;
    }
    elem_hit_enemy(w, a, vic);
    ice_gone(w, it)
}

// ---- the registry entries of the three modules

fn buff_entry(
    class: BuffClass,
    hooks: crate::registry::entries::BuffHooks,
    actor: Option<ActorBuffHooks>,
) -> crate::registry::BuffEntry {
    crate::registry::BuffEntry {
        class,
        end_skips_parent: false,
        block_fast_fall: false,
        thru_atk: None,
        active_attack: false,
        damage_hit_by_ally: false,
        hooks,
        actor,
    }
}

/// The `BUFFS`, `BUFF_GRAPHS_PORTED` and `ACTORS` entries of the elemental, ice and
/// projectile buff modules.
pub fn register(r: &mut crate::registry::Registry) -> Result<(), crate::registry::RegistryError> {
    use crate::registry::entries::BuffHooks;
    let warn = BuffHooks { begin: Some(warn_on_fighter), ..BuffHooks::default() };
    let destroyed = BuffHooks { begin: Some(destroyed_on_fighter), ..BuffHooks::default() };
    let none = BuffHooks::default();
    // Buff_ElementalParent itself (a class literal the general perk table reaches): spawned by no caller of the build.
    r.buff_graphs_ported.enter("Buff_ElementalParent_C", ())?;
    for (c, h) in [(FIREPROOF, none), (ICEPROOF, none), (ELEM_FIRE, warn), (ELEM_ICE, warn)] {
        r.buffs.enter(c.registered_name().unwrap_or_default(), buff_entry(c, h, None))?;
    }
    for c in SLOWS {
        r.buffs.enter(c.registered_name().unwrap_or_default(), buff_entry(c, warn, None))?;
    }
    let alter = ActorBuffHooks { begin: Some(alter_begin), ..ActorBuffHooks::default() };
    let slow30 = ActorBuffHooks {
        begin: Some(slow_begin),
        tick: Some(slow_tick),
        on: Some(slow_on),
        end: Some(slow_end),
        ..ActorBuffHooks::default()
    };
    r.buffs.enter(
        BuffClass::BuffAlterProjectileSpeed.registered_name().unwrap_or_default(),
        buff_entry(BuffClass::BuffAlterProjectileSpeed, destroyed, Some(alter)),
    )?;
    r.buffs.enter(
        BuffClass::BuffSlow30PercentProjectile.registered_name().unwrap_or_default(),
        buff_entry(BuffClass::BuffSlow30PercentProjectile, destroyed, Some(slow30)),
    )?;
    // (the second: a child with the parent's graph, as isEnlarge reads it)
    r.buff_graphs_ported.enter("Buff_Enlarge_C", ())?;
    r.buff_graphs_ported.enter("Buff_Arcade_Enlarge_C", ())?;
    let height = ActorBuffHooks { begin: Some(height_begin), end: Some(height_end), ..ActorBuffHooks::default() };
    r.buffs.enter(
        BuffClass::BuffPolymorphProjectileEnlarge.registered_name().unwrap_or_default(),
        buff_entry(BuffClass::BuffPolymorphProjectileEnlarge, none, Some(ActorBuffHooks::default())),
    )?;
    r.buffs.enter(
        BuffClass::BuffPolymorphProjectileEnlargeWithHieghtChange.registered_name().unwrap_or_default(),
        buff_entry(BuffClass::BuffPolymorphProjectileEnlargeWithHieghtChange, none, Some(height)),
    )?;
    // CanUseItem false: ActivateObject is ItemActorParent's, which drops it. Ported for a spawn with no owner (the
    // FFA area's): its passive row is given through a fighter of the match.
    r.actors.enter(
        crate::actors::ItemClass::IceBlockItem.name(),
        crate::registry::ActorEntry {
            class: crate::actors::ActorClass::Item(crate::actors::ItemClass::IceBlockItem),
            parents: &[],
            foreign_owner: false,
            ownerless: true,
            keep_at_lab_reset: false,
            spawned_meet_bodies: false,
            calls_parent_hit_anything: false,
            elemental: None,
            hooks: crate::registry::ActorHooks::Item(crate::registry::entries::ItemHooks {
                begin: Some(ice_block_begin),
                step: Some(ice_block_step),
                hit_enemy: Some(ice_block_hit_enemy),
                ..crate::registry::entries::ItemHooks::default()
            }),
            carry: None,
            mesh: None,
            hazard: None,
            follower: None,
        },
    )?;
    Ok(())
}

/// `actorDealtDamageMods` over the typed packet: the spawned dealer's buffs'
/// rewrite of the hit, a copy. A dealer with no buffs keeps the packet.
pub fn actor_dealt_damage_mods_hit(
    w: &mut World,
    a: ActorRef,
    victim: ActorRef,
    p: &crate::combat::HitPacket,
    hit_box: bool,
) -> crate::combat::HitPacket {
    // (a dealer with no elem, a world actor's included, keeps the packet: `h.elem !== undefined ? ... : hit`)
    let empty = state(w, a).is_none_or(|e| e.buffs.is_empty());
    if empty {
        return p.clone();
    }
    actor_dealt_damage_mods(w, a, victim, p, hit_box).into_owned()
}

/// `hitActorRows` over the typed rows of a packet: [`hit_actor_rows`]'s rule.
pub fn hit_actor_rows_typed(
    w: &mut World,
    src: FighterId,
    a: ActorRef,
    rows: &[crate::combat::OnHitBuffRow],
    mult: Real,
) {
    if w.fighter(src).team.team == team_of(w, a) {
        return;
    }
    for e in rows {
        if !(e.apply_buff_hit && e.apply_buff_enemy) {
            continue;
        }
        if crate::actors::world_actor::actor_dead(w, a) {
            return;
        }
        let n = math::ue::round_to_int(crate::real(e.stack_count.unwrap_or(1)) * mult); // Round
        spawn_actor_buff(w, a, e.applied_buffs, n, e.buff_duration.unwrap_or(0.0), true, Some(src));
    }
}

/// The overlaps a buff's hitbox sphere holds (the buff's `ext` keys
/// `o<fighter>.h<hit>` / `.p<component>`, meleeHitboxSphereTest): the set of the actor's
/// buff `i`, `None` for a buff with no such sphere (Buff_GarnetStopProjectile's ext is the one class's that has it).
pub fn hitbox_sphere_keys(w: &mut World, a: ActorRef, i: u32) -> Option<&mut crate::combat::phases::SphereKeys> {
    match &mut elem_buff_mut(w, a, i)?.ext {
        ElemExt::GarnetStopProjectile(e) => Some(&mut e.keys),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The oracle's values (normalTimes and remainingOrDuration run in node on the same inputs),
    // moved by the build's InvSqrt: 3, 4 gave (240.00002, 320), -700.25, 120.5 gave (-394.206, 67.835526).
    #[test]
    fn normal_times_scales_the_safe_normal_in_float32() {
        assert_eq!(normal_times(3.0, 4.0, 400.0), (239.999_98, 319.999_97));
        assert_eq!(normal_times(-700.25, 120.5, 400.0), (-394.205_96, 67.835_52));
        assert_eq!(normal_times(0.001, 0.002, 400.0), (0.0, 0.0));
        assert_eq!(normal_times(0.6, 0.8, 250.0), (150.0, 200.0));
    }

    #[test]
    fn remaining_counts_the_frames_left_past_this_one() {
        assert_eq!(remaining_or_duration(Real::INFINITY), 0.0);
        assert_eq!(remaining_or_duration(0.0), 0.0);
        assert_eq!(remaining_or_duration(1.0), 0.0);
        assert_eq!(remaining_or_duration(61.0), 1.000_008);
        assert_eq!(remaining_or_duration(360.0), 5.983_381_3);
    }
}
