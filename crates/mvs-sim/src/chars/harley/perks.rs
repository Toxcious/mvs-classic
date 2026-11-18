//! Harley Quinn's perk classes: Buff_Harley_Bullseye, Buff_Harley_ConfettiExplosion and
//! Buff_Harley_SmoothMoves, entered empty (their presence is what the conditions and actors.rs's confetti read).
//!
//! The three buff classes (Blueprints/Perks/Buffs/Harley) have no function of their own: BuffParentActor's defaults
//! (duration 5, one stack, every mod neutral) and nothing else. Each is a marker that a graph of her kit asks for:
//! - Buff_Harley_Bullseye_C: HarleyHasBullsEyePerk / HarleyHasNoBullsEyePerk (pawn.rs) pick her air down attack,
//!   HarleyQuinn_Attack_Air_Bullseye_Montage (the aimed glove) over HarleyQuinn_Attack_Air_GloveShot_Montage.
//! - Buff_Harley_ConfettiExplosion_C: Buff_ApplyConfetti's CheckHasBuff on its Instigator (actors.rs) picks
//!   Buff_ConfettiExplosionCount_C over Buff_ConfettiCount_C.
//! - Buff_Harley_SmoothMoves_C: HasSmoothMovesEquipped (`combat::conditions`) picks
//!   HarleyQuinn_Special_SmoothMoves(_Air)_Montage for her side special; that montage's own notifies
//!   (BPI_EvadeFromAttack at f0) do the rest.
//!
//! The perk system gives each with duration 0 (no life timer) at the pawn's first possession and at each reset; the
//! defeat tail destroys it (DestroyBuffWhenOwnerDefeated) until then.

use crate::combat::BuffClass;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::{Registry, RegistryError};

/// This file's registry entries (the three perk buffs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let list = [
        BuffEntry::new(BuffClass::BuffHarleyBullseye, BuffHooks::default()),
        BuffEntry::new(BuffClass::BuffHarleyConfettiExplosion, BuffHooks::default()),
        BuffEntry::new(BuffClass::BuffHarleySmoothMoves, BuffHooks::default()),
    ];
    for e in list {
        let n = e.class.registered_name().expect("a registered class");
        r.buffs.enter(n, e)?;
    }
    Ok(())
}
