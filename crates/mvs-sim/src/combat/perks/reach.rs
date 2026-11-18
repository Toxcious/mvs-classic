//! What the perk tables reach beyond the perks' own AppliedBuffs: the classes a reached
//! class's graphs spawn or name, ported from their bytecode (`@n` is a statement offset of the class's decoded
//! graph), and the class default properties that name more buff classes (read from the build at import: the import
//! takes the classes they hold). Full class paths: a short name gives no package. Nothing reads bytecode at run
//! time; a class whose graph names a buff that is not here has no def in the tables.
//!
//! Order matters: a reached class's own entries are followed in this order (its graph's, then its defaults'), and
//! the order of reaching is the order of the tables' buffs, which numbers their objects for the state hash.

/// A Blueprint class under `/Game/Panda_Main/`: `<folder>/<asset>.<class>_C`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassPath {
    pub folder: &'static str,
    pub asset: &'static str,
    pub class: &'static str,
}

impl ClassPath {
    /// The full object path, `/Game/Panda_Main/<folder>/<asset>.<class>_C`.
    pub fn path(&self) -> String {
        format!("/Game/Panda_Main/{}/{}.{}_C", self.folder, self.asset, self.class)
    }
}

/// A class whose asset is named as the class.
const fn c(folder: &'static str, name: &'static str) -> ClassPath {
    ClassPath { folder, asset: name, class: name }
}

macro_rules! perk {
    ($f:literal) => {
        concat!("Blueprints/Perks/Buffs/", $f)
    };
}

const ELEMENTAL: &str = "Blueprints/Buffs/Elemental";
const SHIELD: &str = "Blueprints/Abilities/Shield";

const ELEMENTAL_PARENT: ClassPath = c(ELEMENTAL, "Buff_ElementalParent");
const NO_ARMOR_DAMAGE: ClassPath = c(ELEMENTAL, "Buff_Elemental_NoArmorDamage");
const DAMAGE_ELECTRIC: ClassPath = c(ELEMENTAL, "Buff_Damage_Electric");
const ELEMENTAL_ELECTRIC: ClassPath = c(ELEMENTAL, "Buff_Elemental_Electric");
const SHIELD_ELECTRIC: ClassPath = c(ELEMENTAL, "Buff_Shield_Electric");
const ELEMENTAL_FIRE: ClassPath = c(ELEMENTAL, "Buff_Elemental_Fire");
const ELEMENTAL_ICE: ClassPath = c(ELEMENTAL, "Buff_Elemental_Ice");
const FIREPROOF: ClassPath = c(ELEMENTAL, "Buff_Elemental_Fireproof");
const ICEPROOF: ClassPath = c(ELEMENTAL, "Buff_Elemental_Iceproof");
const FIRE: ClassPath = c("Blueprints/Abilities/DamageOverTime", "Buff_Fire");
const ICE: ClassPath = c(SHIELD, "Buff_Ice");
const FIRE_GAUNTLETS: ClassPath = c(SHIELD, "Buff_FireGauntlets");
const EVIDENCE_COUNT: ClassPath = c(SHIELD, "Buff_EvidenceCount");
const IGNITED_PASS: ClassPath = c("Characters/c016/Blueprints", "Buff_IgnitedBasketballPass");
const COLLATERAL_TRACKER: ClassPath = c(perk!("Bruiser"), "Buff_CollateralDamageTracker");
const DANCE_COUNT: ClassPath = c("Characters/C023A/BluePrints", "C023A_DanceCount_Debuff");
const FORCE_DANCE: ClassPath = c("Characters/C023A/BluePrints", "C023A_ForceDance_Debuff");
const TO_MELEE_FIRE: ClassPath = c(perk!("Ranged"), "Buff_ProjectileToMeleeFire");
const FIRE_WALL: ClassPath = c("Blueprints/Projectiles", "FireWallHazard");

/// By class: the classes its graphs (its own and its Blueprint parents') spawn or name as a class literal, in the
/// order the tables reach them. "spawn": a SpawnBuffActor's class; "names": a literal that only names the class (a
/// CheckIfHasBuff, a cast, a struct's member type), which the tables take all the same, so the sim finds its def.
/// "the struct": an empty Struct_ElementalBuffs (pfgDoDamage's / HitBoxesOn's argument) whose ElementalBuffs member
/// is typed TSubclassOf<Buff_ElementalParent>.
pub const GRAPH_REACH: &[(ClassPath, &[ClassPath])] = &[
    (c(perk!("Arya"), "Buff_Arya_Trophy"), &[c(SHIELD, "Buff_Disguise")]), // spawn @2274
    (c(perk!("C015"), "Buff_C015_TeamFightCloud"), &[c("Blueprints/Buffs/BL_C015", "Buff_C015AttachToDogpile")]), // spawn @8593
    (c(perk!("C016"), "Buff_C016_DistanceShot"), &[ELEMENTAL_PARENT]), // the struct @699
    // (no graph of its own names it: BasketBallProjectile @7215 CheckHasBuff(Buff_C016_HotHands), @7404 / @7531
    // picks Buff_IgnitedBasketballPass for its owner's pass)
    (c(perk!("C016"), "Buff_C016_HotHands"), &[IGNITED_PASS]),
    (c(perk!("C018"), "Buff_C018_Redirect"), &[c("Characters/C018/Blueprints", "Buff_C018_PerkProjectileSpeed")]), // spawn @561
    (c(perk!("C020"), "Buff_C020_Meeseeks"), &[c("Characters/C020/Blueprints", "Buff_MeeseeksDamageWithHitstun")]), // spawn @195
    // spawn @745, @1275, @1745
    (c(perk!("C020"), "Buff_C020_Portal"), &[FIRE, ELEMENTAL_FIRE, FIRE_GAUNTLETS]),
    (c(perk!("C021"), "Buff_C021_Shocked"), &[DAMAGE_ELECTRIC]), // names @79 (BPI_CheckIfHasBuff)
    (c(perk!("C023A"), "Buff_C023A_Backpack"), &[c("Characters/C023A/BluePrints", "C023A_Buff_Perk_ChargeSpeed")]), // spawn @913
    (c(perk!("C023A"), "Buff_C023A_Dance"), &[DANCE_COUNT]), // @677 into AppliedBuffs
    (c(perk!("C023B"), "Buff_C023B_Leap"), &[c("Characters/C023B/BluePrints", "C023B_DashProjectileBlock")]), // names @736 (a cast)
    (c(perk!("C023B"), "Buff_C023B_PerkAttach"), &[c(SHIELD, "Buff_TransferDebuffs")]), // spawn @195
    (COLLATERAL_TRACKER, &[ELEMENTAL_PARENT]),                                          // the struct @332
    (c(perk!("Bruiser"), "Buff_CollateralDamage"), &[COLLATERAL_TRACKER]),              // spawn @473
    (DAMAGE_ELECTRIC, &[ELEMENTAL_PARENT, NO_ARMOR_DAMAGE]), // the struct @1408 (MakeStruct), names @1388
    (c(perk!("Ranged"), "Buff_DetonateFromMelee"), &[FIRE]), // spawn @176
    (c(perk!("Tank"), "Buff_DodgeReflect"), &[ELEMENTAL_PARENT]), // the struct @1115
    // spawn @1325 / @2723, spawn @2508; names @1082 / @1728
    (ELEMENTAL_ELECTRIC, &[DAMAGE_ELECTRIC, ELEMENTAL_ELECTRIC, SHIELD_ELECTRIC]),
    // spawn @1647 / @9062, spawn @6784; names @2606, @5843, @9551 / @744
    (ELEMENTAL_FIRE, &[FIRE, ELEMENTAL_FIRE, ICE, ELEMENTAL_ICE, FIREPROOF]),
    // spawn @2245 / @8761, spawn @7118; names @3720, @6173, @8974 / @507
    (ELEMENTAL_ICE, &[ICE, ELEMENTAL_ICE, FIRE, ELEMENTAL_FIRE, ICEPROOF]),
    (EVIDENCE_COUNT, &[c("Characters/Velma/BluePrints", "Buff_PoliceEvidence")]), // spawn @1473
    (FIRE, &[ELEMENTAL_PARENT, NO_ARMOR_DAMAGE]), // the struct @2471 / @2527, names @2451
    // spawn @458 (names @152), names @914
    (
        c(perk!("Garnet"), "Buff_Garnet_ElectricGroove"),
        &[c("Characters/Garnet/Blueprints", "Buff_GarnetStackingSinging"), DAMAGE_ELECTRIC],
    ),
    (c(perk!("Ranged"), "Buff_IceProjectile"), &[ICE]), // names @143 (BPI_CheckIfHasBuff)
    (IGNITED_PASS, &[ELEMENTAL_FIRE]),                  // spawn @551 / @635
    (c(perk!("Jake/FloatyBounce"), "Buff_Jake_FloatyBounce"), &[c(SHIELD, "Buff_Float")]), // spawn @15
    (c(perk!("Jake/SafetyNet"), "Buff_Jake_SafetyNet"), &[c(SHIELD, "Buff_AirSuperiority")]), // spawn @542
    (
        c(perk!("Jake/WeaponMaster"), "Buff_Jake_WeaponMaster"),
        &[c("Blueprints/Abilities/Shield/BL_Jake", "Buff_Jake_WeaponMasterAlly")],
    ), // spawn @2504
    (c(perk!("Bruiser"), "Buff_LastStand"), &[ELEMENTAL_PARENT]), // the struct, in OnHitByEnemy's hit info @303
    (c(perk!("Tank"), "Buff_OnKnockbackProjectileBlock"), &[c(SHIELD, "Buff_ProjectileBlock")]), // spawn @106
    (c(perk!("WonderWoman"), "Buff_Perk_WonderWoman_FastFallSlam"), &[ELEMENTAL_PARENT]), // the struct @1196 (HitBoxesOn)
    (c(perk!("Ranged"), "Buff_ProjectileApplyBuffAfterGrounded"), &[ELEMENTAL_ELECTRIC]), // spawn @366
    (SHIELD_ELECTRIC, &[ELEMENTAL_PARENT]),                                               // the struct @1904
    (c(perk!("Velma"), "Buff_Studied"), &[EVIDENCE_COUNT]),                               // names @495, @706 (a cast)
    (c(perk!("Superman/IceBreaker"), "Buff_Superman_IceBreaker"), &[ICE]),                // names @224
    (DANCE_COUNT, &[FORCE_DANCE]),                                                        // spawn @161
];

/// By class: the members of its class defaults (a dotted path into a struct) that hold buff classes, which the
/// tables take after its graph's (the build's own values, read at import). StateInfo.BreakBuffs: the buffs whose
/// arrival ends this one (BuffParentActor's state rules).
pub const DEFAULTS_REACH: &[(ClassPath, &[&str])] = &[
    (FIREPROOF, &["StateInfo.BreakBuffs"]),
    (ICEPROOF, &["StateInfo.BreakBuffs"]),
    // Buff_SupermanAllyIceGauntlet's graph spawns BuffToApplyEnemy on the enemy its holder hits (its child's value)
    (FIRE_GAUNTLETS, &["BuffToApplyEnemy"]),
    // its parent's graph spawns Buff (Buff_DetonateFromMelee) on a melee hit
    (TO_MELEE_FIRE, &["Buff"]),
    (FORCE_DANCE, &["StateInfo.BreakBuffs"]),
];

/// Actors a character perk's buff spawns by its own graph that the character's data does not hold, by the buff
/// class: their whole class data goes into the part that buff lands in (`perk_part_actor`), and the buffs their
/// Comp_Pawn_PassiveBuffManager names join that part's buffs.
pub const PART_ACTORS: &[(ClassPath, &[ClassPath])] = &[
    // Buff_Superman_FireSlam SpawnFireWall @387 CompSpawnActor(FireWallHazard_C)
    (c(perk!("Superman/FireSlam"), "Buff_Superman_FireSlam"), &[FIRE_WALL]),
    // Buff_Perk_WonderWoman_ProjectileBlock OnEvadeTriggered @948 CompSpawnActor(WW_EvadeProjectileBlock_C)
    (
        c(perk!("WonderWoman"), "Buff_Perk_WonderWoman_ProjectileBlock"),
        &[c("Blueprints/Projectiles", "WW_EvadeProjectileBlock")],
    ),
    // Buff_C017_ScorchedEarth SpawnFirewall @1840 CompSpawnActor(FireWallHazard_C)
    (c(perk!("C017"), "Buff_C017_ScorchedEarth"), &[FIRE_WALL]),
    // Buff_Creature_FireStorm @553 sets a Firebolt's ActorClassSpawnedAtDeath = FireStormHazard_C
    (
        ClassPath {
            folder: perk!("Creature/Firestorm"),
            asset: "BUff_Creature_Firestorm",
            class: "Buff_Creature_FireStorm",
        },
        &[c("Blueprints/Projectiles", "FireStormHazard")],
    ),
    // Buff_Creature_WreckingBall spawns Creature_WreckingBall_C
    (
        c(perk!("Creature/WreckingBall"), "Buff_Creature_WreckingBall"),
        &[c("Blueprints/Projectiles", "Creature_WreckingBall")],
    ),
    // Buff_TunnelTeleport's Shockwave branch @1708 CompSpawnActor(TunnelShockwave_C) under Buff_BugsBunny_Shockwave
    (
        c(perk!("BugsBunny/Shockwave"), "Buff_BugsBunny_Shockwave"),
        &[c("Blueprints/Projectiles/BugsBunny", "TunnelShockwave")],
    ),
    // Buff_C015_EatProjectiles' anvil
    (c(perk!("C015"), "Buff_C015_EatProjectiles"), &[c("Characters/C015/Blueprints", "C015Anvil")]),
];

/// Actors a general perk's graph spawns that no character's data holds: their class defaults go into the general
/// table's actors. BlastBoxPlayerExit CheckForBlastBoxBounce (Buff_KnockOutBounce's).
pub const GENERAL_ACTORS: &[ClassPath] = &[c("Blueprints/Projectiles", "BlastBoxExitKnockbackExplosion")];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_the_tables_keys() {
        assert_eq!(
            GRAPH_REACH[0].0.path(),
            "/Game/Panda_Main/Blueprints/Perks/Buffs/Arya/Buff_Arya_Trophy.Buff_Arya_Trophy_C"
        );
        assert_eq!(
            PART_ACTORS[3].0.path(),
            "/Game/Panda_Main/Blueprints/Perks/Buffs/Creature/Firestorm/BUff_Creature_Firestorm.Buff_Creature_FireStorm_C"
        );
        assert_eq!(GRAPH_REACH.len(), 36);
        assert_eq!(DEFAULTS_REACH.len(), 5);
        assert_eq!(PART_ACTORS.len(), 7);
    }
}
