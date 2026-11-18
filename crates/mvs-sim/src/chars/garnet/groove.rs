//! The perk Electric Groove: Buff_Garnet_ElectricGroove_C ([`Groove`]), which
//! gives sing.rs's Buff_GarnetStackingSinging.
//!
//! Garnet's signature perk Electric Groove (Perk_Garnet_ElectricGroove: AppliedBuffs Buff_Garnet_ElectricGroove_C,
//! SpawnOnAllies true in its PerkData, so her team-mates get one too). Read: Buff_Garnet_ElectricGroove,
//! AccessorLibrary IsSoftRef @0-104, Comp_State_Manager SpawnBuffActor @1427, SweepBuffState @1626-1671.
//! - ReceiveBeginPlay @601-881: binds the OWNER's Comp_State_Manager NewBuffSpawned (@654 -> OnBuffSpawned) and
//!   BuffStacksAdded (@789 -> OnBuffStacksAdded); both call HandleBuffChange with the buff actor (@886, @1078). No
//!   unbind, no authority gate. Those delegates are broadcast on the state manager whose SpawnBuffActor makes the call:
//!   the owner's own OnHitBuffs rows (the trails, the bolt, the zone's explosion) and a held electric gauntlet's hit. Not
//!   a team-mate's hits (the mate's state manager). Buff_Elemental_Electric spawns through the VICTIM's own state
//!   manager (@1254-1325, @2672-2723): it never rolls for the dealer's groove, but a victim carrying the perk rolls on
//!   the shock it receives.
//! - HandleBuffChange @914: IsSoftRef(buff, Buff_Damage_Electric): GetObjectClass == the class, an exact test (its child
//!   Buff_Low_Damage_Electric_C does not pass). Then @10 RandomBoolWithWeight(RhythmChance, 0.25 in the data); false ends
//!   it. @152 the owner's BPI_CheckIfHasBuff(Buff_GarnetStackingSinging) (a failed cast: none, @551-596); @458 the perk
//!   buff's Instigator's Comp_State_Manager SpawnBuffActor(Buff_GarnetStackingSinging, the owner, the Instigator,
//!   SelectInt(1, 2, hasBuff): 1 stack when she has it, 2 when not, 0.0 s, BreakIfBlocked true, SpawnOnAllies false).
// Unverified: RandomBoolWithWeight (FRand() <= weight) is drawn from the world's stream (`World::random_int`, 2^24
// steps), as Bugs's safe; the build's stream and its draw order are not read.
// The buff's Instigator is kept in `ext.from` at its begin (Comp_Pawn_PerkManager: the owner for her own perk, the
// perk's player's pawn for a team-mate's copy).

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::BuffClass;
use crate::combat::buffs::{BuffExt, BuffRef, buff_of, spawn_buff};
use crate::fighter::FighterId;
use crate::fighter::common::buff_def;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::{buff_mut, num};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

const GROOVE: &str = "Buff_Garnet_ElectricGroove_C";
const GROOVE_NAME: Name = name!("Buff_Garnet_ElectricGroove_C");
const ELECTRIC: Name = name!("Buff_Damage_Electric_C");
const STACKING_SINGING: Name = name!("Buff_GarnetStackingSinging_C");

/// Buff_Garnet_ElectricGroove_C's extension (`{ from }`): the buff's Instigator.
#[derive(State, Debug, Default)]
pub struct Groove {
    pub from: Option<FighterId>,
}

fn begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::ElectricGroove(Groove { from });
    }
}

fn on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if !matches!(s, BuffSignal::NewBuffSpawned | BuffSignal::BuffStacksAdded) || a.buff_cls != Some(ELECTRIC) {
        return false;
    }
    let chance = buff_def(w, ActorRef::Fighter(f), GROOVE_NAME).and_then(|d| num(d, "RhythmChance")).unwrap_or(0.0);
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "a NaN chance fails the test")]
    let miss = !(w.random_int(16_777_216.0) / 16_777_216.0 <= chance);
    if miss {
        return false;
    }
    let from = match crate::registry::props::buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::ElectricGroove(g)) => g.from,
        _ => None,
    };
    let inst = from.unwrap_or(f);
    let stacks = if buff_of(w, f, STACKING_SINGING).is_some() { 1 } else { 2 };
    spawn_buff(w, f, STACKING_SINGING, 0.0, Some(stacks), Some(inst), true, Some(inst));
    false
}

/// This file's registry entries (Buff_Garnet_ElectricGroove_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        GROOVE,
        BuffEntry::new(
            BuffClass::BuffGarnetElectricGroove,
            BuffHooks { begin: Some(begin), on: Some(on), ..BuffHooks::default() },
        ),
    )
}
