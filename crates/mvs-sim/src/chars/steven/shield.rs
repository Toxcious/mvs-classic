//! Comp_StevenShield: the shield charges and the ally's bubble shield
//! (Buff_StevenSharedShield), AnimNotify_StevenArmor, the StevenCollisionBlock actor ([`StevenBlock`], a World.actors
//! class with no place of its own), and its joined hooks (reset, armorBreak, decrementShieldCharge, abilityAmmoUpdated,
//! otherReset).
//!
//! Comp_StevenShield (offsets are its ubergraph's unless a function is named). Its constants are the component's
//! class defaults (Default__Comp_StevenShield_C), which the pawn's component template repeats
//! (`components.pawn.Comp_StevenShield_GEN_VARIABLE`: read from there): MaxShieldCharges 3, ArmorType 'StevenShield',
//! ShieldArmor (CanOverride, ArmorAmount 1000, MaxHits 4, KnockBackThreshold 8000, DamageResistanceMultiplier 1, the
//! other two 0), ShieldAbilityIndex 4, ShieldRegenTime 8.5, ShieldBreakHit. So a class default the data lacks does not
//! bite here.
//! ReceiveBeginPlay @600: binds the owner's Comp_Armor ArmorBreak -> DecrementFromArmorBreak (@935: the pack's type is
//! ArmorType -> DecrementShieldFromHit; `CharHooks::armor_break`) and Comp_Pawn_AbilityManager AbilityAmmoUpdated ->
//! OnShieldChargeChange (@1026; `CharHooks::ability_ammo_updated`), then Delay(0.2) -> ResetAlly @15.
//! The Delay's ResetAlly is not run: Ally is read in SpawnAndActivateAllyShield alone, which runs ResetAlly itself
//! first (@77), and in ShieldRemoved's unbind, which only meets an Ally a SpawnAndActivateAllyShield set.
//! The pawn's BPI_DecrementShieldCharge (Pawn_StevenUniverse @6575, from Comp_Armor CalculateArmor @4529 on every pack
//! that holds a hit: `CharHooks::decrement_shield_charge`) is DecrementShieldFromHit.
//! The component's OnFighterReset (@998: CurrentShieldCharges = MaxShieldCharges) is called by the pawn's BPI_Cleanup
//! (Pawn_StevenUniverse @8654-8665): [`reset`].
//! The VFX (SpawnVFX / UpdateVFX / RemoveVFX: PS_StevenPassive_Bubble at the mesh, scale GetVFXScale by the charges
//! 0 / 0.55 / 0.7 / 0.85 times the mesh's VFX scale; the buff's the same, divided by the ally's Comp_SizeManager
//! GetSizeScale and placed by its BPI_ElementalSystem root capsule) are the view's.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated comparisons, kept for their NaN result")]

use super::state::{steven_of, steven_w};
use crate::Real;
use crate::State;
use crate::actors::WorldActorClass;
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorId, ActorRef};
use crate::combat::HitPacket;
use crate::combat::buffs::{BuffExt, BuffRef, buff_mut, end_buff, spawn_buff};
use crate::combat::defense::{ArmorPack, add_armor_pack, remove_armor};
use crate::combat::hits::refused_ally_hit;
use crate::events::{Event, NotifyRef, UiAbilityOp};
use crate::fighter::common::{actor_components, data_of, lit, peers, warn_text};
use crate::fighter::control::{emit, set_ammo_cooldown_on_authority, use_ability};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::{TraceBody, body_info};
use crate::fighter::{FighterId, actor_y, hold_def};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{
    ActorEntry, ActorHooks, BuffEntry, BuffHooks, NotifyHandler, SizeHooks, WorldActorHooks,
};
use crate::registry::props::{num, obj, text};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::spawned_platform::{PlatformPart, SpawnedPlatform, part_template, plat_mut, plat_of, platform_end};
use mvs_data::value::{Props, Value};

const SHARED: Name = name!("Buff_StevenSharedShield_C");
const SHIELD_MONTAGES: [Name; 2] = [name!("Steven_SpecialAir_Shield_Montage"), name!("Steven_Special_Shield_Montage")];
const COMPONENT: &str = "Comp_StevenShield_GEN_VARIABLE";

/// `StevenBlock` (StevenCollisionBlock_C): its Box, a platform box of the fighters' movement, is the actor's `plat`
/// (its one part, the parent's `collision` slot in the port's list); the actor has no place of its own.
#[derive(State, Debug, Default)]
pub struct StevenBlock {}

/// Buff_StevenSharedShield_C's extension (`{ steven, charges }`: the Steven's fighter id, -1 none, and
/// CurrentShieldCharges).
#[derive(State, Debug, Default)]
pub struct SharedShield {
    pub steven: i32,
    pub charges: Real,
}

/// The component's defaults (the pawn's template of it; see above).
fn comp(w: &World, f: FighterId) -> Option<&Props> {
    data_of(w, f).fighter.components.pawn.get(COMPONENT).and_then(|c| c.p.as_obj())
}

/// `armorType`: the component's ArmorType, 'StevenShield' by default.
fn armor_type(w: &World, f: FighterId) -> Name {
    match comp(w, f).and_then(|c| text(c, "ArmorType")) {
        Some(s) => lit(w, s),
        None => name!("StevenShield"),
    }
}

/// `abilityIndex`: ShieldAbilityIndex, 4 by default.
fn ability_index(w: &World, f: FighterId) -> i32 {
    comp(w, f).and_then(|c| num(c, "ShieldAbilityIndex")).map_or(4, to_int32)
}

/// `pack`: the armor pack of a data row `a` with the type and MaxHits given.
fn pack(a: Option<&Props>, kind: Name, max_hits: Real) -> ArmorPack {
    let n = |k: &str| a.and_then(|a| num(a, k)).unwrap_or(0.0);
    ArmorPack {
        kind,
        can_override: a.is_some_and(|a| crate::registry::props::flag(a, "CanOverride")),
        amount: n("ArmorAmount"),
        max_hits,
        threshold: n("KnockBackThreshold"),
        kb_res: n("KnockBackResistanceMultiplier"),
        dmg_res: n("DamageResistanceMultiplier"),
        hs_res: n("HitStunResistanceMultiplier"),
    }
}

/// `bit`: the bit of a fighter id, 0 beyond 30.
fn bit(id: i32) -> i32 {
    if (0..31).contains(&id) { 1i32.wrapping_shl(id.cast_unsigned()) } else { 0 }
}

/// `byId`: the peer with id `id`.
fn by_id(w: &World, id: i32) -> Option<FighterId> {
    if id < 0 {
        return None;
    }
    peers(w).iter().copied().find(|&o| w.fighter(o).id == id)
}

/// `allyBuffs`: AllySharedShieldBuff, the component's array of the buffs SpawnAndActivateAllyShield spawned: appended
/// at each spawn (@394) and never cleared (RemoveAllyShield destroys its buffs and keeps the entries). Model: the live
/// Buff_StevenSharedShield actors this Steven spawned, on the fighters of the match in their order.
/// Unverified: an entry whose buff was destroyed is taken to answer no call (its cast fails once the GC has cleared
/// it). Up to the GC it still holds the actor, pending kill, and whether a Blueprint call on it runs is native
/// (AActor::ProcessEvent); a call that ran would add a pack of CanOverride (a second StevenShield pack) on the ally
/// at the next ActivateShieldBuffMulticast loop.
fn ally_buffs(w: &World, f: FighterId) -> Vec<BuffRef> {
    let id = w.fighter(f).id;
    let mut out = Vec::new();
    for &o in peers(w) {
        for b in &w.fighter(o).buffs.buffs {
            if b.cls == SHARED && b.frames > 0.0 && matches!(&b.ext, BuffExt::StevenSharedShield(e) if e.steven == id) {
                out.push(BuffRef { fighter: o, serial: b.serial });
            }
        }
    }
    out
}

/// `resetAlly`: over GetAllActorsOfClass(BP_FighterCharacter), each one that is not a NotPlayerCharacter, that the
/// PlayerDataManager finds for both it and the owner (GetByFighter), of the owner's team (GetTeamIndex) and not the
/// owner -> Ally; the last one found wins and Ally keeps its value when none is.
/// Unverified: GetByFighter is taken to fail for a minion (no PlayerData: PandaGameMode's minions are not players).
// Unverified: with two or more allies (the port's own big matches) only the last one found gets the shared shield,
// as the build's single Ally holds; the others never do. Kept as the build's; giving it to every ally starts here.
fn reset_ally(w: &mut World, f: FighterId) {
    if w.fighter(f).life.is_minion {
        return;
    }
    let team = w.fighter(f).team.team;
    let mut pick = None;
    for &o in peers(w) {
        let l = &w.fighter(o).life;
        if !l.not_player_character && !l.is_minion && w.fighter(o).team.team == team && o != f {
            pick = Some(w.fighter(o).id);
        }
    }
    if let Some(id) = pick {
        steven_w(w, f).shield.ally = id;
    }
}

/// `activateShield`: Shielding = true; with the owner's current montage (GetCurrentMontage) one of the two shield
/// montages: with charges, and the owner not a NotPlayerCharacter, ShieldArmor with ArmorType and MaxHits = charges + 1
/// to the owner's Comp_Armor (@581-766); with none, ShowAbilityUINotReady(the owner's player index,
/// ShieldAbilityIndex) (@816-1041: the HUD's error op). Another montage: nothing past the flag.
/// Unverified: GetByFighter for a minion is taken to fail, so no HUD op for one.
fn activate_shield(w: &mut World, f: FighterId) {
    steven_w(w, f).shield.shielding = true;
    let Some(m) = crate::fighter::active_montage_name(w, f) else { return };
    if !SHIELD_MONTAGES.contains(&m) {
        return;
    }
    let charges = steven_w(w, f).shield.charges;
    if charges > 0.0 {
        if !w.fighter(f).life.not_player_character {
            let p = pack(comp(w, f).and_then(|c| obj(c, "ShieldArmor")), armor_type(w, f), charges + 1.0);
            add_armor_pack(w, f, &p);
        }
    } else if !w.fighter(f).life.is_minion {
        let n = ability_index(w, f);
        emit(w, f, |fighter| Event::UiAbility { fighter, n, op: UiAbilityOp::Error });
    }
}

/// `shieldActivated` @479: ShieldRemoved, ActivateShield, SpawnVFX (the view), SpawnAndActivateAllyShield, then
/// OnAllyRespawn bound to Ally's OnFighterReset @535-558 (None: no binding).
pub fn shield_activated(w: &mut World, f: FighterId) {
    if steven_of(w, f).is_none() {
        return;
    }
    shield_removed(w, f);
    activate_shield(w, f);
    spawn_and_activate_ally_shield(w, f);
    let s = &mut steven_w(w, f).shield;
    s.ally_bound = crate::real(to_int32(s.ally_bound) | bit(s.ally));
}

/// `shieldRemoved` @1111: RemoveShield (Shielding = false, the owner's Comp_Armor RemoveArmor(ArmorType)), RemoveVFX
/// (the view), OnAllyRespawn unbound from Ally's OnFighterReset @1139-1162, RemoveAllyShield.
pub fn shield_removed(w: &mut World, f: FighterId) {
    if steven_of(w, f).is_none() {
        return;
    }
    steven_w(w, f).shield.shielding = false;
    let t = armor_type(w, f);
    remove_armor(w, f, t);
    let s = &mut steven_w(w, f).shield;
    s.ally_bound = crate::real(to_int32(s.ally_bound) & !bit(s.ally));
    remove_ally_shield(w, f);
}

/// `spawnAndActivateAllyShield` (authority): ResetAlly; a valid Ally with a Comp_State_Manager gets
/// SpawnBuffActor(Buff_StevenSharedShield_C, Ally, the owner, 1, 0.0, BreakIfBlocked true) @291 (no duration: no life
/// timer), appended to AllySharedShieldBuff @394; then ActivateShieldBuffMulticast(CurrentShieldCharges) on every
/// entry @462-896. A spawn the ally's live one blocks (the class has no CanOverride: a second Steven's on the same
/// ally) returns None, which the loop's cast drops.
fn spawn_and_activate_ally_shield(w: &mut World, f: FighterId) {
    reset_ally(w, f);
    let ally_id = steven_w(w, f).shield.ally;
    let Some(ally) = by_id(w, ally_id) else { return };
    spawn_buff(w, ally, SHARED, 0.0, Some(1), Some(f), true, None);
    let charges = steven_w(w, f).shield.charges;
    for b in ally_buffs(w, f) {
        activate_shield_buff(w, b.fighter, b, charges);
    }
}

/// `removeAllyShield` (authority): RemoveShieldsMulticast on every entry; the array is kept.
fn remove_ally_shield(w: &mut World, f: FighterId) {
    for b in ally_buffs(w, f) {
        remove_shields(w, b.fighter, b);
    }
}

/// `decrementShieldFromHit` (authority, CurrentShieldCharges > 0): with charges - 1 <= 0, ShieldRemovedMulticast @214
/// (ShieldRemoved) and the owner's Comp_Pawn_HitBoxManager.pfgDoDamage(null, the owner, ShieldBreakHit) @313; then the
/// owner's Comp_Pawn_AbilityManager UseAbilityServer(ShieldAbilityIndex) @529 (TriggerAbilityTimers: one ammo off,
/// whose AbilityAmmoUpdated sets the charges, OnShieldChargeChange) and SetAmmoCooldownOnAuthority(index,
/// ShieldRegenTime, ShieldRegenTime) @645.
/// The ShieldBreakHit (Damage 0, BaseKnockBack 500, (0.5, 0.5), IgnoreFacing, ForceKnockback) is the owner's hit on
/// himself: CanBeHitBy refuses his own team (DamageAndLaunch @718, HitByAlly @8047), and ForceKnockback with the owner
/// as the DamageDealer (@8093 -> @8322 -> @3222) runs the knockback path with Hit false, so he is launched; pfgDoDamage's
/// NotifyAllyOfHit needs Victim != Instigator. The call sits inside the breaking hit's CalculateArmor (Comp_Armor
/// ArmorBreak @1501), so that hit's own launch comes after this one.
// The ShieldBreakHit self hit @313 is dealt.
pub fn decrement_shield_from_hit(w: &mut World, f: FighterId) {
    let Some(st) = steven_of(w, f) else { return };
    let charges = st.shield.charges;
    if !(charges > 0.0) {
        return;
    }
    if charges - 1.0 <= 0.0 {
        shield_removed(w, f);
        let hit = comp(w, f).and_then(|c| obj(c, "ShieldBreakHit")).map(|p| HitPacket::from_props(&w.names, p, None));
        match hit {
            Some(p) => refused_ally_hit(w, f, f, &p, false, ActorRef::Fighter(f)), // @313
            None => warn_text(w, "Comp_StevenShield: no ShieldBreakHit"),
        }
    }
    let k = ability_index(w, f);
    let regen = comp(w, f).and_then(|c| num(c, "ShieldRegenTime")).unwrap_or(8.5);
    use_ability(w, f, k);
    set_ammo_cooldown_on_authority(w, f, k, regen, regen);
}

/// `onShieldChargeChange` (Index, NewAmmo, Increase) @1026: the shield's slot alone -> UpdateShieldChargesMulticast
/// @1233 (CurrentShieldCharges = NewAmmo; Shielding: UpdateVFX, and with Increase ShieldActivated), then
/// UpdateShieldBuffMulticast(CurrentShieldCharges) on every entry of AllySharedShieldBuff @451-418.
fn on_shield_charge_change(w: &mut World, f: FighterId, slot: i32, ammo: Real, increase: bool) {
    if steven_of(w, f).is_none() || slot != ability_index(w, f) {
        return;
    }
    let s = &mut steven_w(w, f).shield;
    s.charges = ammo;
    let shielding = s.shielding;
    if shielding && increase {
        shield_activated(w, f);
    }
    let charges = steven_w(w, f).shield.charges;
    for b in ally_buffs(w, f) {
        // UpdateShieldBuffMulticast: + UpdateVFX
        if let Some(BuffExt::StevenSharedShield(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
            e.charges = charges;
        }
    }
}

/// `shieldFighterReset`: Comp_StevenShield.OnFighterReset @998, for the pawn's BPI_Cleanup (Pawn_StevenUniverse
/// @8654-8665; joined after pawn.rs's cleanup, as the graph orders it after isGliding @8654).
pub fn reset(w: &mut World, f: FighterId) {
    if steven_of(w, f).is_none() {
        return;
    }
    let n = comp(w, f).and_then(|c| num(c, "MaxShieldCharges")).unwrap_or(3.0);
    steven_w(w, f).shield.charges = n;
}

/// `armorBreak`: the pack's type is ArmorType -> DecrementShieldFromHit.
fn armor_break(w: &mut World, f: FighterId, kind: Name, _by: Option<FighterId>, _flag: bool) {
    if kind == armor_type(w, f) {
        decrement_shield_from_hit(w, f);
    }
}

/// `decrementShieldCharge`.
fn decrement_shield_charge(w: &mut World, f: FighterId) {
    decrement_shield_from_hit(w, f);
}

/// `otherReset`: OnAllyRespawn -> SpawnAndActivateAllyShield @1309, while bound to that fighter's OnFighterReset.
fn other_reset(w: &mut World, f: FighterId, other: FighterId) {
    let Some(st) = steven_of(w, f) else { return };
    if to_int32(st.shield.ally_bound) & bit(w.fighter(other).id) != 0 {
        spawn_and_activate_ally_shield(w, f);
    }
}

/// The hooks of this file in Steven's CharHooks (the joinHooks call; `reset` is composed in mod.rs).
pub fn hooks(h: &mut CharHooks) {
    h.armor_break = Some(armor_break);
    h.decrement_shield_charge = Some(decrement_shield_charge);
    h.ability_ammo_updated = Some(on_shield_charge_change);
    h.other_reset = Some(other_reset);
}

// ---- Buff_StevenSharedShield
// Defaults (the buffs' data): CurrentShieldCharges 3, ShieldName 'StevenShield', ShieldArmorPack (CanOverride,
// ArmorAmount 1000, MaxHits 4, KnockBackThreshold 8000, DamageResistanceMultiplier 1), BuffDuration 10000 (the spawn
// passes 0: no life timer), MaxStacks 1, DestroyBuffWhenOwnerDefeated. ext: { steven: the Steven that spawned it (its
// Instigator, the component's owner), charges: CurrentShieldCharges }.

/// ShieldName, 'StevenShield' by default.
fn shield_name(w: &World, o: FighterId) -> Name {
    match text(hold_def(w, o, SHARED), "ShieldName") {
        Some(s) => lit(w, s),
        None => name!("StevenShield"),
    }
}

/// `activateShieldBuff` (ActivateShieldBuffMulticast(n) @1620): charges = n; n > 0: ShieldArmorPack with ArmorType
/// ShieldName and MaxHits n + 1 to the owner's Comp_Armor (CanOverride: a pack of its own), SpawnVFX.
fn activate_shield_buff(w: &mut World, o: FighterId, b: BuffRef, n: Real) {
    if let Some(BuffExt::StevenSharedShield(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
        e.charges = n;
    }
    if n > 0.0 {
        let p = pack(obj(hold_def(w, o, SHARED), "ShieldArmorPack"), shield_name(w, o), n + 1.0);
        add_armor_pack(w, o, &p);
    }
}

/// `removeShields` (RemoveShieldsMulticast @1652): RemoveArmor(ShieldName), RemoveVFX, K2_DestroyActor (its
/// ReceiveDestroyed @1797: RemoveArmor and RemoveVFX again: the `end` below).
fn remove_shields(w: &mut World, o: FighterId, b: BuffRef) {
    let n = shield_name(w, o);
    remove_armor(w, o, n);
    end_buff(w, o, b);
}

/// ReceiveBeginPlay binds the owner's MeshManager ActiveMeshChanged -> OnMeshChanged (the VFX alone).
fn shared_begin(w: &mut World, o: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    let charges = num(hold_def(w, o, SHARED), "CurrentShieldCharges").unwrap_or(3.0);
    let steven = from.map_or(-1, |s| w.fighter(s).id);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::StevenSharedShield(SharedShield { steven, charges });
    }
}

/// BPI On Fighter Owner Hit By Enemy @1922 -> @1854: every BP_FighterCharacter the PlayerDataManager finds, with the
/// owner, of the owner's team and not the owner, that has a Comp_StevenShield -> its DecrementShieldFromHit.
/// Unverified: GetByFighter is taken to fail for a minion (the Watermelon has the component but no PlayerData).
fn shared_on(w: &mut World, o: FighterId, _b: BuffRef, sig: BuffSignal, _arg: &BuffSignalArg) -> bool {
    if sig != BuffSignal::HitByEnemy || w.fighter(o).life.is_minion {
        return false;
    }
    let team = w.fighter(o).team.team;
    for c in peers(w).to_vec() {
        if c != o && !w.fighter(c).life.is_minion && w.fighter(c).team.team == team && steven_of(w, c).is_some() {
            decrement_shield_from_hit(w, c);
        }
    }
    false
}

fn shared_end(w: &mut World, o: FighterId, _b: BuffRef) {
    let n = shield_name(w, o);
    remove_armor(w, o, n);
}

// ---- AnimNotify_StevenArmor
// On Steven_Special_Shield_Montage and Steven_SpecialAir_Shield_Montage (t -0.0001, 1.4397 s), and on the Creature's
// StevenPrototype_DodgeN_Montage. Begin: MeshComp cast to BPI_BaseFighter (a mesh component: the cast fails, so
// NotPlayerCharacter reads false @449), then the owner's Comp_StevenShield, if it has one -> ShieldActivated @397.
// End: the owner's Comp_StevenShield -> ShieldRemoved @229, else its Comp_Armor RemoveArmor('StevenShield') @374. The
// notify's CanOverride / ArmorType are read by neither.

fn armor_notify_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if steven_of(w, f).is_some() {
        shield_activated(w, f);
    }
}

fn armor_notify_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if steven_of(w, f).is_some() {
        shield_removed(w, f);
    } else {
        remove_armor(w, f, name!("StevenShield"));
    }
}

// ---- StevenCollisionBlock
// A SpawnedActorParent child, spawned by the two shield montages' AnimNotify_Spawner (AttachToActor, identity
// transform, DestroyAllActorsOnNotifyEnd: it ends with the notify, 1.43687 s; 'Die With Instigator'). Components
// (steven.json actors.StevenCollisionBlock_C): the parent's Capsule (NoCollision) and Box (BoxExtent 100 x 100 x 150
// at the root, object type Platform = ECC_GameTraceChannel7, QueryAndPhysics, a Custom profile that ignores Item,
// Visibility, Camera, WorldStatic, WorldDynamic, PhysicsBody, Vehicle, Destructible, DamageDestructible, Platform,
// Cleanse and Portal: Pawn blocks), HitBoxOn? false.
// ReceiveBeginPlay @1017: over GetAllActorsOfClass(BaseFighter_C), each one's CapsuleComponent
// IgnoreComponentWhenMoving(Box, it has Buff_StevenBubbleEnemy_C OR its Destructible TeamIndex is the instigator's)
// @104-676, once: a fighter that gets the bubble later is not looked at again. Then Delay(0.2) ->
// Box.SetGenerateOverlapEvents(true) @70, which no handler of the class reads: not ported.
// Model: a platform box of the Box (the fighters' movement and World.blockers meet it as a box of a spawned actor:
// solid from every side), placed at the owner's actor location each step (attached).
// Deviation: the box follows its owner at the actor's step (after the fighters' moves of the frame), as the other
// attached boxes do (the platform part's Deviation). Its Comp_Actor_Destructible as a target of another fighter's hit
// box is not ported (the Box's response to HitBox is the profile's default, not read).
// Deviation: the part's ignore list is a bit per fighter id, a minion's in its own list (spawned_platform.rs).

const BOX: Name = name!("Box");

/// `BlockBox`: the Box part (half extents BoxExtent X and Z times RelativeScale3D, collision on unless NoCollision),
/// the one part of the block's `plat`.
fn block_box(c: &Props) -> PlatformPart {
    let ext = |k: &str| obj(c, "BoxExtent").and_then(|e| num(e, k)).unwrap_or(32.0);
    let sc = |k: &str| obj(c, "RelativeScale3D").and_then(|e| num(e, k)).unwrap_or(1.0);
    let mut p = PlatformPart::new(BOX, false, ext("X") * sc("X"), ext("Z") * sc("Z"), c);
    p.on = !c
        .get("BodyInstance")
        .and_then(Value::as_obj)
        .and_then(|b| b.get("CollisionEnabled"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .ends_with("NoCollision");
    p
}

/// `StevenBlock.follow`: the Box at the owner's actor location.
fn follow(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (ox, oy) = (w.fighter(a.owner).body.x, actor_y(w, a.owner));
    if let Some(pl) = plat_mut(w, id) {
        for s in &mut pl.parts {
            s.place(ox, oy);
        }
    }
}

/// `blockSpawn`: the class's spawn (the kind's `spawn`): the state, and the ignore list of the Box at the BeginPlay.
fn block_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = crate::world::spawning::request_props(w, f, r);
    let t = obj(&np, "Transforms").and_then(|t| obj(t, "Translation"));
    let nz = |k: &str| t.and_then(|t| num(t, k)).is_some_and(|v| v != 0.0);
    let socket = text(&np, "Socket");
    if !crate::registry::props::flag(&np, "AttachToActor")
        || matches!(socket, Some(s) if !s.is_empty() && s != "None")
        || nz("X")
        || nz("Y")
        || nz("Z")
    {
        let msg = format!(
            "{}: spawn form not ported (not attached at the actor's place): placed at the actor all the same",
            w.names.str(r.cls)
        );
        warn_text(w, &msg);
    }
    (WorldActorState::StevenBlock(StevenBlock {}), false)
}

/// The rest of the spawn once the World holds the actor (the kind's `spawned`): the Box part, ignored by each fighter
/// that has the enemy bubble or is on the owner's team, placed at the owner.
fn spawned(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let c = actor_components(w, ActorRef::Fighter(f), cls).and_then(|c| c.get("Box")).and_then(Value::as_obj);
    let part = c.map(block_box);
    if part.is_none() {
        let msg = format!("{}: no Box in its data: it blocks nothing", w.names.str(cls));
        warn_text(w, &msg);
    }
    let mut pl = SpawnedPlatform {
        age: 0,
        team: w.fighter(f).team.team,
        collision: None,
        wallbounce: None,
        parts: Vec::new(),
        listed: true,
        collision_at: -1.0,
        edge_at: None,
    };
    if let Some(mut p) = part {
        let team = w.fighter(f).team.team;
        for &o in peers(w) {
            let v = crate::combat::buffs::buff_of(w, o, name!("Buff_StevenBubbleEnemy_C")).is_some()
                || w.fighter(o).team.team == team;
            p.set_ignored(w.fighter(o).id, v);
        }
        pl.collision = Some(pl.push(p));
    }
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.plat = Some(pl);
    }
    follow(w, id);
}

/// `block.step`: follow.
fn block_step(w: &mut World, id: ActorId) {
    follow(w, id);
}

/// `block.size.scale`: the Box is the root's child at its place: its extents at the root's scale (the new over the
/// old).
fn block_scale(w: &mut World, id: ActorId, k: Real, old: Real) {
    if let Some(pl) = plat_mut(w, id) {
        for s in &mut pl.parts {
            s.hx *= k / old;
            s.hy *= k / old;
        }
    }
    follow(w, id);
}

/// `blockBodies`: the Box as a body of the traces (object type Platform, at the box).
fn block_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(pl) = plat_of(w, id) else { return };
    for s in &pl.parts {
        let info = if s.on { body_info(part_template(w, a.owner, a.cls, s.comp), None) } else { None };
        let Some(info) = info.filter(|i| i.on) else { continue };
        out.push(TraceBody {
            ax: s.x,
            ay: s.y,
            bx: s.x,
            by: s.y,
            hx: s.hx,
            hy: s.hy,
            r: 0.0,
            z: Real::NAN,
            object_type: info.kind,
            profile: info.profile,
            component: s.comp,
            actor: Some(ActorRef::Actor(id)),
            platform: -1,
            blocks: info.blocks,
            poly: None,
            floor: None,
        });
    }
}

/// This file's registry entries (Buff_StevenSharedShield_C, AnimNotify_StevenArmor_C, StevenCollisionBlock_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        "Buff_StevenSharedShield_C",
        BuffEntry::new(
            crate::combat::BuffClass::BuffStevenSharedShield,
            BuffHooks { begin: Some(shared_begin), on: Some(shared_on), end: Some(shared_end), ..BuffHooks::default() },
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_StevenArmor_C",
        NotifyHandler { begin: Some(armor_notify_begin), end: Some(armor_notify_end), ..NotifyHandler::default() },
    )?;
    let hooks = WorldActorHooks {
        spawn: Some(block_spawn),
        spawned: Some(spawned),
        size: Some(SizeHooks { scale: Some(block_scale), ..SizeHooks::default() }),
        bodies: Some(block_bodies),
        step: Some(block_step),
        end: Some(platform_end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        "StevenCollisionBlock_C",
        ActorEntry::new(ActorClass::World(WorldActorClass::StevenCollisionBlock), ActorHooks::World(hooks)),
    )
}

use crate::actors::ActorClass;
