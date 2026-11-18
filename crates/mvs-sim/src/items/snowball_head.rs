//! Buff_SnowballHead_C (IsDebuff, CanStack, MaxStacks 4, StackDecayRate 7,
//! BuffDuration 0, UIOffsetIncrease 100): the snowman head a snowball's hit gives (`snowball`: On Hit Buffs, one
//! stack; the super snowball's four).
//! - AddStack @15, the class's own: the parent's AddStack, then FreezeOnMaxStacks @2907: CurrentStackAmount >=
//!   MaxStacks -> the owner's Comp_State_Manager.SpawnBuffActor(Buff_Ice_C, the owner, self, 15 stacks, duration 0)
//!   @3058 and (Buff_Weakened_C, 5 stacks, duration 0) @3156, P_Impact_Ice at the buff's location @3282,
//!   K2_DestroyActor @3351. BeginPlay takes its count with no AddStack, so a fresh four-stack head (the super
//!   snowball's) does not freeze.
//! - BPI After Status Update Multicast @1370: the owner's UI broker, RemoveStateForPlayer(Snowman) when removed or at
//!   no stacks, else AddStateForPlayer(Snowman, StateType 1, DisplayType 0, the count of MaxStacks, Timer -1,
//!   UI_State_Snowman, PopText 'Snowman' when applied, else empty).
//! - ReceiveDestroyed @4224: P_Impact_Ice at the owner's location and sfx_shared_item_snowball_debuff_end there.
//!
//! The snowman head (SnowmanHead attached to the owner's mesh at Head ATT, Comp_Pawn_MeshManager.AddMesh; Pawn_C017's
//! BodyMesh @2388), AdjustScalePerStack and the UI offset (BPI_SetUITransformTop +- UIOffsetIncrease, BeginPlay @288 /
//! EndPlay @3370) are the view's. SpawnBuffActor's Instigator is the buff actor itself, no pawn: the call takes its
//! owner (`spawn_buff`), the head's wearer.
//!
//! Unverified: the buff actor's location (P_Impact_Ice @3254) taken as its owner's actor location (the actor is not
//! moved after its spawn at the owner).
//!
//! On a spawned actor (a snowball's On Hit Buffs reach any HitActor of another team that survives the hit: pfgDoDamage
//! @19011, `combat::projectile_buffs`), every cast to a fighter fails:
//! - BeginPlay: no mesh to attach to, so K2_AttachToComponent(None, Head ATT) @217 returns with nothing done
//!   (USceneComponent::AttachToComponent 142988650: Parent None -> false): SnowmanHead stays under the buff actor's
//!   DefaultSceneRoot, drawn at the buff actor's transform, which SpawnBuffActor @710 / @933 gave it (the HitActor's
//!   at the spawn) and nothing moves after; no UI offset, no AddMesh but on an owner with a Comp_Pawn_MeshManager
//!   (C018_Platform: AddMesh(SnowmanHead) @1056, not followed by the port). The transform is kept in the extension
//!   ([`ElemExt::SnowballHead`]: x, y, scale) for the view.
//! - AddStack's FreezeOnMaxStacks stops at the BaseFighter_C cast @3044 (no Ice, no Weakened, no self-destroy); the
//!   status multicast @1370 and ReceiveEndPlay @3370 do nothing past the parent's. UpdatePlayerStatus gives the head
//!   size to a fighter alone (@715), and SpeedModType is DoNothing.
//! - ReceiveDestroyed @4224 has no cast: P_Impact_Ice and sfx_shared_item_snowball_debuff_end at the owner's
//!   location. It ends by the parent's stack timer (7 s a stack) or with the actor (its children destroyed).

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_owner, actor_place};
use crate::combat::buffs::{buff, spawn_buff};
use crate::combat::elem::{ElemExt, elem_buff_mut};
use crate::combat::{BuffClass, BuffRef};
use crate::events::{Event, UiStateOp, UiStateParams};
use crate::fighter::common::buff_def;
use crate::fighter::{FighterId, actor_y};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks};
use crate::registry::props::num;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;

/// The class data's MaxStacks (`d.MaxStacks ?? 4`).
fn max_stacks(w: &World, f: FighterId) -> Real {
    buff_def(w, ActorRef::Fighter(f), name!("Buff_SnowballHead_C")).and_then(|d| num(d, "MaxStacks")).unwrap_or(4.0)
}

/// P_Impact_Ice at `(x, y)` for `fighter`: the buff's death effect.
fn ice_impact(w: &mut World, fighter: i32, x: Real, y: Real) {
    w.events.push(Event::ProjectileDeath {
        cls: name!("Buff_SnowballHead_C"),
        fighter,
        vfx: Some(name!("P_Impact_Ice")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
}

/// `iceImpact`: the same on the fighter's own event list (`f.events`, flushed with its tick), at its place.
fn fighter_ice_impact(w: &mut World, f: FighterId) {
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    crate::fighter::control::emit(w, f, |fighter| Event::ProjectileDeath {
        cls: name!("Buff_SnowballHead_C"),
        fighter,
        vfx: Some(name!("P_Impact_Ice")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
}

/// `stacks`: FreezeOnMaxStacks after an AddStack; true is the buff's own K2_DestroyActor.
fn stacks(w: &mut World, f: FighterId, b: BuffRef, added: bool) -> bool {
    let Some(n) = buff(w, b).map(|x| x.stacks) else { return false };
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(a >= b)")]
    if !added || !(n >= max_stacks(w, f)) {
        return false;
    }
    spawn_buff(w, f, name!("Buff_Ice_C"), 0.0, Some(15), Some(f), true, None);
    spawn_buff(w, f, name!("Buff_Weakened_C"), 0.0, Some(5), Some(f), true, None);
    fighter_ice_impact(w, f);
    true
}

/// `end`: ReceiveDestroyed @4224.
fn end(w: &mut World, f: FighterId, _b: BuffRef) {
    if reset_pass(w) {
        return; // the Lab reset's end pass
    }
    fighter_ice_impact(w, f);
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    crate::fighter::control::emit(w, f, |fighter| Event::Sound {
        fighter,
        ev: name!("sfx_shared_item_snowball_debuff_end"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// `status`: the owner's UI broker.
fn status(w: &mut World, f: FighterId, _c: BuffClass, n: Real, applied: bool, removed: bool) {
    if removed || n <= 0.0 {
        w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("Snowman")));
        return;
    }
    let max = max_stacks(w, f);
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: name!("Snowman"),
        state_type: 1,
        display_type: 0,
        current_value: n,
        max_value: max,
        timer: -1.0,
        icon_material: name!("UI_State_Snowman"),
        active: true,
        pop_text: if applied { name!("Snowman") } else { Name::NONE },
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

/// The spawned actor's root scale (a host with no scale field: 1).
fn host_scale(w: &World, a: ActorRef) -> Real {
    match a {
        ActorRef::Item(id) => w.actors.items.get(id).map_or(1.0, |i| i.scale),
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(1.0, |p| p.scale),
        _ => 1.0,
    }
}

/// `actor.begin`: the head's transform, kept for the view.
fn actor_begin(w: &mut World, a: ActorRef, i: u32, _from: Option<FighterId>) -> bool {
    let (x, y) = actor_place(w, a).map_or((0.0, 0.0), |p| (p.0, p.1));
    let scale = host_scale(w, a);
    if let Some(b) = elem_buff_mut(w, a, i) {
        b.ext = ElemExt::SnowballHead { x, y, scale };
    }
    false
}

/// `actor.end`: ReceiveDestroyed @4224 at the actor's place.
fn actor_end(w: &mut World, a: ActorRef, _i: u32) {
    if reset_pass(w) {
        return; // the Lab reset's end pass
    }
    let (x, y) = actor_place(w, a).map_or((0.0, 0.0), |p| (p.0, p.1));
    let owner = actor_owner(w, a);
    let id = owner.map_or(-1, |f| w.fighter(f).id);
    ice_impact(w, id, x, y);
    w.events.push(Event::Sound {
        fighter: id,
        ev: name!("sfx_shared_item_snowball_debuff_end"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// This file's registry entries (Buff_SnowballHead_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let entry = BuffEntry {
        actor: Some(ActorBuffHooks { begin: Some(actor_begin), end: Some(actor_end), ..ActorBuffHooks::default() }),
        ..BuffEntry::new(
            BuffClass::BuffSnowballHead,
            BuffHooks { stacks: Some(stacks), end: Some(end), status: Some(status), ..BuffHooks::default() },
        )
    };
    r.buffs.enter(BuffClass::BuffSnowballHead.registered_name().expect("a registered class"), entry)
}
