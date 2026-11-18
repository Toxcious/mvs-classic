//! The owner tables: who can draw a system that its folder or the fighters' data does not tell. A new owner is
//! entered here. A system key is its name (the build path's last part), a component's its key (the path below
//! `/Game`, no export, `#local` / `#spline`).
//!
//! Owners (the port's own: a match holds a system's templates, materials, textures and meshes only while one of its
//! owners is in it): `base` is every match's. Two owners are screens, not fighters (`menu`, `intro`); `ffa`, `items`,
//! `grow` are game modes; `snack` is a team; `arcade` the buffs an arcade round gives its bots; a perk's id owns what
//! that perk's buffs show. Every other system is owned by the fighter whose folder it is in, by each fighter whose
//! data names it, and by the fighters of [`SPAWNED_FOR`].
//! A system without an owner stops the build.

/// The systems every match can draw: what the shared classes spawn for any fighter (the hit boxes' defaults, the
/// game mode, the armor component, the knockback influence visual, Comp_State_Manager's haste).
pub const BASE_SYSTEMS: &[&str] = &[
    "PS_HitImpact_TEMPLATE",
    "PS_HitImpact_TEMPLATE_Grayscale",
    "PS_HitImpact_Directional_01",
    "PS_HitBounceImpact",
    "PS_KO_Default",
    "PS_CharacterSmokeTrail",
    "PS_EndgameLosers",
    "PS_Invulnerability",
    "PS_Heal",
    "PS_Buff_Haste_Speed",
    "PS_DustUp",
    "PS_Dust_DoubleJump",
    "PS_JumpLand",
    "PS_FastFall",
    "PS_WallTrail",
    "PS_DustWallGrab",
    "PS_RunTrail",
    "PS_EvadeConfirm",
    "PS_Reflect",
    "PS_PlayerRibbon_DI",
    "PS_ArmorImpact1",
    "PS_ArmorBreak01",
];

/// A screen or game mode that owns a system: `menu` the preview actor's spawn effect (the main menu and the podium),
/// `intro` the reveal of PlayIntroAnimation, `ffa` the catch-up buffs of a Free For All respawn.
pub const SCREEN_SYSTEMS: &[(&str, &[&str])] = &[
    ("PS_CharacterSelectSwirl04", &["menu"]),
    ("PS_CharacterSelectLightBeam", &["menu"]),
    ("PS_MainMenuGlowLightning", &["menu"]),
    ("PS_MainMenuLensFlare", &["menu"]),
    ("PS_CharacterSelectSwirl", &["intro"]),
    ("PS_ShadowClone_Impact", &["ffa"]),
];

/// Systems of another folder that the port spawns by name for a fighter's actors or components and that no data file
/// names: the fighters (or `items`, `snack`, `grow`, `ffa`) that can show them.
pub const SPAWNED_FOR: &[(&str, &[&str])] = &[
    // Tutorial_Meeseeks's On Hit SpawnEmitterAtLocation and its ParticleSystem_JetsLeft / Right components: the tutorial
    // draws its Meeseeks with Rick's assets (the tutorial cast loads Rick).
    ("PS_MeeseeksDestroyed", &["rick"]),
    ("PS_JetPackFlame_Looping", &["rick"]),
    ("PS_ItemSpawn", &["items"]),
    ("PS_BugsBunnyRocketImpact", &["items"]),
    ("PS_BruteRageAura_Hold", &["shaggy"]),
    ("PS_CreatureCrystal_Activate", &["morty"]),
    ("PS_Garnet_ProjectileSlowTemp", &["morty"]),
    ("PS_Debuff_Fire_FOE", &["morty", "irongiant", "harley", "superman", "lebron", "creature", "stripe"]),
    ("PS_TomJerryRocket_Explosion", &["stripe", "items"]),
    ("PS_VDay_Petals", &["batman", "garnet", "shaggy", "superman", "velma", "wonderwoman"]),
    ("PS_TomJerryPoof00", &["irongiant"]),
    ("PS_TetherPoints", &["wonderwoman", "steven"]),
    ("PS_HitImpact_Directional", &["wonderwoman", "harley", "lebron", "blackadam", "steven", "velma", "gizmo"]),
    ("PS_Explosion_Satchel", &["harley"]),
    ("PS_ShaggyRageImpact", &["superman"]),
    ("PS_SupermanSkillBounds", &["superman"]),
    ("PS_SharpenBuff", &["arya"]),
    ("PS_SharpenBuff_Loop", &["garnet"]),
    ("PS_BuffTurnAround", &["arya"]),
    ("PS_ShaggyScoobySnack_Pickup", &["snack"]),
    ("PS_ShaggyScoobySnackBuff", &["snack"]),
    // (Buff_Arcade_MidBoss CreateBuffVisuals @205)
    ("PS_ShaggyRageAuraBuff_Loop", &["snack", "arcade"]),
    ("PS_WonderWomanBracers_Cancel", &["snack"]),
    ("PS_Batarang", &["batman"]),
    ("PS_Bombarang", &["batman"]),
    ("PS_BatmanPickup", &["batman"]),
    ("PS_ShaggyScoobySnack", &["batman"]),
    ("PS_BatmanGrappleGunImpact", &["batman"]),
    ("PS_BatmanSmokeBombImpact", &["batman"]),
    ("PS_BatmanSmoke", &["batman"]),
    ("PS_BatmanBombAttach", &["batman"]),
    ("PS_BatmanBomb", &["batman"]),
    ("PS_BatBomb_WarningUp", &["batman"]),
    ("PS_BatmanBombExplosion", &["batman"]),
    ("PS_BugsBunnySafeProjectile", &["creature"]),
    ("PS_Debuff_ElectricityBurst", &["creature", "blackadam"]),
    ("PS_Garnet_Debuff_Electricity", &["creature", "blackadam"]),
    ("PS_Garnet_ElecBeam", &["creature", "blackadam"]),
    ("PS_TeleportingScreenOverlay", &["rick"]),
    ("PS_Laser_Muzzle", &["rick"]),
    ("PS_Blaster_Impact", &["rick"]),
    ("PS_NinjaBomb", &["rick"]),
    ("PS_ShadowClone_Impact", &["rick", "taz", "velma", "grow", "snack"]),
    // Buff_AXBotEnemyHardNemesis EyesMulticast @289 / @492
    ("PS_RageEyes", &["arcade"]),
    ("PS_ShaggyScoobySnacksPickup", &["velma", "snack"]),
    ("PS_Heal_Loop", &["velma"]),
    ("PS_AryaDashAllyTrail02", &["velma"]),
];

/// Perks: what a perk's buffs show, owned by the perk's id (the PerkData export's name, matched exactly): each system
/// none of whose possible wearers' kits owns it.
pub const PERK_FOR: &[(&str, &[&str])] = &[
    ("PS_HitImpact_Big", &["Perk_General_Sniper"]),
    ("PS_CreatureCrystal_Buff", &["Perk_General_ElectricProjectile"]),
    ("PS_Garnet_Debuff_Electricity", &["Perk_General_ElectricProjectile"]),
    ("PS_Debuff_ElectricityBurst", &["Perk_General_ElectricProjectile"]),
    ("PS_Garnet_ElecBeam", &["Perk_General_ElectricProjectile"]),
    ("PS_Debuff_Fire_FOE", &["Perk_C020_Portal", "Perk_General_FireProjectile"]),
    ("P_Impact_Ice", &["Perk_C020_Portal", "Perk_General_IceProjectile"]),
    ("PS_SupermanIceBreath_IceCube", &["Perk_General_IceProjectile"]),
    ("PS_WonderWomanBracers_Cancel", &["Perk_C020_Portal"]),
    ("PS_Buff_FireFist_VFX", &["Perk_C020_Portal"]),
    ("PS_Creature_FireWall", &["Perk_C017_ScorchedEarth", "Perk_Superman_FireSlam"]),
    ("PS_Creature_FireWallProjectileBuff", &["Perk_Superman_FireSlam"]),
    ("PS_C016_PERK_Explosion", &["Perk_C016_DistanceShot"]),
    ("PS_WW_LassoPerk_LightningGrab", &["Perk_WonderWoman_LassoGrapple"]),
    ("PS_SupermanSPU_Impact", &["Perk_BugsBunny_Shockwave", "Perk_Harley_ConfettiExplosion"]),
    ("PS_WonderWomanLassoPERK_Projectile", &["Perk_WonderWoman_LassoTipper"]),
    ("PS_FinnShield", &["Perk_General_ProjectileBlock"]),
    ("PS_ShaggyClap", &["Perk_Batman_HeavyGrapple"]),
    ("PS_BugsBunnyKiss_Perk", &["Perk_BugsBunny_LingeringLove"]),
    ("PS_Jake_WeaponMaster", &["Perk_Jake_WeaponMaster"]),
    ("PS_Fizzy_Targeting_Up", &["Perk_Jake_SafetyNet"]),
    ("PS_BigJump", &["Perk_Jake_SafetyNet"]),
    // Knockout Bounce (Buff_KnockOutBounce): BlastBoxPlayerExit CheckForBlastBoxBounce's
    // BlastBoxExitKnockbackExplosion, BoomVisuals @2722-3503 (mvs-sim combat/perks/hooks.rs).
    // Given to every match, not to batman only: a match without Batman draws it too.
    ("PS_BatmanBombExplosion", &["Perk_General_KnockOutBounce"]),
];

/// Systems a fighter's data names that only a perk of [`PERK_FOR`] shows: that perk alone owns them.
pub const PERK_ONLY: &[&str] = &["PS_BugsBunnyKiss_Perk"];

/// Systems of a fighter's folder that this fighter never shows, so the folder gives them no owner.
pub const NOT_FOLDER: &[&str] = &[
    "PS_ShaggyScoobySnack_Pickup",
    "PS_ShaggyScoobySnackBuff",
    "PS_ShaggyScoobySnack",
    "PS_BugsBunnySafeProjectile",
    "PS_ShaggyScoobySnacksPickup",
    "PS_BugsBunnyKiss_Perk",
    "PS_WW_LassoPerk_LightningGrab",
    "PS_WonderWomanLassoPERK_Projectile",
];

/// Materials of mesh components the runtime builds itself, keyed `<path below /Game, no export>#local` (a
/// FLocalVertexFactory) or `#spline` (a SplineMeshComponent's).
pub const COMPONENT_MATERIALS: &[&str] = &[
    "Panda_Main/Characters/C019/VFX/C019_PlasmaWhip#local",
    "Panda_Main/Characters/C019/VFX/C019_RewindTrail_MAT#spline",
    "Panda_Main/Characters/C017/VFX/C017_FireMesh01_MAT#local",
    "Panda_Main/Characters/C017/VFX/C017_BoosterFire_MAT#local",
    "Panda_Main/Characters/C017/VFX/C017_UI_Reticule_Inner_MAT#local",
    "Panda_Main/VFX/Materials/Mat_Shield_P1#local",
    "Panda_Main/VFX/Materials/Mat_Shield_P2#local",
    "Panda_Main/Characters/TomAndJerry/VFX/TomJerryFishLine_MAT#local",
];

/// Static meshes of those components (cooked LOD 0 buffers, as the particle meshes).
pub const COMPONENT_MESHES: &[&str] = &[
    "Panda_Main/Characters/C019/Animations/Trails/C019_Marker_Mesh",
    "Panda_Main/VFX/Meshes/SM_Plane_CornerPivot_Slanted",
    "Panda_Main/VFX/Meshes/SM_Plane_CornerPivot",
    "Panda_Main/Characters/C017/VFX/SM_C017_ReticuleArrow",
    "Engine/Content/BasicShapes/Sphere",
];

/// A component mesh, material or decal of a shared folder: the fighters (or perks) whose components draw it.
pub const COMPONENT_FOR: &[(&str, &[&str])] = &[
    ("Panda_Main/VFX/Meshes/SM_Plane_CornerPivot_Slanted", &["irongiant"]),
    ("Panda_Main/VFX/Meshes/SM_Plane_CornerPivot", &["irongiant"]),
    (
        "Panda_Main/Characters/Creature/VFX/CreatureFireGround_Decal_MAT",
        &["lebron", "Perk_C017_ScorchedEarth", "Perk_Superman_FireSlam"],
    ),
    ("Panda_Main/VFX/Materials/Mat_Shield_P1#local", &["Perk_Creature_WreckingBall"]),
    ("Panda_Main/VFX/Materials/Mat_Shield_P2#local", &["Perk_Creature_WreckingBall"]),
    ("Engine/Content/BasicShapes/Sphere", &["Perk_Creature_WreckingBall"]),
    ("Panda_Main/Characters/Shaggy/VFX/ShaggyGroundDecal_MAT", &["finn", "lebron"]),
    // (the boombox's blob shadow)
    ("Panda_Main/Materials/BlobShadow/Character_BlobShadow_MAT", &["stripe"]),
];

/// Decal materials of the port's actors (DBM_Emissive and the like), keyed by their path below /Game.
pub const DECALS: &[&str] = &[
    "Panda_Main/Characters/Shaggy/VFX/ShaggyGroundDecal_MAT",
    "Panda_Main/Characters/Creature/VFX/CreatureFireGround_Decal_MAT",
    "Panda_Main/Characters/Steven/VFX/StevenHealDecal_MAT",
    "Panda_Main/Characters/Velma/VFX/VelmaSlowDecal_MAT",
    "Panda_Main/Characters/C023B/VFX/C023B_SawBlade_Ground_RDBA_MAT",
    "Panda_Main/Materials/BlobShadow/Character_BlobShadow_MAT",
];
