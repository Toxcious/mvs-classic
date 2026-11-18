//! The mutators' data: MutatorData_Map's MutatorMap (MutatorBuffName -> the
//! mutator's buff class, read by PandaGameMode InitializeMutators @577), every class of the map with its class
//! defaults, and the buffs the mutators' graphs give (Mutator_GrowWithDamage @3090, Mutator_Boost_WeakenOnHit @104,
//! Mutator_Boost_ChargeFreeze @353, Mutator_Boost_IceGauntlets @91), each bound as the fighter data's buffs are
//! (ActorClasses::class_defaults).

use mvs_data::fighter::Map;
use mvs_data::{MutatorsData, Value};

use super::actor_class::ActorClasses;
use super::reach::c;
use super::shape::short_name;
use crate::kind::{Cx, KindError};

const MAP: &str = c!("Blueprints/Buffs/Mutators", "MutatorData_Map");
/// The world mutators' folder: the classes the port takes as IsWorldMutator (MutatorsData.world).
const WORLD_FOLDER: &str = "/Game/Panda_Main/Blueprints/Buffs/Mutators/MutatorBuffs/WorldMutatorBuffs/";
/// The buffs the mutators' graphs spawn on a fighter or a spawned actor.
pub const MUTATOR_GIVEN_BUFFS: [&str; 4] = [
    c!("Blueprints/Abilities/Shield", "Buff_GrowWithDamageProjectile"),
    c!("Blueprints/Abilities/Shield", "Buff_Weakened"),
    c!("Blueprints/Abilities/Shield", "Buff_Ice"),
    c!("Blueprints/Abilities/Shield/BL_Superman", "Buff_SupermanAllyIceGauntlet"),
];

/// The mutators' data. A map entry whose class has no asset
/// (Mutator_Boost_FastCharge) is left out, as LoadClassAsset_Blocking's failed cast skips it (InitializeMutators
/// @956).
pub fn mutators(cx: &Cx<'_>) -> Result<MutatorsData, KindError> {
    let classes = ActorClasses::new(*cx, "mutators");
    // (a TMap as [key, value] pairs, a soft class path as its string)
    let raw = classes.raw_defaults(MAP)?;
    let mut out = MutatorsData { map: Map::new(), world: Vec::new(), buffs: Map::new() };
    for pair in raw.get("MutatorMap").and_then(Value::as_arr).unwrap_or_default() {
        let (key, path) = match pair.as_arr() {
            Some([Value::Str(k), Value::Str(p)]) => (k, p.as_str()),
            Some([Value::Str(k), _]) => (k, ""),
            _ => continue,
        };
        if path.is_empty() {
            continue;
        }
        let cls = short_name(path).to_owned();
        if !out.buffs.contains_key(&cls) {
            // (no asset in the build: left out)
            let Ok(d) = classes.class_defaults(path) else { continue };
            out.buffs.insert(cls.clone(), d);
        }
        out.map.insert(key.clone(), cls.clone());
        if path.starts_with(WORLD_FOLDER) && !out.world.contains(&cls) {
            out.world.push(cls);
        }
    }
    for p in MUTATOR_GIVEN_BUFFS {
        let cls = short_name(p).to_owned();
        if !out.buffs.contains_key(&cls) {
            let d = classes.class_defaults(p)?;
            out.buffs.insert(cls, d);
        }
    }
    Ok(out)
}
