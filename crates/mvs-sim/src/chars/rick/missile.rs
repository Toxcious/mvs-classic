//! c020_WeaponModeMissile, a plain projectile spawned by C020_Attack_Combo3_Montage.
//!
//! The parent and the data give the flight, the hit box, the packet and the death effect. Its own graph: the particle
//! system made visible 0.033 s after BeginPlay and the colour writes (the drawing's), and HitEnemyPlayer @120: the
//! parent's, then PF_Post_VO_Event(Instigator, "", HitPlayer, 1.0, false, false, self) @148: RandomFloat <= 1.0 (a draw
//! of the random stream) and BPI_PlayVo with an empty alias, which no VO row carries (`fighter::match_side::post_vo`: the
//! draw, then no line). Owner reads: the Instigator's VO (a disguised wearer speaks with his own rows).

use crate::actors::{ActorClass, ProjectileClass, ProjectileId};
use crate::fighter::FighterId;
use crate::fighter::match_side::post_vo;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

fn hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    let Some(o) = w.actors.projectiles.get(id).and_then(|p| p.owner) else { return };
    post_vo(w, o, Name::NONE, Some(vic), 1.0);
}

/// This file's registry entries.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ProjectileHooks { hit_enemy: Some(hit_enemy), ..ProjectileHooks::default() };
    let mut e =
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::C020WeaponModeMissile), ActorHooks::Projectile(hooks));
    e.foreign_owner = true;
    r.actors.enter("c020_WeaponModeMissile_C", e)
}
