//! Morty's perk classes: Buff_C019_Hammer and Buff_C019_Cleaner (entered empty: their
//! presence is what the conditions read) and Buff_C019_Cook ([`Cook`], a buff's `ext`: its afterSpawnedActor
//! signal sets the cook grenade's UseCookDuration or UseMaxRadius).
//!
//! - Buff_C019_Hammer_C and Buff_C019_Cleaner_C have no graph of their own: C019HammerPerkEquipped /
//!   C019HammerPerkNotEquipped (C019_Attack_Arm_Combo2's two branch notifies) and C019CleanerPerkEquipped
//!   (C019_Attack_Down_Scrub's ConditionalSpawner of C019ScrubShield_C) read their presence (pawn.rs).
//! - Buff_C019_Cook_C: ReceiveBeginPlay @357 -> @10, authority, the owner a BP_FighterCharacter with a valid
//!   Comp_Spawner: binds its AfterSpawnedActor to OnSpawnedActor @237-311. OnSpawnedActor @372: the actor cast to
//!   C019_CookGrenade (its children included) -> ModifyBlastRadius(UseCookDuration = GetInstigator() == GetOwner(),
//!   UseMaxRadius = its negation) @451-558. Nothing else. Its Instigator is the perk's (the owner for his own perk,
//!   the team mate who took it for SpawnOnAllies' copy). MaxStacks 1: a second copy stacks on the first entry and
//!   keeps that one's Instigator.

use crate::State;
use crate::actors::ActorRef;
use crate::actors::projectile::ProjectileExt;
use crate::combat::buffs::{BuffExt, buff, buff_mut};
use crate::combat::{BuffClass, BuffRef};
use crate::fighter::FighterId;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// `CookState`: Buff_C019_Cook's extension (`{ own }`).
#[derive(State, Debug)]
pub struct Cook {
    /// The perk is the grenade owner's own (no Instigator, or the owner himself).
    pub own: bool,
}

impl Default for Cook {
    /// Read as `ext?.own ?? true`.
    fn default() -> Self {
        Cook { own: true }
    }
}

/// `begin`: `b.ext = { own: !instigator || instigator === f }`.
fn begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<crate::Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Cook(Cook { own: from.is_none_or(|i| i == f) });
    }
}

/// `on`: the afterSpawnedActor signal for a cook grenade (ModifyBlastRadius @0-38: the two variables written as given,
/// CookGrenade keeps the one that is true).
fn on(w: &mut World, _f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::AfterSpawnedActor {
        return false;
    }
    let Some(ActorRef::Projectile(pr)) = a.actor else { return false };
    let own = match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::Cook(c)) => c.own,
        _ => true,
    };
    if let Some(p) = w.actors.projectiles.get_mut(pr)
        && let ProjectileExt::CookGrenade(g) = &mut p.ext
    {
        if own {
            g.use_cook_duration = true;
        } else {
            g.use_max_radius = true;
        }
    }
    false
}

/// This file's registry entries (the three perk buffs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let list = [
        BuffEntry::new(BuffClass::BuffC019Hammer, BuffHooks::default()),
        BuffEntry::new(BuffClass::BuffC019Cleaner, BuffHooks::default()),
        BuffEntry::new(BuffClass::BuffC019Cook, BuffHooks { begin: Some(begin), on: Some(on), ..BuffHooks::default() }),
    ];
    for e in list {
        let n = e.class.registered_name().expect("a registered class");
        r.buffs.enter(n, e)?;
    }
    Ok(())
}
