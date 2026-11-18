//! Black Adam's signature perks (all but its PerkForceField, which is field.rs's):
//! Buff_C021_Shocked (its DealtDamageModCondition: the victim has an electric buff), Buff_C021_Lightning and
//! Buff_C021_Bubble (markers: bolt.rs and the Bubble conditions read them). All three perks are self-only
//! (SpawnOnAllies false).
//!
//! Buff_C021_Shocked DealtDamageModCondition @0-307: the Victim cast to BPI_BaseFighter @0 (failed: false @257); its
//! BPI_CheckIfHasBuff(Buff_Damage_Electric_C) @79 -> ShouldApplyBuffMods @233. The mods are the data's: DamageMod
//! 1.15, Multiply (NewEnumerator2), the rest DoNothing. The dealt pass reads them.
//! Buff_C021_Lightning has no graph of its own (a marker). C021_Bolt's BeginPlay reads it @8560: GetAllies @8794, the
//! first one @8942 becomes the bolt's owner @8997 and the bolt goes to its location + (0, 0, 2000) @9175 (bolt.rs).
//! Buff_C021_Bubble has no graph: C021BubblePerkCondition / C021NoBubblePerkCondition (pawn.rs) choose the down
//! special's spawner.

use crate::actors::ActorRef;
use crate::combat::BuffRef;
use crate::combat::HitPacket;
use crate::combat::buff_class::BuffClass;
use crate::fighter::FighterId;
use crate::name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// CheckIfHasBuff is BuffParentActor IsBuffOfClass (@152-228): the class or a child of it. The port's buffs carry no
/// parent class, so the family is listed from the bytecode dump's SuperStruct chains: Buff_Low_Damage_Electric_C
/// (Wonder Woman's LassoGrapple perk puts it on an enemy) is the one child of Buff_Damage_Electric_C in the build, and
/// it has no child of its own.
fn electrified(w: &World, v: FighterId) -> bool {
    let family = [name!("Buff_Damage_Electric_C"), name!("Buff_Low_Damage_Electric_C")];
    w.fighter(v).buffs.buffs.iter().any(|b| b.frames > 0.0 && family.contains(&b.cls))
}

/// @0-65: the Victim cast to BPI_BaseFighter, else false @257 (a projectile, item or actor victim).
fn shocked_dealt(
    w: &mut World,
    victim: ActorRef,
    _own: FighterId,
    _dealer: FighterId,
    _hit: &HitPacket,
    _b: BuffRef,
    _col: bool,
) -> bool {
    match victim {
        ActorRef::Fighter(v) => electrified(w, v),
        _ => false,
    }
}

/// This file's registry entries (the three perk buffs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let list = [
        BuffEntry::new(
            BuffClass::BuffC021Shocked,
            BuffHooks { dealt_cond: Some(shocked_dealt), ..BuffHooks::default() },
        ),
        BuffEntry::new(BuffClass::BuffC021Lightning, BuffHooks::default()),
        BuffEntry::new(BuffClass::BuffC021Bubble, BuffHooks::default()),
    ];
    for e in list {
        let n = e.class.registered_name().expect("a registered class");
        r.buffs.enter(n, e)?;
    }
    Ok(())
}
