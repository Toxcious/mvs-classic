//! Pawn_Harley's own logic: the conditions HarleyBombEnabledCondition,
//! HarleyHasBullsEyePerk / HarleyHasNoBullsEyePerk and HarleyGumCondition, and the hooks equip (SetIdleAnim), reset
//! (BPI_Cleanup's UnequipObject) and idleAnim. The bot injections (`botInjections`) come with the bots' crate.

use super::state::{harley, harley_w};
use crate::actors::{ActorRef, WorldActorState};
use crate::combat::buffs::has_buff;
use crate::events::NotifyRef;
use crate::fighter::anim::montages::notify_data;
use crate::fighter::anim::update::{nmo, parts_of};
use crate::fighter::control::char_data;
use crate::fighter::{FighterId, has_equipped, unequip_object};
use crate::name;
use crate::names::Name;
use crate::registry::entries::Condition;
use crate::registry::props::truthy;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// Buff_Harley_Bullseye_C, the satchel's equip tag and the BubbleGum component's tag (Fighter.subHidden keeps tags in
/// lower case).
const BULLSEYE: Name = name!("Buff_Harley_Bullseye_C");
const GUM: Name = name!("bubblegum");
const SATCHEL: Name = name!("satchelcharge");
const HOLD_IDLE: Name = name!("HarleyQuinn_Nav_Idle_HoldItem");

/// The hooks of this file in Harley's CharHooks (`mod.rs`: equip, reset, idleAnim).
pub fn hooks(h: &mut CharHooks) {
    h.equip = Some(equip);
    h.reset = Some(reset);
    h.idle_anim = Some(idle_anim);
}

/// HarleyQuinn_AnimBP SetIdleAnim, at the ItemEquipped / ItemUnequipped broadcasts of AnimNotify_Equip's list
/// writes (Comp_Spawner AddEquippedComponentMulticast @9191-9259, RemoveEquippedComponentMulticast @9279-9339):
/// its Begin always makes one (@1267 / @1598), its End with UnequipAtEnd (@545) or EquipAtEnd (@1173); this
/// hook runs after the notify's own steps, so the list is the one the broadcast's handler read. An End that
/// writes nothing broadcasts nothing.
/// The AnimBP's BPI_HoldingItem (the notify's @625-705, Comp_Spawner's @6380) only stores IsHoldingItem, which
/// nothing reads before SetIdleAnim writes it again: no `holdingItem` hook.
fn equip(w: &mut World, f: FighterId, n: NotifyRef, begin: bool) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let flag = |k: &str| p.get(k).is_some_and(truthy);
    if begin || flag("UnequipAtEnd") || flag("EquipAtEnd") {
        let on = has_equipped(w, f, SATCHEL);
        harley_w(w, f).hold_idle = on;
    }
}

/// BPI_Cleanup @1588 (BaseFighter ResetFighterLogic @554 calls it): Comp_Spawner.UnequipObject('SatchelCharge')
/// -> UpdateEquippedComponentMulticast(tag, false) @5818: the name out of the list @6443, the mesh hidden @6574,
/// BPI_HoldingItem @6380. That path makes no ItemUnequipped broadcast (only @9339 does), so SetIdleAnim does not
/// run: a Harley Quinn reset while holding the bomb keeps the hold idle until her next Equip notify.
// Unverified: as the graphs give it; not seen in the game.
fn reset(w: &mut World, f: FighterId) {
    unequip_object(w, f, name!("SatchelCharge"));
}

/// SetIdleAnim @0-115: LikedBPRef.Anim_Idle = HarleyQuinn_Nav_Idle_HoldItem (@28) or HarleyQuinn_Nav_Idle (@74,
/// the anim set's own).
fn idle_anim(w: &World, f: FighterId) -> Name {
    if harley(w, f).hold_idle {
        return HOLD_IDLE;
    }
    nmo(&w.names, &parts_of(&w.data, w, f).an.idle)
}

/// HarleyBombEnabledCondition Check (HarleyBombEnabledCondition) @161-507:
/// FilterArray(the owner's Comp_Spawner.SpawnedActors, the class HarleyBomb_C) and true for its first valid
/// entry; without one, or without a spawner, false @523. A bomb counts from its spawn to its destruction,
/// attached or not (the list is the spawner's own: `Items::spawned_actors`, which the world appends to at the
/// spawn and sweeps of dead actors).
fn bomb_enabled(w: &mut World, f: FighterId) -> bool {
    w.fighter(f).items.spawned_actors.iter().any(|&a| match a {
        ActorRef::Actor(id) => {
            w.actors.actors.get(id).is_some_and(|x| matches!(x.state, WorldActorState::HarleyBomb(_)) && !x.dead)
        }
        _ => false,
    })
}

/// HarleyHasBullsEyePerk Check (HarleyHasBullsEyePerk) @131-297: the owner's
/// BPI_CheckIfHasBuff(Buff_Harley_Bullseye_C)'s HasBuff; no fighter interface false (@321, @337). The buff is
/// the Bullseye ("Glove Control") perk's marker (Perk_Harley_Bullseye; perks.rs): with it air down plays
/// HarleyQuinn_Attack_Air_Bullseye_Montage, without it HarleyQuinn_Attack_Air_GloveShot_Montage.
fn has_bullseye(w: &mut World, f: FighterId) -> bool {
    has_buff(w, f, BULLSEYE)
}

/// HarleyHasNoBullsEyePerk Check @0-58, a child of HarleyHasBullsEyePerk with its own Check: Not(the parent's Check).
fn has_no_bullseye(w: &mut World, f: FighterId) -> bool {
    !has_bullseye(w, f)
}

/// HarleyGumCondition Check @0-307: Comp_Pawn_MeshManager Get Mesh(BubbleGum) and that mesh's bVisible; no such mesh
/// false (@296). The BubbleGum component starts hidden and no notify of hers shows it, so the alternate idle's
/// Ak_OneShotWithCondition (sfx_cha_harley_foley_idle_alt) stays silent.
fn gum(w: &mut World, f: FighterId) -> bool {
    let has = char_data(w, f).def.sub_meshes.iter().any(|s| s == "bubblegum");
    has && !w.fighter(f).pose.sub_hidden.contains(&GUM)
}

/// This file's registry entries (the four conditions).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("HarleyBombEnabledCondition_C", Condition::Fn(bomb_enabled))?;
    r.conditions.enter("HarleyHasBullsEyePerk_C", Condition::Fn(has_bullseye))?;
    r.conditions.enter("HarleyHasNoBullsEyePerk_C", Condition::Fn(has_no_bullseye))?;
    r.conditions.enter("HarleyGumCondition_C", Condition::Fn(gum))?;
    Ok(())
}
