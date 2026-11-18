//! What the items part reaches: the classes a game mode's random items lead to beyond the properties the bind reads,
//! ported from their bytecode and frozen here per class, as `reach.rs` does for the
//! fighter data (whose tables the items bind reads too, for the classes a fighter's data shares with the items).
//!
//! Nothing reads bytecode at runtime. `@n` is a statement offset of the class's Blueprint (an inherited one: the
//! parent's). Keys are class names; values are object paths. Order matters: the reach follows each list in its order,
//! and the order of reaching numbers the part's objects for the state hash.

use super::reach::{Reach, c};

/// FFAGameMode: the class default object whose Items StartNewMatch hands to StartTimeSpawnerServer (@2954).
pub const FFA_MODE_CDO: &str = "/Game/Panda_Main/Blueprints/GameModes/FFAGameMode.Default__FFAGameMode_C";

/// The member of [`FFA_MODE_CDO`] that holds the items.
pub const FFA_MODE_MEMBER: &str = "Items";

/// The class literal StartNewMatch hands to SpawnCenterActor (@3054).
pub const FFA_CENTER_ACTOR: &str = c!("Blueprints/Projectiles", "BugsBunnyNeutralSafe");

/// The class default members that hold the classes a class's graph spawns from (each entry, in the array's order):
/// Item_Mystery's three tier pools (SpawnAndEquipItem's class, by the holder's place), Item_CarWheel's CarList (the
/// car it spawns), BugsBunnyNeutralSafe's SpawnableItems (its parent's death loot: `reach.rs` GRAPH_SPAWNS
/// BugsBunnySafe_C names the same list as literals).
pub const ITEM_MEMBERS: Reach = Reach::of(&[
    ("Item_Mystery_C", &["WinnersItems", "MiddleItems", "LastPlaceItems"]),
    ("Item_CarWheel_C", &["CarList"]),
    ("BugsBunnyNeutralSafe_C", &["SpawnableItems"]),
]);

/// The classes a class's graph spawns with a class literal, beyond `reach.rs` GRAPH_SPAWNS.
/// - Item_Safe: Comp_Spawner.CompSpawnActor(BugsBunnySafe_C) @2270 (the throw).
/// - Item_BouncePad: BeginDeferredActorSpawnFromClass(BouncePad_SpawnedActor_C) @150.
/// - Buff_Chicken: the holder's Comp_Spawner.CompSpawnActor(C015_HealPickup_C) @650 (HandleDamageRecieved).
pub const ITEM_SPAWNS: Reach = Reach::of(&[
    ("Item_Safe_C", &[c!("Blueprints/Projectiles", "BugsBunnySafe")]),
    ("Item_BouncePad_C", &[c!("Blueprints/Items/Item_BouncePad", "BouncePad_SpawnedActor")]),
    ("Buff_Chicken_C", &[c!("Characters/C015/Blueprints", "C015_HealPickup")]),
]);

/// The buff classes a class's graph names to SpawnBuffActor by a literal, beyond `reach.rs` GRAPH_BUFFS. The cars:
/// Parent_Car_SpawnedActor's SpawnBuffActor(Car_Buff_C) @1138, for each child the wheel spawns. Buff_Enlarge is
/// FFAGameMode's respawn catch-up buff @2154-2233: SpawnBuffActor(select(Buff_Polymorph_ProjectileEnlarge_WithHieghtChange_C,
/// Buff_Polymorph_ProjectileEnlarge_C)) on a projectile its holder spawns (the fighters' data leaves them out:
/// GRAPH_BUFFS_SKIP).
pub const ITEM_BUFFS: Reach = Reach::of(&[
    // Buff_ArmorPack @81
    ("Item_ArmorToken_C", &[c!("Blueprints/Abilities/Shield", "Buff_ArmorPack")]),
    // Buff_Ice @3058, Buff_Weakened @3156
    (
        "Buff_SnowballHead_C",
        &[c!("Blueprints/Abilities/Shield", "Buff_Ice"), c!("Blueprints/Abilities/Shield", "Buff_Weakened")],
    ),
    ("BatMobile_SpawnedActor_Child_C", &[c!("Blueprints/Items/Item_Cars", "Car_Buff")]),
    ("MysteryMachine_SpawnedCar_C", &[c!("Blueprints/Items/Item_Cars", "Car_Buff")]),
    (
        "Buff_Enlarge_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_Polymorph_ProjectileEnlarge_WithHieghtChange"),
            c!("Blueprints/Abilities/Shield", "Buff_Polymorph_ProjectileEnlarge"),
        ],
    ),
]);
