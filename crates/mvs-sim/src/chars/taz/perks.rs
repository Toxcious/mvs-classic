//! The perk EatProjectiles: Buff_C015_EatProjectiles and C015Anvil_C ([`Anvil`], an
//! item).
//!
//! The signature perks of Taz: EatProjectiles, self-only (SpawnOnAllies false). What the perk changes is in the kit
//! files that hold the branches, each reading the marker with `buff_of`:
//! - projectile_eat.rs setIsEatingProjectile / storeProjectile: with the buff any eaten actor is stored (@6977), and
//!   the stored class is C015Anvil_C (@69-107);
//! - eat.rs C015EatMedium: false with the buff (its own test); C015EatLarge: the stored class is C015Anvil_C, so the
//!   spit montages' anvil notify (AnimNotify_C015ResetConditionalSpawner, Ground [8] / Air [9]) spawns it;
//! - burp.rs: the anvil is the perk part's class (the taz perk part's "actors"), not an eaten one of another kit;
//! - the HUD's eat meter: UpdateMeterIcon's BurpAnvil.

use crate::Real;
use crate::State;
use crate::actors::item::{Item, ItemDef, ItemExt, num};
use crate::actors::{ActorClass, ActorRef, ItemClass, ItemId};
use crate::combat::BuffClass;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_data, def_is_projectile};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::projectile_speed_buffed;
use crate::math::max;
use crate::math::ue::size;
use crate::name;
use crate::registry::entries::{BuffEntry, BuffHooks, ItemHooks};
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use std::sync::Arc;

/// `Anvil extends Item` (C015Anvil_C; also NeutralAnvil_C's, which the FFA area spawns with no owner).
///
/// No graph of its own (C015Anvil: UserConstructionScript turns Nav2's collision off, the parent's). Everything it
/// does is ItemActorParent's (`actors::item`) from its class defaults (the perk part's "actors"): Durability 3,
/// HitBoxOn? true, IsThrown false (ItemActorParent's default), RestrictedToTeam, NumberOfActorsAllowed 1, CanUseItem
/// false, CanBeHitWhileIdle / CanBeMeleedWhileIdle, Hit 10 / 1700 / 15 / (0.5, 0.5), pause 0.2, Heavy (attack type
/// 1), DestroyProjectiles, OnHitBounce 500, ItemIdleLife 15; the ProjectileMovement on (EnableProjectileMovement),
/// gravity scale 3, no bounce.
/// The root Capsule's profile ProjectileNavNoPlatform ignores the Platform channel (and Pawn, PhysicsBody, Item): from
/// the spawn the stage's pass-through platforms (and the other bodies of the item blockers) do not block it. Every
/// ItemThrowHit (a spend's, ItemActorParent ReceiveHit @4915 / HitEnemyPlayer / HitNonPlayer, and an ally's,
/// HitAlly @7501) runs @7378 -> Server @7558 -> ItemThrowHitMulticast @7609, whose @7620
/// SetCollisionResponseToChannel(20, Block) makes the capsule block the Platform channel from then on (the Platform
/// profile blocks PhysicsBody, DefaultEngine.ini :574, :592): after its first hit the anvil falls back onto a
/// platform and lies there (`Item::step`'s rule; a pick-up and a throw are Item's IsThrown rule as for any item).
/// `platforms_on` is that response (`Item::throw_hit` sets it, `Item::step` passes the platforms only with it).
/// Unverified: before the first hit the move takes the stage block alone; a body of the item blockers that is not of
/// the Platform object type (a ship's piece, an actor's platform), which the profile may block, is not met.
#[derive(State, Debug, Default)]
pub struct Anvil {
    /// The Capsule's response to channel 20 (Platform): Ignore from the profile, Block after @7620.
    pub platforms_on: bool,
}

/// Comp_Spawner HandleActorPostSpawn with EnableProjectileMovement (the BPI_SetInitialMovement branch, no impulse), as
/// Projectile's constructor computes it: ApplyProjectileSpeedBuffsToInputs on InitialSpeed and MaxSpeed (the class
/// has BPI_Projectiles), the launch VSize(direction x InitialSpeed), the velocity direction x InitialSpeed mirrored by
/// the facing, MaxSpeed max(launch, the notify's). The anvil notifies set no charge, momentum or movement-speed
/// values.
/// Unverified: HitBoxOn? (true in the class defaults) is taken as the ThrowHitBox's collision from the spawn, as
/// Item_BatarangV2's (`chars::batman::batarang`); ItemActorParent's UpdateHitbox at BeginPlay is not read.
fn make(w: &mut World, f: FighterId, r: &SpawnRequest, x: Real, y: Real) -> ItemId {
    let n = crate::world::spawning::request_props(w, f, r);
    let (dy, dz) = match n.get("InitialMovementDirection").and_then(|v| v.as_obj()) {
        Some(d) => (num(d, "Y").unwrap_or(0.0), num(d, "Z").unwrap_or(0.0)),
        None => (1.0, 0.0),
    };
    let cls = name!("C015Anvil_C");
    let data = Arc::clone(&w.data);
    let buffed = actor_data(w, &data, ActorRef::Fighter(f), cls).is_some_and(def_is_projectile);
    let initial = num(&n, "InitialSpeed").unwrap_or(0.0);
    let speed = if buffed { projectile_speed_buffed(w, f, initial) } else { initial };
    let launch = speed * size([0.0, dy, dz]);
    let facing = crate::fighter::facing(w, f);
    let (vx, vy) = (dy * speed * facing, dz * speed);
    let src = crate::world::items::resolve_def(w, Some(f), ItemClass::C015Anvil.name())
        .expect("C015Anvil_C in its spawner's data");
    let sb = crate::world::blast::spawn_box(w);
    let class = ItemClass::C015Anvil;
    let mut it =
        Item::make(w, cls, class, Some(f), src, ItemDef::at(&data, src), x, y, 0.0, facing, vx, vy, &sb, false);
    it.flying = true;
    it.hit_on = true;
    let max_n = num(&n, "MaxSpeed").unwrap_or(0.0);
    let max_n = if buffed { projectile_speed_buffed(w, f, max_n) } else { max_n };
    it.max_speed = max(launch, max_n);
    it.attack_id = r.attack_id;
    it.ext = sub();
    w.actors.items.insert(it)
}

/// The class's own variables (an item made outside [`make`]).
fn sub() -> ItemExt {
    ItemExt::Anvil(Anvil::default())
}

/// This file's registry entries (Buff_C015_EatProjectiles_C, C015Anvil_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // Buff_C015_EatProjectiles: ReceiveBeginPlay @190 -> @151: with authority, the owner's BPI_UpdateMeterIcon @109
    // when it implements BPI_C015 (Pawn_C015 @8527 -> @881: MeterUI valid -> MeterUI.RightSideIcon.
    // SetBrushFromTexture(UI_AbilityIcons_C015_BurpAnvil, false) @924; not valid -> Delay(0.0) @993 and @881 again).
    // That is a widget write only, with no sim state: the HUD reads the marker. Nothing else in the class: the rest
    // of the perk is the pawn's CheckHasBuff on it.
    r.buffs
        .enter("Buff_C015_EatProjectiles_C", BuffEntry::new(BuffClass::BuffC015EatProjectiles, BuffHooks::default()))?;
    let hooks = ItemHooks { make: Some(make), sub: Some(sub), ..ItemHooks::default() };
    r.actors.enter("C015Anvil_C", ActorEntry::new(ActorClass::Item(ItemClass::C015Anvil), ActorHooks::Item(hooks)))?;
    // NeutralAnvil_C, a child of C015Anvil_C that only sets defaults: the parent's own variables,
    // made by the FFA area's spawn with no owner (not through [`make`], which is the burp's).
    let hooks = ItemHooks { sub: Some(sub), ..ItemHooks::default() };
    r.actors.enter(
        "NeutralAnvil_C",
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::NeutralAnvil), ActorHooks::Item(hooks))
        },
    )?;
    Ok(())
}
