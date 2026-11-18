//! Shaggy's perk buffs: `Buff_Shaggy_ExtraHungry_C` (no hooks) and
//! `Buff_Shaggy_DyingRage_C` (its `ext` is [`crate::combat::BuffExt::DyingRage`]).
//!
//! Buff_Shaggy_ExtraHungry (Hangry Man): a BuffParentActor child with no graph of its own. Its effect is read through
//! IsExtraHungryPerk_C (BPI_CheckIfHasBuff of this class): the builder branches Shaggy_(Air_)ExtraHungryRageBuilder_Montage
//! of his special rows, and Comp_Pawn_Rage GetRageThreshold (1.5 with the sandwich held) with the RockMesh unequip at a
//! full meter.
//!
//! Buff_Shaggy_DyingRage (One Last Zoinks). ReceiveBeginPlay @399 -> @10: with authority and the owner's
//! BPI_GetFighterCharacter == 3 (Pawn_Shaggy) @189, bind its Comp_Pawn_DamageManager HitByEnemy to OnHitbyEnemy
//! @254-328. OnHitbyEnemy -> @414: while !Activated, the owner Shaggy again @602, its Comp_Actor_Damage.CurrentDamage >
//! DamageRequiredToActivate (100; strictly) @718 -> Comp_Pawn_Rage.EnableRage(false) @863 (no success montage; the meter
//! is not filled), Activated = true @900, and the HitByEnemy binding removed @1005. No EndPlay or reset of its own.

use crate::actors::ActorRef;
use crate::combat::buffs::BuffExt;
use crate::combat::defense::enable_rage;
use crate::combat::{BuffClass, BuffRef};
use crate::fighter::FighterId;
use crate::fighter::common::buff_def;
use crate::registry::entries::BuffHooks;
use crate::registry::props::{buff, buff_mut, num};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;

/// EFighter 3 is Pawn_Shaggy's BPI_GetFighterCharacter (Pawn_Shaggy @0); no other pawn returns it.
fn is_shaggy(w: &World, f: FighterId) -> bool {
    w.names.str(w.fighter(f).ch) == super::KEY
}

fn dying_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<crate::Real>) {
    let bound = is_shaggy(w, f);
    if let Some(e) = buff_mut(w, b) {
        e.ext = BuffExt::DyingRage { bound, activated: false };
    }
}

fn dying_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    if s != BuffSignal::HitByEnemy {
        return false;
    }
    let Some(e) = buff(w, b) else { return false };
    let BuffExt::DyingRage { bound: true, activated: false } = e.ext else { return false };
    if !is_shaggy(w, f) {
        return false;
    }
    // The table's default; no graph writes it.
    let need = buff_def(w, ActorRef::Fighter(f), e.cls).and_then(|d| num(d, "DamageRequiredToActivate"));
    let Some(need) = need else { return false };
    let over = w.fighter(f).stats.damage > need; // strictly
    if !over {
        return false;
    }
    enable_rage(w, f, false);
    if let Some(e) = buff_mut(w, b) {
        e.ext = BuffExt::DyingRage { bound: false, activated: true };
    }
    false
}

pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs
        .enter("Buff_Shaggy_ExtraHungry_C", BuffEntry::new(BuffClass::BuffShaggyExtraHungry, BuffHooks::default()))?;
    let dying = BuffHooks { begin: Some(dying_begin), on: Some(dying_on), ..BuffHooks::default() };
    r.buffs.enter("Buff_Shaggy_DyingRage_C", BuffEntry::new(BuffClass::BuffShaggyDyingRage, dying))?;
    Ok(())
}
