//! What the stage data's `buffs` and `actors` (`sim.rs`) reach beyond the properties the bind reads: the actor
//! classes the level Blueprints spawn and the buff classes the placed and spawned actors' classes lead to, ported from
//! their bytecode and frozen here per class (the fighter data's twin is `reach.rs` of
//! the kind).
//!
//! Nothing reads bytecode at runtime. `@n` is a statement offset of the class's Blueprint; a name the package holds
//! outside its graphs is cited by the property that holds it. Keys are class names (a placed actor's `cls`, a buff's
//! name); values are class paths, the classes the bind loads. Order matters: the reach follows each list in its order,
//! and the order of reaching numbers the data's objects for the state hash. A class with no entry is an error (no
//! default for an unknown name).

/// What a level Blueprint hands BeginDeferredActorSpawnFromClass as its class: a class literal, or the array member of
/// the level script's class defaults that a function of it picks the class from with Array_Get (each of its entries,
/// in the array's order).
pub enum LevelSpawn {
    Cls(&'static str),
    Member(&'static str),
}

const BASKETBALL: &str = "/Game/Panda_Main/Blueprints/Items/Item_Basketball.Item_Basketball_C";

/// By level script class (the PersistentLevel's LevelScriptActor's class), the classes its graphs spawn, in the order
/// of its bytecode. Every level Blueprint of the built stages' levels (but the audio levels, read apart) is listed;
/// none but The Court's spawns an actor. A level whose LevelScriptActor is the engine's has none.
/// - Map_M010 (The Court): SpawnBall @82 BeginDeferredActorSpawnFromClass(Item_Basketball_C); PowerupSpawner @722
///   Array_Get(PowerupList) into the class @781 spawns (PickupPowerup_Speed / Power / Jump_C, the class defaults').
/// - Map_Batcave_Godrays_VFX, Map_Batcave_EarlyTestGameplay, Map_BatcaveNew_VFX: classes with no function (their
///   packages hold no Function export).
/// - Map_TrophyRoom_Tutorial: BeginPlay (@378 to @281) toggles multikeyboard and creates the TransitionIn widget; it
///   spawns nothing.
pub fn level_spawns(script: &str) -> Option<&'static [LevelSpawn]> {
    const M010: &[LevelSpawn] = &[LevelSpawn::Cls(BASKETBALL), LevelSpawn::Member("PowerupList")];
    const NONE: &[LevelSpawn] = &[];
    Some(match script {
        "Map_M010_C" => M010,
        "Map_M010_VFX_C" | "Map_M010_Animation_C" => NONE,
        "Map_TrainingRoomSmall_C" | "Map_TrainingRoomLarge_C" => NONE,
        "Map_M011_C" | "Map_M011_Lighting_C" | "Map_M011_Background_C" | "Map_M011_Animation_C" => NONE,
        "Map_M011_Small_C"
        | "Map_M011_Lighting_Small_C"
        | "Map_M011_Background_Small_C"
        | "Map_M011_Animation_Small_C" => NONE,
        "Map_M011_LargeNoPlat_C"
        | "Map_M011_Lighting_LargeNoPlat_C"
        | "Map_M011_Background_LargeNoPlat_C"
        | "Map_M011_Animation_LargeNoPlat_C" => NONE,
        "Map_M011_SmallNoPlat_C"
        | "Map_M011_Lighting_SmallNoPlat_C"
        | "Map_M011_Background_SmallNoPlat_C"
        | "Map_M011_Animation_SmallNoPlat_C" => NONE,
        "Map_Classic3Platform_Small_C" | "Map_Classic3Platform_Large_C" => NONE,
        "Map_TrophyRoom_LargePlatform_C" | "Map_TrophyRoom_Lighting_C" | "Map_TrophyRoom_2platforms_C" => NONE,
        "Map_TrophyRoom_Tutorial_C" => NONE,
        "Map_TreeHouse_C" | "Map_TreeHouse_Geo_C" => NONE,
        "Map_TreeHouseSmall_C" | "Map_TreeHouse_1Plat_Large_C" => NONE,
        "Map_M009_C" | "Map_M009_small_C" => NONE,
        "Map_ScoobyDoo_C" | "Map_ScoobyDoo_NoRoof_C" => NONE,
        "Map_Batcave_C" | "Map_Batcave_Godrays_VFX_C" | "Map_Batcave_EarlyTestGameplay_C" | "Map_BatcaveNew_VFX_C" => {
            NONE
        }
        "Map_M008_C" | "Map_M008_Lighting_C" | "Map_M008_Animation_C" => NONE,
        _ => return None,
    })
}

/// A Blueprint class by its package under /Game/Panda_Main (the class named as the package with `_C`), as the object
/// path the bind loads.
fn class(pkg: &str) -> String {
    let name = &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..];
    format!("/Game/Panda_Main/{pkg}.{name}_C")
}

const FOG: &str = "Blueprints/Abilities/Shield/Buff_Invisibility_Fog";
const ELEMENTAL: &str = "Blueprints/Buffs/Elemental/Buff_ElementalParent";
const E_FIRE: &str = "Blueprints/Buffs/Elemental/Buff_Elemental_Fire";
const E_FIREPROOF: &str = "Blueprints/Buffs/Elemental/Buff_Elemental_Fireproof";
const E_ICE: &str = "Blueprints/Buffs/Elemental/Buff_Elemental_Ice";
const E_ICEPROOF: &str = "Blueprints/Buffs/Elemental/Buff_Elemental_Iceproof";
const E_NOARMOR: &str = "Blueprints/Buffs/Elemental/Buff_Elemental_NoArmorDamage";
const FIRE: &str = "Blueprints/Abilities/DamageOverTime/Buff_Fire";
const ICE: &str = "Blueprints/Abilities/Shield/Buff_Ice";

/// By class, the buff classes it leads to (bcreach graph_buffs then named_buffs: the literals its graphs and its
/// Blueprint parents' hand SpawnBuffActor or a Struct_ApplyBuffs' AppliedBuffs, then every other buff class its
/// package names, in the package's order). A placed or spawned actor's class is a root of the reach, a buff reached is
/// walked in turn (buff_reach). A buff's BUFF_SPAWN_MEMBERS (the kind's reach.rs) are read from its class defaults
/// before its entry. GRAPH_BUFFS_SKIP names none of these. `None`: no entry for the class.
pub fn stage_buff_reach(c: &str) -> Option<Vec<String>> {
    let pkgs: &[&str] = match c {
        // ---- the placed actors' classes (sim/stages/placed.json)
        // FogOcclusionBP: SpawnBuffActor(Buff_Invisibility_Fog_C) @3497 / @5319 / @6304.
        "FogOcclusionBP_C" => &[FOG],
        "BatcaveBatsBP_C"
        | "BatcaveWallDestructible_C"
        | "M008_Elevator_BP_C"
        | "M009_DestructiblePlatformBP_C"
        | "M009_DestructiblePlatformBP_Child_C"
        | "ScoobyElevatorSwitch_C"
        | "ScoobyElevator_C"
        | "ScoobyWall_C"
        | "TreehouseSidePillar_C"
        | "TreehouseSidePillar_Child_C"
        | "m010_wall_C" => &[],
        // M010_Hoop_A_01_BP DoHoopExplosion: its local array's element class (Buff_ElementalParent_C), then @941
        // MakeArray([Buff_Elemental_Fire_C]), the ElementalBuffs it hands pfgDoDamage.
        "M010_Hoop_A_01_BP_C" => &[ELEMENTAL, E_FIRE],
        // ---- the level scripts' spawned classes (LEVEL_SPAWNS)
        // Item_Basketball: its class defaults' ElementalVFXOverrides key.
        "Item_Basketball_C" => &[E_FIRE],
        // PickupPowerup_*: @657 SpawnBuffActor(Pickup*Buff_C); its Comp_Pawn_PassiveBuffManager template's Buffs[0].
        "PickupPowerup_Speed_C" => &["Maps/M010/PickupSpeedBuff", E_FIREPROOF],
        "PickupPowerup_Power_C" => &["Maps/M010/PickupPowerBuff", E_FIREPROOF],
        "PickupPowerup_Jump_C" => &["Maps/M010/PickupJumpBuff", E_FIREPROOF],
        // ---- the buffs reached
        "Buff_Invisibility_Fog_C"
        | "Buff_ElementalParent_C"
        | "PickupSpeedBuff_C"
        | "PickupPowerBuff_C"
        | "PickupJumpBuff_C"
        | "Buff_Ice_C"
        | "Buff_Elemental_NoArmorDamage_C" => &[],
        // Buff_Elemental_Fire: SpawnBuffActor @1647 / @9062 / @9296 (Buff_Fire_C), @6784 (Buff_Elemental_Fire_C);
        // named: RemoveBuffStack @2606 (Buff_Ice_C), CheckHasBuff @5843 (Buff_Elemental_Ice_C), @9551
        // (Buff_Elemental_Fireproof_C).
        "Buff_Elemental_Fire_C" => &[FIRE, E_FIRE, ICE, E_ICE, E_FIREPROOF],
        // Buff_Elemental_Ice: SpawnBuffActor @2245 / @8761 (Buff_Ice_C), @7118 (Buff_Elemental_Ice_C); named:
        // RemoveBuffStack @3720 (Buff_Fire_C), CheckHasBuff @6173 (Buff_Elemental_Fire_C), @8974
        // (Buff_Elemental_Iceproof_C).
        "Buff_Elemental_Ice_C" => &[ICE, E_ICE, FIRE, E_FIRE, E_ICEPROOF],
        // Buff_Elemental_Fireproof / Iceproof: their class defaults' StateInfo.BreakBuffs.
        "Buff_Elemental_Fireproof_C" => &[FIRE, E_FIRE],
        "Buff_Elemental_Iceproof_C" => &[ICE, E_ICE],
        // Buff_Fire ExecuteUbergraph: its local array's element class (Buff_ElementalParent_C), then @2451
        // MakeArray([Buff_Elemental_NoArmorDamage_C]).
        "Buff_Fire_C" => &[ELEMENTAL, E_NOARMOR],
        _ => return None,
    };
    Some(pkgs.iter().map(|p| class(p)).collect())
}

/// The class every buff is a child of (a reached class that is not one is an error).
pub const BUFF_PARENT: &str = "BuffParentActor_C";
