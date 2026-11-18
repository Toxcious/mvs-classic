//! Batman's signature perks: Bouncerang, Chimney, BombDownSpike and
//! IncreasedBatarangControl.
//!
//! The three buff classes (Blueprints/Perks/Buffs/Batman) have no function of their own: BuffParentActor's defaults
//! and nothing else. Each is a marker a graph of his kit asks for with CheckHasBuff:
//! - Buff_BatmanBouncerang_C: Item_BatarangV2 ReceiveBeginPlay @1880 -> IsBouncerang @2049; its StartReturn
//!   @828-1094 makes the returning batarang's OnHitBuffs one Buff_Weakened row (`batarang`). The buff's MaxBounces is
//!   read by no graph.
//! - Buff_Batman_Chimney_C: SmokeGrenadeOcclusionBP ReceiveBeginPlay @3715 on the Instigator -> ChimneyPerkActive
//!   (`actors::smoke_bomb`).
//! - Buff_BatmanBombDownSpike_C: Buff_DelayKnockUp ReceiveBeginPlay @806 on the Instigator -> ReveresedDirection
//!   (`bomb`).
//!
//! Perk_Batman_IncreasedBatarangControl has no AppliedBuffs, and its fields are read by no graph: it does nothing in
//! this build.

use crate::combat::BuffClass;
use crate::registry::entries::BuffHooks;
use crate::registry::{BuffEntry, Registry, RegistryError};

/// The three marker buffs: entries with no hooks.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for (name, class) in [
        ("Buff_BatmanBouncerang_C", BuffClass::BuffBatmanBouncerang),
        ("Buff_Batman_Chimney_C", BuffClass::BuffBatmanChimney),
        ("Buff_BatmanBombDownSpike_C", BuffClass::BuffBatmanBombDownSpike),
    ] {
        r.buffs.enter(name, BuffEntry::new(class, BuffHooks::default()))?;
    }
    Ok(())
}
