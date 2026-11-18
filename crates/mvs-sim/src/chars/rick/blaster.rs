//! Rick's blaster shots: C020_BlasterProjectileWeak and
//! C020_BlasterProjectileCharged (plain projectiles).
//!
//! C020_BlasterProjectileParent and its two children have no graph of their own beyond the parent's presentation: the
//! flight, the lifetime, the hit box, the packets and the ability slot 5 are class data and `ParentProjectilePhysics`.
//! What the graph adds is the muzzle flash (the view's: an emitter attached to the owner's mesh), and at
//! ReceiveEndPlay @850 / OnReleasedToPool @1159 -> @869 SpawnEmitterAtLocation(PS_Blaster_Impact, the location) with Team,
//! then PF_Post_Event_At_Location(EndSFX) @1072 (Weak sfx_cha_C020_blaster_hit, Charged sfx_cha_C020_blaster_charged_hit).
//! PS_Blaster_Impact's LOD0 emitters are all disabled (not drawn in the build either). The class is not pooled (no
//! CanBePooled), so only the EndPlay path runs. MulticastReflect @1174 -> @40 is the parent's, then
//! ParticleSystem.Activate(true): the drawing's. Owner reads: the Instigator's id for the sound's fighter.

use crate::actors::projectile::class_p;
use crate::actors::{ActorClass, ProjectileClass, ProjectileId};
use crate::events::Event;
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, ReflectHooks};
use crate::registry::props::name_of;
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// `blasterEnd`: the impact emitter at the shot's place, then its EndSFX there.
fn blaster_end(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let (cls, fighter, x, y) = (p.cls, p.owner_id, p.x, p.y);
    let ev = name_of(w, class_p(w, p), "EndSFX");
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(name!("PS_Blaster_Impact")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    });
    if let Some(ev) = ev.filter(|e| !e.is_none()) {
        w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    }
}

/// The reflect's multicast: the parent's, then the particle system's activation (the drawing's).
fn reflect_multicast(_w: &mut World, _a: crate::actors::ActorRef, _by: crate::fighter::FighterId, _pause: bool) {}

/// This file's registry entries (the two shots).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = || ProjectileHooks {
        end: Some(blaster_end),
        reflect: ReflectHooks { multicast: Some(reflect_multicast), ..ReflectHooks::default() },
        ..ProjectileHooks::default()
    };
    let entry = |class: ProjectileClass| {
        let mut e = ActorEntry::new(ActorClass::Projectile(class), ActorHooks::Projectile(hooks()));
        e.foreign_owner = true;
        e
    };
    r.actors.enter("C020_BlasterProjectileWeak_C", entry(ProjectileClass::C020BlasterProjectileWeak))?;
    r.actors.enter("C020_BlasterProjectileCharged_C", entry(ProjectileClass::C020BlasterProjectileCharged))
}
