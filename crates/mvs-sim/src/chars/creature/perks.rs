//! Reindog's FireStorm and AirSupport perks: Buff_Creature_AirSupport (a marker the
//! seismic reactor reads), Buff_Creature_FireStorm (SetMaxFireWalls at its begin; its afterSpawnedActor signal sets a
//! Firebolt's ActorClassSpawnedAtDeath and its hit; its state is `BuffExt::FireStorm`), FireStormHazard_C (the
//! fire wall's kind).
//!
//! Class defaults come from the creature perk part of the data (buffs; FireStormHazard_C's whole class data in the
//! perk reach's part actors, which `actor_def` falls back to). Read: Buff_Creature_Firestorm, FireStormHazard
//! (offsets of their ubergraphs).

use super::fire::FIREBOLT;
use crate::actors::classes::ProjectileClass;
use crate::actors::firewall::{entry as fire_wall_entry, set_max_fire_walls};
use crate::actors::projectile::set_instance_vars;
use crate::actors::{ActorClass, ActorRef};
use crate::combat::buffs::{BuffExt, buff, buff_mut, inst_view};
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::fighter::FighterId;
use crate::fighter::common::{actor_is_a, warn_once};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Value;

/// `FIRE_STORM_HAZARD`.
pub const FIRE_STORM_HAZARD: Name = name!("FireStormHazard_C");

// ---- Buff_Creature_AirSupport (Perk_Creature_AirSupport)
// The buff class has no graph of its own: SeismicReactor's BeginPlay asks its owner for it (BPI_CheckIfHasBuff
// @14042) and sways toward him while it flies (`seismic.rs`). A marker.

// ---- Buff_Creature_FireStorm (Perk_Creature_FireStorm)
// ReceiveBeginPlay @10: GetOwner().GetInstigator() is the fighter himself (APawn::PreInitializeComponents sets a
// pawn's null Instigator to itself, and no fighter spawn passes one: native), so GetComponentByClass(
// Comp_Pawn_FireWall) is valid on Reindog @162 -> SetMaxFireWalls(MaxFirewalls 1) @289 (`set_max_fire_walls`; his
// component's own default is 1 too), then the owner's Comp_Spawner AfterSpawnedActor bound @334-428 (the
// AfterSpawnedActor signal of his spawns). A fighter without the component skips both (@162 -> @643).
// AfterSpawnedActor @474: the actor cast to Firebolt_C (Firebolt_Child_C, its child, too) @474-539 ->
// ActorClassSpawnedAtDeath = FireStormHazard_C @553 and HitBoxDamage = FireballDamage @594 (damage 4, BKB 1100,
// scaling 14, pause 0.05, OnHitAudio sfx_cha_reindog_firewall_hit). The signal comes behind the firebolt's
// BeginPlay and ahead of its first hit test (the world's spawn loop), as the build's broadcast @12116 comes ahead of
// the 2D hit boxes. Both are read at use: the gate's and the death roll's class (`fire.rs`, the spawn at death) from
// the actor's p, the hit from the projectile's `hit`, which the constructor copied from p and is set here with it.

/// `StormState`: `BuffExt::FireStorm { bound }`, the bind @428 reached.
fn storm_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<crate::Real>) {
    let max = inst_view(w, b).num("MaxFirewalls");
    if max.is_none() {
        warn_once(w, name!("Buff_Creature_FireStorm_C data missing: MaxFirewalls"));
    }
    let bound = set_max_fire_walls(w, f, max.map_or(0, to_int32));
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::FireStorm { bound };
    }
}

fn storm_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::AfterSpawnedActor {
        return false;
    }
    if !matches!(buff(w, b).map(|x| &x.ext), Some(BuffExt::FireStorm { bound: true })) {
        return false;
    }
    let Some(ActorRef::Projectile(pr)) = a.actor else { return false };
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    if actor_is_a(w, ActorRef::Fighter(f), p.cls, FIREBOLT) != Some(true) {
        return false;
    }
    let Some(hit) = inst_view(w, b).obj("FireballDamage").cloned() else {
        warn_once(w, name!("Buff_Creature_FireStorm_C data missing: FireballDamage"));
        return false;
    };
    let packet = HitPacket::from_props(&w.names, &hit, None);
    set_instance_vars(
        w,
        pr,
        vec![
            ("ActorClassSpawnedAtDeath".to_string(), Value::Str("FireStormHazard_C".to_string())),
            ("HitBoxDamage".to_string(), Value::Obj(hit)),
        ],
    );
    if let Some(p) = w.actors.projectiles.get_mut(pr) {
        p.hit = packet;
    }
    false
}

// ---- FireStormHazard_C
// A FireWallHazard child (`actors::firewall`) with its own class data: Box extent (300, 300, 300) for 200 (the wall's
// make reads it), the rest inherited (lifetime 8, the passive fire rows, FireWallOn and so MaxFirewalls, Die With
// Instigator). ReceiveBeginPlay @154: the parent's first; then, off a dedicated server, Delay(0.25) @224 -> @15: VFX
// Growth Timeline.PlayFromStart (0.25 s, linear 0 -> 1) -> @49 ParticleSystem.SetRelativeScale3D(VLerp(1.25, 1.875,
// alpha)) @112: the drawing side's (from the actor's age). No other function of its own.

/// This file's registry entries (the two perk buffs, the hazard's class).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        BuffClass::BuffCreatureAirSupport.registered_name().expect("a registered class"),
        BuffEntry::new(BuffClass::BuffCreatureAirSupport, BuffHooks::default()),
    )?;
    let storm = BuffHooks { begin: Some(storm_begin), on: Some(storm_on), ..BuffHooks::default() };
    r.buffs.enter(
        BuffClass::BuffCreatureFireStorm.registered_name().expect("a registered class"),
        BuffEntry::new(BuffClass::BuffCreatureFireStorm, storm),
    )?;
    r.actors.enter(
        "FireStormHazard_C",
        ActorEntry { class: ActorClass::Projectile(ProjectileClass::FireStormHazard), ..fire_wall_entry() },
    )?;
    Ok(())
}
