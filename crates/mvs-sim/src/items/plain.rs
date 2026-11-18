//! Item_Rock_C, Item_Pie_C and Item_DrumStick_C: the ItemActorParent children whose
//! graph adds nothing the sim keeps (`actors::item` does the parent's); what each does comes from its class defaults.
//! - Item_Rock_C: no function of its own. CanUseItem false, Durability 3, ThrowHitBox radius 60; hit 7 / 1250 / 15.
//! - Item_Pie_C: ReceiveBeginPlay, PickUp, Thrown and Drop only turn its mesh (the parent's call first in each); its
//!   'On Hit Buffs' row gives Buff_SlowStackable to an enemy it hits (ApplyBuffHit? and ApplyBuffEnemy?, stacks 1,
//!   duration 0).
//! - Item_DrumStick_C: no function of its own. CanUseItem false, CanEnemyDestroy false, HitBoxOn? true; hit 10 / 1600 /
//!   10 at (0, 1); its 'On Hit Buffs' row gives Buff_Chicken to an enemy it hits (stacks 0, 8 s).
//!
//! CanUseItem false: ActivateObject is ItemActorParent's, which drops the item, made in the use notify itself. Each is
//! ported for a spawn with no owner: their code is ItemActorParent's, which reads the owner only once a fighter holds
//! or threw the item (`Item::owner`).
//!
//! Buff_SlowStackable_C (the pie's slow): SpeedMod 0.8 (Multiply), MaxStacks 3, BuffDuration 6, StackDecayRate 3,
//! BlockerGroups [StateGroups byte 8, CC Immune]; the parent's graph does the rest. Its graph is BPI After Status
//! Update Multicast (@196-974) alone: BuffRemoved or NewStackCount <= 0 -> RemoveStateForPlayer(SlowDown); else
//! AddStateForPlayer {SlowDown, StateType 1, DisplayType 1, NewStackCount of MaxStacks, Timer StackDecayRate,
//! MI_State_Slow, Active, PopText "Slowed" when BuffApplied, AutoRemoveTime -1, no sub icon}.

use crate::Real;
use crate::actors::{ActorClass, ActorRef, ItemClass};
use crate::combat::BuffClass;
use crate::events::{UiStateOp, UiStateParams};
use crate::fighter::FighterId;
use crate::fighter::common::buff_def;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks, ItemHooks};
use crate::registry::props::num;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;

/// `status`: the owner's cast to BP_FighterCharacter @30 (a minion: nothing), then the UI state.
fn slow_status(w: &mut World, f: FighterId, _c: BuffClass, stacks: Real, applied: bool, removed: bool) {
    if w.fighter(f).life.not_player_character {
        return;
    }
    if removed || stacks <= 0.0 {
        w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("SlowDown")));
        return;
    }
    let d = buff_def(w, ActorRef::Fighter(f), name!("Buff_SlowStackable_C"));
    let max = d.and_then(|d| num(d, "MaxStacks")).unwrap_or(1.0);
    let timer = d.and_then(|d| num(d, "StackDecayRate")).unwrap_or(0.0);
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: name!("SlowDown"),
        state_type: 1,
        display_type: 1,
        current_value: stacks,
        max_value: max,
        timer,
        icon_material: name!("MI_State_Slow"),
        active: true,
        pop_text: if applied { name!("Slowed") } else { Name::NONE },
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

/// This file's registry entries.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for class in [ItemClass::ItemRock, ItemClass::ItemPie, ItemClass::ItemDrumStick] {
        r.actors.enter(
            class.name(),
            ActorEntry {
                ownerless: true,
                ..ActorEntry::new(ActorClass::Item(class), ActorHooks::Item(ItemHooks::default()))
            },
        )?;
    }
    let slow =
        BuffEntry::new(BuffClass::BuffSlowStackable, BuffHooks { status: Some(slow_status), ..BuffHooks::default() });
    r.buffs.enter(BuffClass::BuffSlowStackable.registered_name().expect("a registered class"), slow)
}
