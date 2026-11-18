//! Item_Mystery_C: an ItemActorParent child; held, CanUseItem. Its pools:
//! WinnersItems, MiddleItems, LastPlaceItems (class defaults).
//! PickUp @1442: the parent's, then IsHeld = false @1461; with authority a DoOnce @470-1406: the PandaGameState's
//! TeamKnockouts[Comp_Actor_Destructible.TeamIndex] as OwnerScore @612, NumberOfTeams its length @774, and
//! TeamsBeatingOwner + 1 for every other team index whose entry is above OwnerScore @1012-1280; then @15 ChooseItem:
//! r = TeamsBeatingOwner / NumberOfTeams; r < 0.25: WinnersItems; r < 0.75: WinnersItems + MiddleItems x 2; else
//! WinnersItems + MiddleItems + LastPlaceItems x 2; one Array_Random of that pool @218 (ChosenItem). The owner's
//! Comp_ItemManager valid @100: ThrowItemMulticast((0, 0, 0)) @210, then Delay(0) @284 -> @339: the owner's
//! SpawnAndEquipItem(ChosenItem) @410 (`world::items::spawn_and_equip_item`) and ReleaseToPool @455.
//! - ThrowItemMulticast @210 does nothing: PickUp runs inside EquipOnSpawn (BPI_PickUp @983), ahead of its HeldItem
//!   write (@1729), so ThrowItem finds no HeldItem and returns. SpawnAndEquipItem then drops the box itself (its
//!   HeldItem by then), and the box is released behind it.
//! - IsHeld = false keeps the box's ReceiveEndPlay from ClearHeldItem on its LastHolder (which would clear the item
//!   SpawnAndEquipItem equipped); the port's `destroy_item` clears a held item only when it is that item, and the box's
//!   PickupArea stays off in the hand (the pick-up's actor collision off), so the flag is not kept.
//!
//! Deviation: Array_Random is the world's stream (`World::random_int`).
//! Ported for a spawn with no owner: its code reads the owner from the pick-up on.

use crate::Real;
use crate::State;
use crate::actors::item::{ItemExt, item_activate_parent, item_destructible_team};
use crate::actors::{ActorClass, ItemClass, ItemId};
use crate::fighter::FighterId;
use crate::fighter::common::latent_frames;
use crate::fighter::control::data_name;
use crate::math::to_int32;
use crate::names::Name;
use crate::registry::entries::ItemHooks;
use crate::registry::props::arr;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::items::spawn_and_equip_item;

/// `MysteryItem extends Item` (Item_Mystery_C).
#[derive(State, Debug)]
pub struct MysteryItem {
    /// The DoOnce @490-505.
    pub done: bool,
    /// TeamsBeatingOwner (never reset).
    pub teams_beating: i32,
    /// ChosenItem.
    pub chosen: Name,
    /// The `World::flow.tick` its Delay(0) @284 fires at (-1: none).
    pub equip_at: i32,
}

impl Default for MysteryItem {
    fn default() -> Self {
        MysteryItem { done: false, teams_beating: 0, chosen: Name::NONE, equip_at: -1 }
    }
}

fn state(w: &mut World, it: ItemId) -> &mut MysteryItem {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::MysteryItem(m) => m,
        _ => unreachable!("Item_Mystery_C holds a MysteryItem"),
    }
}

/// A pool of the class data (`it.p[key] ?? []`), the names interned at bind.
fn pool_of(w: &World, it: ItemId, key: &str) -> Vec<Name> {
    let i = w.actors.items.get(it).expect("a live item");
    arr(i.def(&w.data).p, key).iter().filter_map(|v| v.as_str()).map(|s| data_name(w, s)).collect()
}

/// `chooseItem`.
fn choose_item(w: &mut World, it: ItemId) {
    let teams = w.flow.team_knockouts.len();
    let beating = state(w, it).teams_beating;
    #[allow(clippy::cast_precision_loss, reason = "a team count")]
    let r = crate::real(beating) / teams as Real;
    let win = pool_of(w, it, "WinnersItems");
    let mid = pool_of(w, it, "MiddleItems");
    let last = pool_of(w, it, "LastPlaceItems");
    let pool: Vec<Name> = if r < 0.25 {
        win
    } else if r < 0.75 {
        [win, mid.clone(), mid].concat()
    } else {
        [win, mid, last.clone(), last].concat()
    };
    #[allow(clippy::cast_precision_loss, reason = "a short pool")]
    let n = pool.len() as Real;
    let chosen =
        if pool.is_empty() { Name::NONE } else { pool[usize::try_from(to_int32(w.random_int(n))).expect("an index")] };
    state(w, it).chosen = chosen;
}

/// `activate`: no ActivateObject of its own.
fn activate(w: &mut World, it: ItemId, _f: FighterId) {
    item_activate_parent(w, it);
}

/// `grabbed`: the PickUp's DoOnce.
fn grabbed(w: &mut World, it: ItemId, _f: FighterId) {
    if state(w, it).done {
        return;
    }
    state(w, it).done = true;
    let team = item_destructible_team(w, it);
    let ko = &w.flow.team_knockouts;
    let own = usize::try_from(team).ok().and_then(|t| ko.get(t)).copied().unwrap_or(0.0);
    let mut beating = 0;
    for (t, &k) in ko.iter().enumerate() {
        if usize::try_from(team).ok() != Some(t) && k > own {
            beating += 1;
        }
    }
    state(w, it).teams_beating += beating;
    choose_item(w, it);
    // The Delay(0), created in the picker's tick: its first update is the box's own tick of this frame.
    let at = w.flow.tick + to_int32(latent_frames(0.0)) - 1;
    state(w, it).equip_at = at;
}

/// `step`: the Delay(0)'s end (ReleaseToPool @455); true destroys the item.
fn step(w: &mut World, it: ItemId) -> bool {
    let (at, chosen) = {
        let m = state(w, it);
        (m.equip_at, m.chosen)
    };
    if at < 0 || w.flow.tick < at {
        return false;
    }
    state(w, it).equip_at = -1;
    let f = w.actors.items.get(it).expect("a live item").owner.expect("a held Item_Mystery_C has its owner");
    spawn_and_equip_item(w, f, chosen);
    true
}

fn sub() -> ItemExt {
    ItemExt::MysteryItem(MysteryItem::default())
}

/// This file's registry entries (Item_Mystery_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks {
        sub: Some(sub),
        activate: Some(activate),
        grabbed: Some(grabbed),
        step: Some(step),
        ..ItemHooks::default()
    };
    r.actors.enter(
        ItemClass::ItemMystery.name(),
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::ItemMystery), ActorHooks::Item(hooks))
        },
    )
}
