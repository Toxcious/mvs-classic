//! Buff_Cheese (Blueprints/Abilities/Shield, offsets of its ubergraph): the
//! mousetrap's bait on a fighter, its stacks and its UI state (CheeseStack, MI_State_CheeseBuff); a fighter buff (no
//! ActorBuffSignal) granted by Jerry's follow @3543 (follower.rs: 0 stacks, Buff Duration 10 s, Instigator Tom), read by
//! MouseTrap_SpawnedActor @4627-4907 (trap.rs: the bait hit and AddStack(1)).
//!
//! Its mods are data (DealtDamageMods: damage +2, knockback x1.05, whatever the stack count; MaxStacks 6, CanStack):
//! the shared buff code.
//! - ReceiveBeginPlay @2378: the parent's, then Delay(0.1) @2191 -> @15: the owner a BPI_BaseFighter -> BPI_PopText
//!   "+ Damage" @134-643 (lifetime 0.5, white, (0, 0, 200), space 3, justification 1, attached to the owner); on
//!   authority SetUiServer(false, CurrentStackAmount) @719, the owner's Comp_Pawn_HitBoxHandler.HitEnemyPlayer ->
//!   UpdateCheeseStacks @743-837, K2_AttachToActor(the owner) @898 (nothing of the sim).
//! - SetUiServer(Debuff, Index) @2557 -> SetUIMulticast @2660 (a texture UI_CheeseBuff_01..04 by Index, which nothing
//!   reads) -> @938: the owner a BaseFighter; not being destroyed and CurrentStackAmount >= 0 -> the UI_Broker's
//!   AddStateForPlayer {id CheeseStack, StateType 2, DisplayType 0, values -1, Timer -1, MI_State_CheeseBuff, Active,
//!   PopText '', AutoRemoveTime -1, no Subicon} @1251-1647; else RemoveStateForPlayer(CheeseStack) @1858.
//! - AddStack(n) @2246 (its override): the parent's AddStack, then on authority SetUiServer(false, CurrentStackAmount)
//!   @2295 and, at 4 stacks or more, K2_DestroyActor @2363 (`stacks`).
//! - UpdateCheeseStacks (HitEnemyPlayer: the owner's own hit on an enemy fighter, BuffSignal HitEnemy) @3479 ->
//!   AddStack(1) @2795, then the owner a BPI_BaseFighter -> BPI_PopText "+2" @2929-3432.
//! - ReceiveEndPlay @1917: the parent's, then RemoveStateForPlayer(CheeseStack) @2132.
//! - HealVisuals @2398 (PS_Heal): no caller (xref): dormant.
//!
//! So a follow's cheese ends after 10 s or at the owner's fourth hit (0 + 4 stacks).
//! The pop texts: the fighter's PopText event, kind TjCheese: stacks 0 for "+ Damage" (the BeginPlay's), 2 for "+2" (a
//! hit's).
// Unverified: the "+2" of the hit that destroyed the buff is not shown: K2_DestroyActor clears the actor's Owner
// (UWorld::DestroyActor's SetOwner(nullptr), stock 4.26, not read in the exe), so GetOwner's cast @2834 fails.
// Unverified: the Delay 0.1 counts the buff's ticks from its spawn (`tick`, as fighter/buffs.rs's latent actions).

use crate::Real;
use crate::State;
use crate::combat::buffs::{BuffExt, buff, buff_mut, buff_of, remove_buff, stack_buff};
use crate::combat::{BuffClass, BuffRef};
use crate::events::{Event, PopKind, UiStateOp, UiStateParams};
use crate::fighter::FighterId;
use crate::fighter::common::latent_frames;
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// The buff's class.
pub const CHEESE: &str = "Buff_Cheese_C";
/// Its UI state (UI_StateParams.id).
pub const CHEESE_STATE: &str = "CheeseStack";
/// Its UI state's icon; the icon's HUD entry is the view's.
pub const CHEESE_ICON: &str = "MI_State_CheeseBuff";

/// `CheeseExt`: the buff's own variables (`BuffExt::Cheese`): the Delay 0.1's ticks left, and UpdateCheeseStacks bound.
#[derive(State, Debug, Default)]
pub struct CheeseExt {
    /// Ticks left of the Delay(0.1) @2191 (0: not running).
    pub delay: i32,
    /// The Delay ran: the UI set and the hit binding live.
    pub bound: bool,
}

/// The buff's variables, none for a buff of another extension.
fn ext(w: &mut World, b: BuffRef) -> Option<&mut CheeseExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::Cheese(e) => Some(e),
        _ => None,
    }
}

/// The buff is on the fighter and alive.
fn live(w: &World, f: FighterId, b: BuffRef) -> bool {
    buff(w, b).is_some_and(|x| x.frames > 0.0) && w.fighter(f).buffs.buffs.iter().any(|x| x.serial == b.serial)
}

/// SetUiServer -> SetUIMulticast -> @938 (the buff alive and its count >= 0: the state shown, else removed).
fn set_ui(w: &mut World, f: FighterId, b: BuffRef) {
    let shown = live(w, f, b) && buff(w, b).is_some_and(|x| x.stacks >= 0.0);
    let ops = &mut w.fighter_mut(f).controls.ui_state_ops;
    if shown {
        ops.push(UiStateOp::Add(UiStateParams {
            id: name!("CheeseStack"),
            state_type: 2,
            display_type: 0,
            current_value: -1.0,
            max_value: -1.0,
            timer: -1.0,
            icon_material: name!("MI_State_CheeseBuff"),
            active: true,
            pop_text: Name::NONE,
            auto_remove_time: -1.0,
            subicon: None,
        }));
    } else {
        ops.push(UiStateOp::Remove(name!("CheeseStack")));
    }
}

fn pop(w: &mut World, f: FighterId, stacks: Real) {
    let fighter = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText { fighter, kind: PopKind::TjCheese, stacks, gain: None, params: None });
}

/// `addCheeseStack`: Buff_Cheese.AddStack(AddStackCount) on `f`'s Buff_Cheese (none: nothing). Caller: MouseTrap
/// @4627-4907 (trap.rs), after its cast @4627.
pub fn add_cheese_stack(w: &mut World, f: FighterId, n: i32) {
    if let Some(b) = buff_of(w, f, name!("Buff_Cheese_C")) {
        stack_buff(w, f, b, crate::real(n));
    }
}

/// `hasCheese`: `f` holds a Buff_Cheese (the cast MouseTrap makes @4627: Comp_State_Manager CheckHasBuff). Called by
/// trap.rs.
pub fn has_cheese(w: &World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_Cheese_C")).is_some()
}

/// `removeCheese`: the follower's Comp_State_Manager.RemoveBuff(Buff_Cheese_C, 0) in Jerry's reset @983-1128 (Tom's
/// overlap with his following Jerry, @15848 -> @940; jerry.rs calls it with the Jerry's `following`). Called by
/// follower.rs's callers.
pub fn remove_cheese(w: &mut World, f: FighterId) {
    remove_buff(w, f, Some(name!("Buff_Cheese_C")), None);
}

fn begin(w: &mut World, _f: FighterId, b: BuffRef, _by: Option<FighterId>, _secs: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Cheese(CheeseExt { delay: to_int32(latent_frames(0.1)), bound: false });
    }
}

/// The Delay(0.1) @2191 -> @15.
fn tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = ext(w, b) else { return false };
    if e.delay <= 0 {
        return false;
    }
    e.delay -= 1;
    if e.delay > 0 {
        return false;
    }
    pop(w, f, 0.0); // "+ Damage" @643
    set_ui(w, f, b); // @719
    if let Some(e) = ext(w, b) {
        e.bound = true; // @837
    }
    false
}

fn on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, _arg: &BuffSignalArg) -> bool {
    if s != BuffSignal::HitEnemy || !ext(w, b).is_some_and(|e| e.bound) {
        return false;
    }
    // UpdateCheeseStacks @3479 -> AddStack(1) @2795 (the override below runs behind the parent's).
    stack_buff(w, f, b, 1.0);
    if live(w, f, b) {
        pop(w, f, 2.0); // "+2" @3432
    }
    false
}

/// AddStack's override @2246-2363 (RemoveStack has none).
fn stacks(w: &mut World, f: FighterId, b: BuffRef, added: bool) -> bool {
    if !added {
        return false;
    }
    set_ui(w, f, b); // @2295
    buff(w, b).is_some_and(|x| x.stacks >= 4.0) // @2319-2363
}

fn end(w: &mut World, f: FighterId, _b: BuffRef) {
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("CheeseStack"))); // @2132
}

/// This file's registry entries (Buff_Cheese_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = BuffHooks {
        begin: Some(begin),
        tick: Some(tick),
        on: Some(on),
        stacks: Some(stacks),
        end: Some(end),
        ..BuffHooks::default()
    };
    r.buffs.enter(CHEESE, BuffEntry::new(BuffClass::BuffCheese, hooks))?;
    Ok(())
}
