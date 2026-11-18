//! What the fighter binds reach beyond the defaults: the actors, montages and buffs a pawn's or an actor's graph
//! spawns, plays or applies, written out from the decodes: the classes and
//! montages a class's graphs spawn, play or hand on by a literal, ported from their bytecode (`@n` is a statement
//! offset of the class's Blueprint).
//!
//! The tables stand for the bytecode: nothing reads bytecode at import. They were compared with a scan
//! over the bytecode of every class the characters' data reaches (GRAPH_BUFFS included), so a graph literal these
//! lack was found there.
//!
//! Keys are class names (`Pawn_BugsBunny_C`): what they are matched against is a name (a component's `type`, an
//! actor's `supers`, a class the worklist holds); values are full object paths, the classes and montages the bind
//! loads (a short name gives no package). Order matters: the worklist follows each list in its order, and the order
//! of reaching numbers the data's objects for the state hash.

use crate::kind::KindError;

/// A Blueprint class by its folder and package under /Game/Panda_Main: the class named as the package with `_C`, or
/// `cls`.
macro_rules! c {
    ($dir:literal, $name:literal) => {
        concat!("/Game/Panda_Main/", $dir, "/", $name, ".", $name, "_C")
    };
    ($dir:literal, $name:literal, $cls:literal) => {
        concat!("/Game/Panda_Main/", $dir, "/", $name, ".", $cls)
    };
}
/// An asset (a montage) by its folder and package under /Game/Panda_Main.
macro_rules! m {
    ($dir:literal, $name:literal) => {
        concat!("/Game/Panda_Main/", $dir, "/", $name, ".", $name)
    };
}
// (`m` for the other binds of the kind; none outside this file names a montage by its package yet)
#[allow(unused_imports)]
pub(crate) use {c, m};

/// A table keyed by class or character, each key's list in its order.
pub struct Reach {
    entries: &'static [(&'static str, &'static [&'static str])],
}

impl Reach {
    pub const fn of(entries: &'static [(&'static str, &'static [&'static str])]) -> Reach {
        Reach { entries }
    }

    /// The list of `key` (empty when the table has none).
    pub fn get(&self, key: &str) -> Result<&'static [&'static str], KindError> {
        Ok(self.entries.iter().find(|(k, _)| *k == key).map_or(&[], |(_, l)| l))
    }

    /// The list of `key` matched without regard to case (the build's names: a table keyed by object path).
    pub fn get_any_case(&self, key: &str) -> &'static [&'static str] {
        self.entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map_or(&[], |(_, l)| l)
    }

    /// Every key's list, in the table's order.
    pub fn entries(&self) -> Result<&'static [(&'static str, &'static [&'static str])], KindError> {
        Ok(self.entries)
    }
}

/// Classes an actor's own graph spawns with a class literal (no class default property names them). A key may also be
/// the class of a component of the pawn (BaseFighter's or its own), the pawn's own class, or a buff the data holds (a
/// buff's graph that spawns an actor: the worklist's late pass).
/// C019_MortarAim SeismicBoom: CompSpawnActor(c019_CookGrenade_Split_C) for a grenade in its boxes.
///
/// Left out, per spawn literal of a reached graph whose spawn no match reaches:
/// - Buff_Ice @3484 ShatterProjectile_C: only on the ShatterEvent DoOnce chain (@4399 -> @3738 -> @3633 -> @2782);
///   nothing calls or binds ShatterEvent (the buff's binds are HitBreakFreeze and its other handlers).
/// - Bugs_AnvilDrop @268 Item_Anvil_C (BeginPlay, after a 2 s Delay): Bugs_AnvilDrop is spawned only by
///   BugsBunny_Attack_Air_PaintTarget_Montage, which no attack list or branch plays.
/// - Pawn_C015 @16237 C015ComboEscape_C, in SpawnEscapeServer (entry @16434 -> @15984), which nothing calls.
/// - Pawn_C017 ShootAtTarget @1440 C017_TenticalShot_C: ShootAtTarget has no caller.
/// - Pawn_C018 @337 C018CannoAmmo_Item_C, in ServerSpawnAmmo (entry @3386 -> @30), which nothing calls.
/// - Pawn_C018 @2197 C018WarpWithAllyHit2_C, on the ally branch of BPI_WarpWithFighter (entry @1796), which no caller
///   takes.
/// - Buff_BasketballPass SpawnVFX @366 (called @2934) Orbiting_PS_Actor_C: reached (LeBron's pass buff), but a
///   particle-only actor with no hit box and no state; the renderer draws it from the buff (`mvs-render`
///   `fx/chars/lebron.rs`), with no sim def.
pub const GRAPH_SPAWNS: Reach = Reach::of(&[
    ("C019_MortarAim_C", &[c!("Characters/C019/Projectiles", "c019_CookGrenade_Split")]),
    // Iron Giant's weapon mode aimers (CompSpawnActor with a class literal at their ActivateObject).
    ("C017_CannonShotAim_C", &[c!("Characters/C017/BluePrints", "C017_CannonShot")]),
    ("C017_PlasmaAimer_C", &[c!("Characters/C017/BluePrints", "C017_Plasma_Shot")]),
    ("C017_EnergyDiscAimer_C", &[c!("Characters/C017/BluePrints", "C017_DiscShot")]),
    ("C017_CannonAim_C", &[c!("Characters/C017/BluePrints", "C017_CannonShot")]), // ActivateObject @986
    ("C017_ThrownScrap_C", &[c!("Characters/C017/BluePrints", "Item_C017_Scrap")]), // OnBounce @771, HitEnemyPlayer @1439
    // The ally turret (a component of Pawn_C017), CompSpawnActor with a class literal: PilotFastAttack @11212,
    // PilotLargeAttackVisuals @974, JumpIn @6364.
    (
        "Comp_C017_Ally_Turret_C",
        &[
            c!("Characters/C017/BluePrints", "C017_TenticalFastShot"),
            c!("Characters/C017/BluePrints", "C017_Ally_Turret_Bomb"),
            c!("Characters/C017/BluePrints", "C017_Pilot_Reticle"),
        ],
    ),
    // Superman. SupermanGroundLazer ExecuteUbergraph @4036: CompSpawnActor(SupermanGroundLazerDamage_C).
    // Superman_IceBreath's overlap @2349: CompSpawnActor(IceBlockItem_C) in place of a frozen projectile.
    ("SupermanGroundLazer_C", &[c!("Blueprints/Projectiles", "SupermanGroundLazerDamage")]),
    ("Superman_IceBreath_C", &[c!("Blueprints/Items", "IceBlockItem")]),
    // Batman. SmokeGrenade ExecuteUbergraph @137: the instigator's CompSpawnActor(SmokeGrenadeOcclusionBP_C), the
    // smoke zone, where the grenade lands.
    ("SmokeGrenade_C", &[c!("Maps/Props/FogOcclusion", "SmokeGrenadeOcclusionBP")]),
    // Black Adam. C021_ElectricChain ExecuteUbergraph @7261: the instigator's CompSpawnActor(C021_ElectricChain_C),
    // the next link of the chain; C021_ElectricChain_Starter (the first link) is its child with the parent's graph.
    ("C021_ElectricChain_C", &[c!("Characters/C021/BluePrints", "C021_ElectricChain")]),
    ("C021_ElectricChain_Starter_C", &[c!("Characters/C021/BluePrints", "C021_ElectricChain")]),
    // Bugs Bunny (bugs, BugsBunnyV2). Pawn_BugsBunny ReceiveBeginPlay @65: Comp_Spawner.CompSpawnActor(ItemDispenser_Bugs_C)
    // (the ACME box's dispenser, which the crouch loop's ActivateSpawned states call). Comp_Pawn_Tunnel @1897
    // BeginDeferredActorSpawnFromClass(BugsBunnyTunnelParent_C). The item pools are CDO arrays their graphs pick from
    // (no ActorClass property the build follows): ItemDispenser_Bugs ItemSpawnList (SpawnItem, ubergraph @187),
    // BugsBunnySafe SpawnableItems (@2041 Array_Get, @2100 ActorClassSpawnedAtDeath), both [Item_Meeseek, Item_Sandwich,
    // Item_Dynamite, Item_Helper]; Item_Helper HelperList (@63 / @595 Array_Random, @434 CompSpawnActor, @816
    // BeginDeferredActorSpawnFromClass) [ItemHelper_BananaGuard, ItemHelper_Nepter]. Item_Meeseek @1337
    // SelfActivate_Meeseeks_Run_C; ItemHelper_Nepter @1023 Projectile_Neptr_Pie_C; BugsBunny_SafeDrop @2786 BugsBunnySafe_C.
    ("Pawn_BugsBunny_C", &[c!("Blueprints/Items/BL_Bugs", "ItemDispenser_Bugs")]),
    ("Comp_Pawn_Tunnel_C", &[c!("Blueprints/Projectiles", "BugsBunnyTunnelParent")]),
    (
        "ItemDispenser_Bugs_C",
        &[
            c!("Blueprints/Items", "Item_Meeseek"),
            c!("Blueprints/Items", "Item_Sandwich"),
            c!("Blueprints/Items", "Item_Dynamite"),
            c!("Blueprints/Items", "Item_Helper"),
        ],
    ),
    (
        "BugsBunnySafe_C",
        &[
            c!("Blueprints/Items", "Item_Meeseek"),
            c!("Blueprints/Items", "Item_Sandwich"),
            c!("Blueprints/Items", "Item_Dynamite"),
            c!("Blueprints/Items", "Item_Helper"),
        ],
    ),
    ("BugsBunny_SafeDrop_C", &[c!("Blueprints/Projectiles", "BugsBunnySafe")]),
    (
        "Item_Helper_C",
        &[
            c!("Characters/ItemHelpers/BananaGuard", "ItemHelper_BananaGuard"),
            c!("Characters/ItemHelpers/Neptr", "ItemHelper_Nepter"),
        ],
    ),
    ("Item_Meeseek_C", &[c!("Blueprints/Tutorial/TempMeeseeksMove", "SelfActivate_Meeseeks_Run")]),
    ("ItemHelper_Nepter_C", &[c!("Characters/ItemHelpers/Neptr", "Projectile_Neptr_Pie")]),
    // Rick (rick, C020). C020PortalAimer: Comp_Pawn_Portal.SpawnObject with a select of two class literals, at its
    // first placement (@3184 ExitPortalParent_C / @3203 AutoTeleportPortal_C, SpawnObject @3346) and at the partner's
    // (@4653 ExitPortalParent_C / @4672 VisualOnlyPortal_C, SpawnObject @4837). Comp_Pawn_Portal (Pawn_C020's
    // component) HandleTeleport @2374 SpawnObject(VisualOnlyPortal_C). SpawnObject spawns the class it is given; the
    // component's PortalClass default (PortalParent) does not stand in for it.
    (
        "C020PortalAimer_C",
        &[
            c!("Characters/C020/Blueprints", "ExitPortalParent"),
            c!("Characters/C020/Blueprints", "AutoTeleportPortal"),
            c!("Characters/C020/Blueprints", "VisualOnlyPortal"),
        ],
    ),
    ("Comp_Pawn_Portal_C", &[c!("Characters/C020/Blueprints", "VisualOnlyPortal")]),
    // Taz (taz, C015). Pawn_C015 CalculateSpitFighters @1545 Comp_Spawner.CompSpawnActor(C015_HealPickup_C); Buff_Chicken
    // HandleDamageRecieved @650 spawns the same class (a buff's graph is no key here: the pawn's literal brings it in).
    ("Pawn_C015_C", &[c!("Characters/C015/Blueprints", "C015_HealPickup")]),
    // Shaggy (shaggy). Comp_Pawn_Rage (Pawn_Shaggy's component) SpawnScoobySnack @9863
    // Comp_Spawner.CompSpawnActor(ScoobySnackBox_C), the rage's heal box.
    ("Comp_Pawn_Rage_C", &[c!("Blueprints/Projectiles", "ScoobySnackBox")]),
    // Steven (steven, C011). StevenShieldProjectile ExecuteUbergraph @1202 the instigator's
    // Comp_Spawner.CompSpawnActor(Steven_ShieldActor_C) (the barrier); HealGrenade @466 CompSpawnActor(HealOverTimeZoneBP_C)
    // (the heal zone). Not entered: StevenBubbleGrenadePart1 @299 BeginDeferredActorSpawnFromClass(StevenBubbleGrenadePart2_C)
    // (Part1 is dormant). Pawn_StevenUniverse @3572
    // SpawnAIFromClass(Pawn_StevenUniverse_C) spawns the pawn's own class (the Watermelon minion): no actor entry.
    ("StevenShieldProjectile_C", &[c!("Blueprints/Projectiles", "Steven_ShieldActor")]),
    ("HealGrenade_C", &[c!("Maps/Props/FogOcclusion", "HealOverTimeZoneBP")]),
    // Garnet (garnet, C012). GarnetLightningBolt (Characters/Garnet/Blueprints, the up special's bolt) EndPlay @566 the
    // instigator's Comp_Spawner.CompSpawnActor(GarnetElectricZone_C). Not entered: Pawn_Garnet @5244
    // CompSpawnActor(GarnetMarker2_C) (BPI_SpawnIdleGauntlets, dormant; the marker comes from the down
    // special's Spawner notify), @2734 / @3706 GarnetGauntletsRecall_C (the recall, dormant), GarnetLightningStrike @2103
    // (the prototype's). GarnetGauntletsProjectile @933 CompSpawnActor(GarnetMarker2_C) is a live perk route (gated by
    // BPI_CheckIfHasBuff(Buff_Garnet_Marker) @3868, Perk_Garnet_Marker): entered.
    ("GarnetLightningBolt_C", &[c!("Characters/Garnet/Blueprints", "GarnetElectricZone")]),
    ("GarnetGauntletsProjectile_C", &[c!("Characters/Garnet/Blueprints", "GarnetMarker2")]),
    // Finn (finn, C013). Pawn_Finn's own graph @1806 / @7202 Comp_Spawner.CompSpawnActor(FinnTreasure_C) (the coins his
    // hits and drops make); FinnDashThrownTarget (the gem) @7321 FinnGemBoostBlast_C and @7726 / @8010
    // FinnGemBoostUseProjectile_C (its perk pulse and item-use shots); FinnHighFiveListener @1791
    // Finn_HighFive_Shockwave_C (the ally high five); Bmo_Chop_Buff (a buff: the late pass above) @207 BMO_FallowActor_C.
    // Not entered: Buff_Finn_GemOnCharge @1804 FinnDashThrownTarget_C (the gem, already there from the coin throw's
    // Spawner notify).
    ("Pawn_Finn_C", &[c!("Characters/Finn/BluePrints", "FinnTreasure")]),
    (
        "FinnDashThrownTarget_C",
        &[
            c!("Characters/Finn/BluePrints", "FinnGemBoostBlast"),
            c!("Characters/Finn/BluePrints", "FinnGemBoostUseProjectile"),
        ],
    ),
    ("FinnHighFiveListener_C", &[c!("Characters/Finn/BluePrints", "Finn_HighFive_Shockwave")]),
    ("Bmo_Chop_Buff_C", &[c!("Characters/Finn/BluePrints", "BMO_FallowActor")]),
    // Jake (jake, C005). Comp_JakeEat (a pawn component) @5678 Comp_Spawner.CompSpawnActor(JakeEatAlly_Aim_C) (the ally
    // spit's aimer; @1835 ActivateActors of the same class); JakeEatAlly_Aim @6739 CompSpawnActor(JakeSpitAllyHitBox_C);
    // ProjectileAimer_SpitJake @1465 CompSpawnActor(JakeSpitCharacterHitBox_C). The other actors come from the montages'
    // Spawner notifies; Buff_AttachToAllyHorse (JakeHorse_Actor3 @18812) and Comp_JakeEat's buffs are graph_buffs'.
    ("Comp_JakeEat_C", &[c!("Blueprints/Projectiles", "JakeEatAlly_Aim")]),
    ("JakeEatAlly_Aim_C", &[c!("Blueprints/Projectiles", "JakeSpitAllyHitBox")]),
    ("ProjectileAimer_SpitJake_C", &[c!("Blueprints/Projectiles", "JakeSpitCharacterHitBox")]),
    // Tom and Jerry (tomandjerry, C010). Comp_Spawner.CompSpawnActor with a class literal: TomAndJerry_SlingShot_Aim_02
    // @1387 Jerry_SpawnedActor_C; Jerry_SpawnedActor @9383 Tom_and_Jerry_TennisBall_Reflected_C (the returned ball)
    // and LaunchCork @1583 JerryCork_C; TomAndJerry_NoJerry_SlingShot_Aim @5763 Tom_and_Jerry_TennisBall_C; JerryRocket
    // @7418 and JerryKickedRocket @183 Jerry_SpawnedActor_TempInvincibility_C; Dynamite_SpawnedActor @2555 / @3303 /
    // @3934 CantSplitDynamite_SpawnedActor_C. Not entered: Jerry_SpawnedActor's SlingShot @1342
    // (Tom_and_Jerry_TennisBall_Reflected_AllyFollow_C): the function has no caller. The aimers and the other actors
    // come from the montages' Spawner notifies.
    ("TomAndJerry_SlingShot_Aim_02_C", &[c!("Blueprints/Projectiles/TomAndJerry", "Jerry_SpawnedActor")]),
    (
        "Jerry_SpawnedActor_C",
        &[
            c!("Blueprints/Projectiles/TomAndJerry", "Tom_and_Jerry_TennisBall_Reflected"),
            c!("Blueprints/Projectiles/TomAndJerry", "JerryCork"),
        ],
    ),
    ("TomAndJerry_NoJerry_SlingShot_Aim_C", &[c!("Blueprints/Projectiles/TomAndJerry", "Tom_and_Jerry_TennisBall")]),
    ("JerryRocket_C", &[c!("Blueprints/Projectiles/TomAndJerry", "Jerry_SpawnedActor_TempInvincibility")]),
    ("JerryKickedRocket_C", &[c!("Blueprints/Projectiles/TomAndJerry", "Jerry_SpawnedActor_TempInvincibility")]),
    ("Dynamite_SpawnedActor_C", &[c!("Blueprints/Projectiles/TomAndJerry", "CantSplitDynamite_SpawnedActor")]),
    // Velma (velma). Pawn_Velma's evidence timer @2316 CompSpawnActor(VelmaEvidenceNoGrav_C); VelmaVialAttack @339
    // SpawnActor(SlowtoStunGrenade_C); Buff_VelmaHealOverTimeBlast @281 VelmaChildBeam_C; Buff_VelmaSpawnHomingTarget
    // @1348 VelmaMarkProjectile_C; VelmaHealBeam @9696 and Buff_SpawnEvidence @631 VelmaEvidence_C. Pawn_Velma's
    // SpawnAIFromClass @5963 (Pawn_VelmaAIBotPolice) is the data-only key velmapolice (MATCH_NEEDS), not an actor.
    // The police pawn's own BeginDeferredActorSpawnFromClass @792 VelmaJenkinsPin_C (built into velmapolice.json).
    // VelmaAimProjectile @5456 sets ActorClassSpawnedAtDeath to VelmamSassFollowUpProjectile_C on an ally hit
    // (SpawnedActorParent @16684 spawns it at death); no property names the class.
    ("Pawn_Velma_C", &[c!("Characters/Velma/BluePrints", "VelmaEvidenceNoGrav")]),
    ("VelmaVialAttack_C", &[c!("Maps/Props/FogOcclusion", "SlowtoStunGrenade")]),
    ("Buff_VelmaHealOverTimeBlast_C", &[c!("Characters/Velma/BluePrints", "VelmaChildBeam")]),
    ("Buff_VelmaSpawnHomingTarget_C", &[c!("Characters/Velma/BluePrints", "VelmaMarkProjectile")]),
    ("VelmaHealBeam_C", &[c!("Characters/Velma/BluePrints", "VelmaEvidence")]),
    ("Buff_SpawnEvidence_C", &[c!("Characters/Velma/BluePrints", "VelmaEvidence")]),
    ("Pawn_VelmaAIBotPolice_C", &[c!("Characters/Velma", "VelmaJenkinsPin")]),
    ("VelmaAimProjectile_C", &[c!("Characters/Velma/BluePrints", "VelmamSassFollowUpProjectile")]),
    // Marvin (marvin, C018). The UFO C018_Platform fires its cannon through the instigator's Comp_Spawner @14742
    // CompSpawnActor(C018CannonBlast_C) (FireBlastServer, reached from ActivateObject @15164-15174); the perk UFO
    // C018_PerkPlatform @1443 CompSpawnActor(C018PerkCannonBlast_C). Each blast's ActorClassSpawnedAtDeath brings its
    // rocket explosion. C018_Platform's BeginPlay C019MarkerCameraFocus_C (@11221, BeginDeferredActorSpawnFromClass) is
    // left to the code, as Morty's marker's. C018Cannon (@1244, the dormant
    // SpawnCannon montage's actor) has no entry.
    ("C018_Platform_C", &[c!("Characters/C018/Blueprints", "C018CannonBlast")]),
    ("C018_PerkPlatform_C", &[c!("Characters/C018/Blueprints", "C018PerkCannonBlast")]),
    // Stripe (stripe, C023B). Pawn_C023B's Comp_C023B_TargetTracker fires the gun through the owner's Comp_Spawner
    // @2540 / @3390 CompSpawnActor(C023B_SpawnedGunShot_C); Comp_C023B_Bomb @452 CompSpawnActor(C023B_DroppedBomb_C).
    // The live saw C023B_SawBlade_EdgeTrace (Air_Saw_Throw; Saw_Throw spawns its data child _StartOnGround) HitAlly @8246
    // CompSpawnActor(C023B_AllySaw_C); C023B_AllySaw @1149 CompSpawnActor(C023B_SawBlade_FromAlly_EdgeTrace_C), a child
    // of the same saw whose own graph is ReceiveBeginPlay alone (HitAlly is the parent's). Not entered: the
    // TargetTracker's BeginDeferredActorSpawnFromClass @3696 C023B_AimWidget_C (an Actor with the UI_C023B_Target
    // widget, left to the code as Marvin's marker focus is) and the dormant C023B_SawBlade @6351 AllySaw (ThrowBlade,
    // a montage no move plays).
    ("Comp_C023B_TargetTracker_C", &[c!("Characters/C023B/BluePrints", "C023B_SpawnedGunShot")]),
    ("Comp_C023B_Bomb_C", &[c!("Characters/C023B/BluePrints", "C023B_DroppedBomb")]),
    ("C023B_SawBlade_EdgeTrace_C", &[c!("Characters/C023B/BluePrints", "C023B_AllySaw")]),
    ("C023B_SawBlade_EdgeTrace_StartOnGround_C", &[c!("Characters/C023B/BluePrints", "C023B_AllySaw")]),
    ("C023B_SawBlade_FromAlly_EdgeTrace_C", &[c!("Characters/C023B/BluePrints", "C023B_AllySaw")]),
    ("C023B_AllySaw_C", &[c!("Characters/C023B/BluePrints", "C023B_SawBlade_FromAlly_EdgeTrace")]),
    // Gizmo (gizmo, C023A). C023A_Music_Buff @2048 CompSpawnActor(C023A_Music_Projectile_C) (the buff the music pickup
    // gives allies, @5362). Every other Gizmo actor comes from a montage's Spawner notify; the rest of the class literals
    // in the C023A graphs are casts and IsA tests (C023A_PopCorn, C023A_MusicPickup, Buff_C023A_Dance).
    ("C023A_Music_Buff_C", &[c!("Characters/C023A/BluePrints", "C023A_Music_Projectile")]),
    // LeBron (lebron, c016). Buff_BasketballPass ThrowBall @2634: the owner's Comp_Spawner.CompSpawnActor(
    // BasketBallProjectile_AllyPass_C) (the team mate's throw back to him); Buff_IgnitedBasketballPass is its
    // child with no ThrowBall of its own.
    ("Buff_BasketballPass_C", &[c!("Characters/c016/Blueprints", "BasketBallProjectile_AllyPass")]),
    ("Buff_IgnitedBasketballPass_C", &[c!("Characters/c016/Blueprints", "BasketBallProjectile_AllyPass")]),
]);

/// Montages a spawned actor's own graph plays on its own mesh with a montage literal (no property names them), by the
/// actor class (an actor whose `supers` holds the key takes them too). The data's `actorMontages` (each the montage()
/// entry of `montages`), apart from the pawn's: these play on the actor's skeleton. Read by the actor's view
/// (FighterData.actorMontages). Meeseeks_Run @1755 CreateProxyObjectForPlayMontage(Meeseeks_SkelMesh,
/// Meeseeks_Attack_Run_Montage); ItemHelper_Nepter ThrowPieMontageMulticast @5283 (Neptr_Skelmesh, Neptr_Throw_Montage).
/// Not listed: Meeseeks_Base's PlayMontageMulticast literals (ChargeUppercut, GolfSpike: C020's runners, no class of a
/// ported character's).
pub const GRAPH_MONTAGES: Reach = Reach::of(&[
    ("Meeseeks_Run_C", &[m!("Blueprints/Tutorial/TempMeeseeksMove", "Meeseeks_Attack_Run_Montage")]),
    ("ItemHelper_Nepter_C", &[m!("Characters/ItemHelpers/Neptr/Animations", "Neptr_Throw_Montage")]),
    // Rick's Meeseeks (Meeseeks_Base children, C020/Projectiles): CreateProxyObjectForPlayMontage(Meeseeks_SkelMesh,
    // <literal>) in each ubergraph (@180 ChargeFly, @194 ChargePunch, @241 ChargeStomp, @520 ChargeUppercut, @199 the
    // others). Meeseeks_SpawnedActor (the ground Meeseeks, a sibling of Meeseeks_Base) PlayMontageMulticast literals
    // of its AttackUppercutLogic / AttackRunLogic / AttackGolfLogic (Meeseeks_SpawnedActor @648/653/670).
    ("Meeseeks_ChargeFly_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_Flap_Montage")]),
    ("Meeseeks_ChargePunch_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_ChargePunch_Montage")]),
    ("Meeseeks_ChargeStomp_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_ChargeStomp_Montage")]),
    ("Meeseeks_ChargeUppercut_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_ChargeUppercut_Montage")]),
    ("Meeseeks_Fall_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_Fall_Montage")]),
    ("Meeseeks_FlapMultiHit_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_FlapMultiHit_Montage")]),
    ("Meeseeks_Fly_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_Flap_Montage")]),
    ("Meeseeks_Kick_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_Kick_Montage")]),
    ("Meeseeks_Kick2_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_Kick2_Montage")]),
    ("Meeseeks_Punch_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_Punch_Montage")]),
    ("Meeseeks_Spike_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_Spike_Montage")]),
    ("Meeseeks_Stomp_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_Stomp_Montage")]),
    ("Meeseeks_Taunt_C", &[m!("Characters/C020/Meeseeks", "Meeseeks_Attack_LookAtMe_Montage")]),
    (
        "Meeseeks_SpawnedActor_C",
        &[
            m!("Characters/C020/Meeseeks", "Meeseeks_Attack_ChargeUppercut_Montage"),
            m!("Blueprints/Tutorial/TempMeeseeksMove", "Meeseeks_Runnig_Montage"),
            m!("Characters/C020/Meeseeks", "Meeseeks_Attack_GolfSwing_Montage"),
        ],
    ),
    // Jake's house (JakeHouse_Actor, Projectiles/): CreateProxyObjectForPlayMontage(JakeHouse, <literal>) @4901 / @8442 /
    // @9770 / @12151 JakeHouse_Bounce_Montage, @5322 JakeHouse_Exit_Montage, on its own Jake_House mesh
    // (Characters/Jake/SubMeshes/JakeHouse).
    (
        "JakeHouse_Actor_C",
        &[
            m!("Characters/Jake/SubMeshes/JakeHouse", "JakeHouse_Bounce_Montage"),
            m!("Characters/Jake/SubMeshes/JakeHouse", "JakeHouse_Exit_Montage"),
        ],
    ),
    // Jerry (Jerry_SpawnedActor, Projectiles/TomAndJerry; its JerryMesh is Jerry_SkelMesh with JerryAnimBlueprint):
    // PlayMontageMulticast(<literal>) of its own graph, Champagne Attack(Tom_and_Jerry_Champagne_F_Montage) @15259 /
    // (_B) @15694, @7057 Only_Jerry_ReflectSwing, @24373 Jerry_Fly_Bounce, LaunchRocket @0 Jerry_Rocket; and of other
    // classes on Jerry: Tom_and_Jerry_TennisBall @5049 PlayMontageMulticast(Only_Jerry_TennisLoop), TomAimRocket1
    // @5027 PlayMontage(Jerry.JerryMesh, Only_Jerry_HR_03). Not entered: SpawnCheese @0 Jerry_Cheese and SlingShot
    // @1469 Only_Jerry_ThrowBall (functions with no caller). These montages are under Animations/JerryOnly, so they
    // are in `montages` as well.
    (
        "Jerry_SpawnedActor_C",
        &[
            m!("Characters/TomAndJerry/Animations/JerryOnly", "Tom_and_Jerry_Champagne_F_Montage"),
            m!("Characters/TomAndJerry/Animations/JerryOnly", "Tom_and_Jerry_Champagne_B_Montage"),
            m!("Characters/TomAndJerry/Animations/JerryOnly", "Tom_and_Jerry_Only_Jerry_ReflectSwing_Montage"),
            m!("Characters/TomAndJerry/Animations/JerryOnly", "Tom_and_Jerry_Jerry_Fly_Bounce_Montage"),
            m!("Characters/TomAndJerry/Animations/JerryOnly", "Tom_and_Jerry_Jerry_Rocket_Montage"),
            m!("Characters/TomAndJerry/Animations/JerryOnly", "Tom_and_Jerry_Only_Jerry_TennisLoop_Montage"),
            m!("Characters/TomAndJerry/Animations/JerryOnly", "Tom_and_Jerry_Only_Jerry_HR_03_Montage"),
        ],
    ),
    // Velma's police (velmapolice): VelmaJenkinsPin's ubergraph @142 Enter, @3132 Captured, @3639 Exit
    // (CreateProxyObjectForPlayMontage on its own Jenkins mesh).
    (
        "VelmaJenkinsPin_C",
        &[
            m!("Characters/Velma/Animation/Jenkins", "Velma_JenkinsPin_Enter_Montage"),
            m!("Characters/Velma/Animation/Jenkins", "Velma_JenkinsPin_Captured_Montage"),
            m!("Characters/Velma/Animation/Jenkins", "Velma_JenkinsPin_Exit_Montage"),
        ],
    ),
    // Marvin's UFO (C018_Platform; C018_PerkPlatform takes them through its supers): the class defaults IntroMontage,
    // LoadRocketMontage, IdleMontage, FireRocketMontage, TurnMontage, BounceMontage, HitReactFrontMontage,
    // HitReactBackMontage, ExitMontage, DestroyedMontage, played on its own ShipMesh (C018_Spaceship_SkelMesh,
    // @683-@21184). characters.json marvin animSkip keeps these packages out of the pawn's `montages`.
    (
        "C018_Platform_C",
        &[
            m!("Characters/C018/Animations", "C018_Ship_Enter_Montage"),
            m!("Characters/C018/Animations", "C018_Ship_Reload_Montage"),
            m!("Characters/C018/Animations", "C018_Ship_Idle_Montage"),
            m!("Characters/C018/Animations", "C018_Ship_Fire_Montage"),
            m!("Characters/C018/Animations", "C018_Ship_turnAround_Montage"),
            m!("Characters/C018/Animations", "C018_Ship_Bounce_Montage"),
            m!("Characters/C018/Animations", "C018_Ship_HR_F_Montage"),
            m!("Characters/C018/Animations", "C018_Ship_HR_B_Montage"),
            m!("Characters/C018/Animations", "C018_Ship_Exit_Montage"),
            m!("Characters/C018/Animations", "C018_Ship_Destroy_Montage"),
        ],
    ),
    // Gizmo's car (C023A_Car_SpawnedActor): @11555 CarMontageMulticast(C023A_Car_Damage_Montage), played @14303 through
    // CreateProxyObjectForPlayMontage on its own SkeletalMesh (C023A_Corvette_SkelMesh, the Corvette skeleton).
    // characters.json gizmo animSkip keeps the package out of the pawn's `montages`.
    ("C023A_Car_SpawnedActor_C", &[m!("Characters/C023A/Animation/Car", "C023A_Car_Damage_Montage")]),
]);

/// A spawner notify's ActorClass that its own graph never spawns in a match, per notify class: left out of the actors.
/// AnimNotify_C015ResetConditionalSpawner spawns ActorClass only when the pawn's StoredProjectile is no valid class
/// (@1483 / @1782); Pawn_C015's BeginPlay sets C015Trash (@9924 -> BPI_ResetStoredProjectile @452) and every later write
/// is a class, so the montages' C015Anvil_C is spawned only after the perk / debug paths store it
/// (@1058).
pub const NOTIFY_ACTOR_UNSPAWNED: Reach = Reach::of(&[("AnimNotify_C015ResetConditionalSpawner_C", &["C015Anvil_C"])]);

/// The pawn's Comp_Pawn_PassiveBuffManager Buffs whose graph the sim ports: the buff with what its own graph spawns
/// with a class literal.
/// Buffs a passive buff's own graph spawns with a class literal.
/// C017_TempHealth_DamageReflect_Passive TempHealthAdded @548: SpawnBuffActor(Buff_Thorns_C) on its owner.
/// Buff_BackStab (Pawn_Arya's passive row, BuffAllies): a child of Buff_AryaBackStab with no graph of its own; it
/// spawns nothing.
/// Buff_PassiveApplyBuffonHit (Pawn_BatmanV2's passive row, BuffSelf) ExecuteUbergraph @627:
/// SpawnBuffActor(Buff_Weakened_C, WeakenedStacksPerHit) on the fighter its owner hits.
pub const PASSIVE_SPAWNS: Reach = Reach::of(&[
    ("C017_TempHealth_DamageReflect_Passive_C", &[c!("Blueprints/Abilities/Shield", "Buff_Thorns")]),
    ("Buff_BackStab_C", &[]),
    ("Buff_PassiveApplyBuffonHit_C", &[c!("Blueprints/Abilities/Shield", "Buff_Weakened")]),
    // Pawn_C015's passive row: the lick heal (BPI_LickHealOnOverlap @1521 / @2903); its graph spawns no
    // buff
    ("Buff_c015_ApplybuffOnOverlap_C", &[]),
]);

/// What an exported buff spawns that no literal states, by the buff class: the class default members (a dotted path)
/// that hold the class its graph spawns. Buff_AllyElectricGauntlets: a child of Buff_SupermanAllyIceGauntlet with data
/// of its own; the parent's ExecuteUbergraph @374 SpawnBuffActor(BuffToApplyEnemy, StacksToApplyEnemy) on the enemy its
/// holder hits (the child's BuffToApplyEnemy is Buff_Damage_Electric_C).
pub const BUFF_SPAWN_MEMBERS: Reach = Reach::of(&[("Buff_AllyElectricGauntlets_C", &["BuffToApplyEnemy"])]);

/// What a class's graphs (its own and its Blueprint parents', its own first) name as a buff class by a literal: the
/// class SpawnBuffActor is given (an EX_ObjectConst, or the literals of a select in its place, each case a literal or
/// a local variable a function of the class sets to one), and a literal let into a Struct_ApplyBuffs' AppliedBuffs (the
/// on-hit buffs an actor hands to its hit, C021_Bolt @4215). By class: an actor of the data, a component class of the
/// pawn, the pawn's own class, a buff of the data. Each entry's comment gives the statements of the
/// class's bytecode that name each buff (an inherited one: the parent's). A class not here names none.
///
/// Left out, per class whose own bytecode names the buff:
/// - Comp_Pawn_TeleportToAlly @2996 SpawnBuffActor(Buff_MoveToPosition_C), a component of Pawn_Shaggy,
///   Pawn_WonderWoman and Pawn_Arya: the sim ports the move to the ally in code and reads no def (Arya has
///   the def all the same, from MarkTeleportProjectile and AryaBackStabProjectile).
/// - Buff_Enlarge (the mode buffs, every character) @2154-2233: SpawnBuffActor(select(Buff_Polymorph_ProjectileEnlarge
///   _WithHieghtChange_C, Buff_Polymorph_ProjectileEnlarge_C)) on a projectile its holder spawns (the sim's
///   enlargeSpawned). The defs would renumber every character's static ids; a match finds them in Rick's data
///   (PolymorphGrenade names them), and an FFA match (where the mode gives the buff) in the items part's item buffs,
///   else the buff is refused with a warning.
/// - Comp_Pawn_Tunnel (Pawn_BugsBunny's component) AddPortalBuff @0 SpawnBuffActor(Buff_PortalBuff_C): a function
///   nothing calls (xref calls AddPortalBuff: Comp_Pawn_Portal's own HandleTeleport alone, C020's).
/// - Marvin. C018_Platform HandleFighterInGetInCollision @835
///   SpawnBuffActor(Buff_C018ControlPlatform_C): a function nothing calls (the GetInCollision overlap events @15217 /
///   @15218 are a bare pop_flow), the UFO's piloting. Pawn_C018 BPI_WarpWithFighter (entry @1796) @2366
///   SpawnBuffActor(Buff_C018ProjectileWarpMoveAlly_C): an interface event nothing calls (its enemy branch's
///   Buff_C018ProjectileWarpEnemy_C @2667 stays: Buff_C018WarpFighterHit @584 spawns it).
/// - Stripe. Comp_C023B_EnemyAttach @1854 SpawnBuffActor(Buff_C023B_AttachSlow_C) is on the
///   attach path, reached only from ActorReachedLogic, which nothing calls or binds in this kit (the bite is dormant).
pub const GRAPH_BUFFS_SKIP: Reach = Reach::of(&[
    ("Comp_Pawn_TeleportToAlly_C", &["Buff_MoveToPosition_C"]),
    ("Buff_Enlarge_C", &["Buff_Polymorph_ProjectileEnlarge_WithHieghtChange_C", "Buff_Polymorph_ProjectileEnlarge_C"]),
    ("Comp_Pawn_Tunnel_C", &["Buff_PortalBuff_C"]),
    ("C018_Platform_C", &["Buff_C018ControlPlatform_C"]),
    ("Pawn_C018_C", &["Buff_C018ProjectileWarpMoveAlly_C"]),
    ("Comp_C023B_EnemyAttach_C", &["Buff_C023B_AttachSlow_C"]),
]);

pub const GRAPH_BUFFS: Reach = Reach::of(&[
    // AryaBackStabProjectile's: Buff_MoveToPosition @583 / @667
    ("AryaBackStabAirProjectile_C", &[c!("Blueprints/Abilities/Shield", "Buff_MoveToPosition")]),
    // Buff_PieHeal @713, Buff_SlowDown @868
    (
        "AryaKillPie_C",
        &[c!("Blueprints/Abilities/Shield", "Buff_PieHeal"), c!("Blueprints/Abilities/Shield", "Buff_SlowDown")],
    ),
    // PortalParent's: Buff_Elemental_Fire @12524
    ("AutoTeleportPortal_C", &[c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire")]),
    // BasketBallProjectile's: Buff_BasketballPass @749 / @7385, Buff_C016_Ball_Conditional @1776 / @7028, Buff_IgnitedBasketballPass @7404
    (
        "BasketBallProjectile_AllyPass_C",
        &[
            c!("Characters/c016/Blueprints", "Buff_BasketballPass"),
            c!("Characters/c016/Blueprints", "Buff_c016_Ball_Conditional", "Buff_C016_Ball_Conditional_C"),
            c!("Characters/c016/Blueprints", "Buff_IgnitedBasketballPass"),
        ],
    ),
    // BasketBallProjectile's: Buff_BasketballPass @749 / @7385, Buff_C016_Ball_Conditional @1776 / @7028, Buff_IgnitedBasketballPass @7404
    (
        "BasketBallProjectile_Pass_C",
        &[
            c!("Characters/c016/Blueprints", "Buff_BasketballPass"),
            c!("Characters/c016/Blueprints", "Buff_c016_Ball_Conditional", "Buff_C016_Ball_Conditional_C"),
            c!("Characters/c016/Blueprints", "Buff_IgnitedBasketballPass"),
        ],
    ),
    // BasketBallProjectile's: Buff_BasketballPass @749 / @7385, Buff_C016_Ball_Conditional @1776 / @7028, Buff_IgnitedBasketballPass @7404
    (
        "BasketBallProjectile_Shot_C",
        &[
            c!("Characters/c016/Blueprints", "Buff_BasketballPass"),
            c!("Characters/c016/Blueprints", "Buff_c016_Ball_Conditional", "Buff_C016_Ball_Conditional_C"),
            c!("Characters/c016/Blueprints", "Buff_IgnitedBasketballPass"),
        ],
    ),
    // Buff_ConfettiCount @303, Buff_ConfettiExplosionCount @284
    (
        "Buff_ApplyConfetti_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_ConfettiCount"),
            c!("Blueprints/Abilities/Shield", "Buff_ConfettiExplosionCount"),
        ],
    ),
    // Buff_Tasty @229 / @727
    ("Buff_ApplyTasty_C", &[c!("Characters/C015/Blueprints", "Buff_Tasty")]),
    // Buff_DelayKnockUp @132
    ("Buff_BatmanOnHitApplyBuff_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_DelayKnockUp")]),
    // Buff_StevenBubbleEnemy @1005
    ("Buff_BubbleCount_C", &[c!("Blueprints/Abilities/Shield/BL_Steven", "Buff_StevenBubbleEnemy")]),
    // Buff_Fire @770
    ("Buff_C016_IgnitedDunk_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")]),
    // Buff_C018HomingProjectile @3226 / @3427 / @626 / @775
    ("Buff_C018HomingVictim_C", &[c!("Characters/C018/Blueprints/Buffs", "Buff_C018HomingProjectile")]),
    // Buff_C018ProjectileWarpEnemy @584
    ("Buff_C018WarpFighterHit_C", &[c!("Characters/C018/Blueprints/Buffs", "Buff_C018ProjectileWarpEnemy")]),
    // Buff_AlterProjectileSpeed @807 / @2050, Buff_C021_SlowReflectProjectile @1195 / @2334
    (
        "Buff_C021_SetHomingTowardsTarget_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_AlterProjectileSpeed"),
            c!("Characters/C021/BluePrints", "Buff_C021_SlowReflectProjectile"),
        ],
    ),
    // Buff_TastyBlock @530
    ("Buff_Chicken_C", &[c!("Characters/C015/Blueprints", "Buff_TastyBlock")]),
    // Buff_Fire @2475
    ("Buff_ConfettiCount_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")]),
    // Buff_ConfettiCount's: Buff_Fire @2475
    ("Buff_ConfettiExplosionCount_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")]),
    // Buff_Cleanse @49
    ("Buff_Educated_C", &[c!("Blueprints/Abilities/Shield", "Buff_Cleanse")]),
    // Buff_Educated's: Buff_Cleanse @49
    ("Buff_Educated_C020_C", &[c!("Blueprints/Abilities/Shield", "Buff_Cleanse")]),
    // Buff_Damage_Electric @1325 / @2723, Buff_Elemental_Electric @2508
    (
        "Buff_Elemental_Electric_C",
        &[
            c!("Blueprints/Buffs/Elemental", "Buff_Damage_Electric"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Electric"),
        ],
    ),
    // Buff_Fire @1647 / @8701 / @9062 / @9296, Buff_Elemental_Fire @6784 / @9798 / @10150 / @1666
    (
        "Buff_Elemental_Fire_C",
        &[
            c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire"),
        ],
    ),
    // Buff_Ice @2245 / @3190 / @8761, Buff_Elemental_Ice @7118 / @9219 / @9571 / @1375
    (
        "Buff_Elemental_Ice_C",
        &[c!("Blueprints/Abilities/Shield", "Buff_Ice"), c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Ice")],
    ),
    // Buff_PoliceEvidence @1473
    ("Buff_EvidenceCount_C", &[c!("Characters/Velma/BluePrints", "Buff_PoliceEvidence")]),
    // Buff_Fear @608
    ("Buff_FearStack_C", &[c!("Blueprints/Abilities/Shield", "Buff_Fear")]),
    // Buff_Elemental_Fire @551 / @635
    ("Buff_IgnitedBasketballPass_C", &[c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire")]),
    // Buff_StunnedMoveToPositionThroughGeo @3509 / @3597
    (
        "Buff_OnExitedArenaTeleportToAlly_C",
        &[c!("Blueprints/Abilities/Shield", "Buff_StunnedMoveToPositionThroughGeo")],
    ),
    // Buff_Weakened @627
    ("Buff_PassiveApplyBuffonHit_C", &[c!("Blueprints/Abilities/Shield", "Buff_Weakened")]),
    // Buff_Fire @1338 / @1465
    ("Buff_PortalBuff_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")]),
    // Buff_Weakened @1051
    ("Buff_ScoobySnack_C", &[c!("Blueprints/Abilities/Shield", "Buff_Weakened")]),
    // Buff_Ice @618 / @949
    ("Buff_SlowIntoStun_C", &[c!("Blueprints/Abilities/Shield", "Buff_Ice")]),
    // Buff_MoveToPosition @1588 / @1672
    ("Buff_StevenBubbleEnemy_C", &[c!("Blueprints/Abilities/Shield", "Buff_MoveToPosition")]),
    // Buff_Fire @409
    ("Buff_Superman_FireSlamConditional_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")]),
    // Buff_Disguise @618 / @1051 / @1135
    ("Buff_TakeFace_C", &[c!("Blueprints/Abilities/Shield", "Buff_Disguise")]),
    // Buff_Tasty @231
    ("Buff_TastyOnHit_C", &[c!("Characters/C015/Blueprints", "Buff_Tasty")]),
    // Buff_Chicken @2081, Buff_TastySpeedBoost @1140
    (
        "Buff_Tasty_C",
        &[c!("Characters/C015/Blueprints", "Buff_Chicken"), c!("Characters/C015/Blueprints", "Buff_TastySpeedBoost")],
    ),
    // Buff_Reverse @734
    ("Buff_TurnEnemyAwayOnHit_C", &[c!("Blueprints/Abilities/Shield", "Buff_Reverse")]),
    // Buff_ProjectileThroughTunnel @13008 / @13114
    ("BugsBunnyTunnelParent_C", &[c!("Characters/BugsBunnyV2/Blueprints", "Buff_ProjectileThroughTunnel")]),
    // Buff_C017_AllyBurst @6213, Buff_C017_Passive @8557
    (
        "C017_AntenaBolts_C",
        &[
            c!("Characters/C017/BluePrints", "Buff_C017_AllyBurst"),
            c!("Characters/C017/BluePrints", "Buff_C017_Passive"),
        ],
    ),
    // Buff_HeldByPlasmaBall @394 / @489
    ("C017_Plasma_Shot_C", &[c!("Characters/C017/BluePrints", "Buff_HeldByPlasmaBall")]),
    // Buff_Thorns @548
    ("C017_TempHealth_DamageReflect_Passive_C", &[c!("Blueprints/Abilities/Shield", "Buff_Thorns")]),
    // C018Flag's: Buff_C018HomingVictim @669 / @1463 / @6692 / @8318
    ("C018AirFlag_C", &[c!("Characters/C018/Blueprints/Buffs", "Buff_C018HomingVictim")]),
    // Buff_C018BubbledProjectile @545 / @651
    ("C018BubbleProj_C", &[c!("Characters/C018/Blueprints/Buffs", "Buff_C018BubbledProjectile")]),
    // Buff_C018HomingVictim @669 / @1463 / @6692 / @8318
    ("C018Flag_C", &[c!("Characters/C018/Blueprints/Buffs", "Buff_C018HomingVictim")]),
    // Buff_C018PerkHomingProjectile @2382 / @2466
    ("C018_PerkPlatform_C", &[c!("Characters/C018/Blueprints/Buffs", "Buff_C018PerkHomingProjectile")]),
    // Buff_GarnetStunnedMove @9436
    ("C019Marker_C", &[c!("Characters/Garnet/Blueprints", "Buff_GarnetStunnedMove")]),
    // Buff_Weakened @4792
    ("C019_MortarAim_C", &[c!("Blueprints/Abilities/Shield", "Buff_Weakened")]),
    // Buff_MoveToPosition_LassoGrapple @1927 / @2011
    ("C019_WhipProjectile_C", &[c!("Blueprints/Abilities/Shield", "Buff_MoveToPosition_LassoGrapple")]),
    // Buff_Held_By_C021 @3006
    ("C021GrabHit_C", &[c!("Characters/C021/BluePrints", "Buff_Held_By_C021")]),
    // Buff_AllyElectricGauntlets @4059, Buff_Damage_Electric @4215
    (
        "C021_Bolt_C",
        &[
            c!("Blueprints/Abilities/Shield/BL_Superman", "Buff_AllyElectricGauntlets"),
            c!("Blueprints/Buffs/Elemental", "Buff_Damage_Electric"),
        ],
    ),
    // Buff_AllyElectricGauntlets @1656, Buff_C021_ShockProjectile @4678
    (
        "C021_Discharge_C",
        &[
            c!("Blueprints/Abilities/Shield/BL_Superman", "Buff_AllyElectricGauntlets"),
            c!("Characters/C021/BluePrints", "Buff_C021_ShockProjectile"),
        ],
    ),
    // Buff_C021_EarthSmashVictim @681 / @765
    ("C021_EarthSmashIn_C", &[c!("Characters/C021/BluePrints", "Buff_C021_EarthSmashVictim")]),
    // Buff_AllyElectricGauntlets @2396, C021_Buff_ElectricChainVictim @2415, Buff_Damage_Electric @4953
    (
        "C021_ElectricChain_C",
        &[
            c!("Blueprints/Abilities/Shield/BL_Superman", "Buff_AllyElectricGauntlets"),
            c!("Characters/C021/BluePrints", "C021_Buff_ElectricChainVictim"),
            c!("Blueprints/Buffs/Elemental", "Buff_Damage_Electric"),
        ],
    ),
    // C021_ElectricChain's: Buff_AllyElectricGauntlets @2396, C021_Buff_ElectricChainVictim @2415, Buff_Damage_Electric @4953
    (
        "C021_ElectricChain_Starter_C",
        &[
            c!("Blueprints/Abilities/Shield/BL_Superman", "Buff_AllyElectricGauntlets"),
            c!("Characters/C021/BluePrints", "C021_Buff_ElectricChainVictim"),
            c!("Blueprints/Buffs/Elemental", "Buff_Damage_Electric"),
        ],
    ),
    // Buff_HeldByEnemy @1169 / @1462, Buff_DamageOverTime @1275
    (
        "C021_GrabListener_C",
        &[
            c!("Blueprints/Buffs/Navigation", "Buff_HeldByEnemy"),
            c!("Characters/C021/BluePrints", "Buff_DamageOverTime"),
        ],
    ),
    // Buff_Fire @405
    ("C023A_Arrow_Projectile_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")]),
    // C023A_Car_Buff @129 / @703 / @7877 / @17618
    ("C023A_Car_SpawnedActor_C", &[c!("Characters/C023A/BluePrints", "C023A_Car_Buff")]),
    // C023A_Music_Buff @979 / @3161 / @5101 / @5362
    ("C023A_MusicPickup_C", &[c!("Characters/C023A/BluePrints", "C023A_Music_Buff")]),
    // C023A_Buff_Silenced @1169
    ("C023A_Music_DeBuff_C", &[c!("Characters/C023A/BluePrints", "C023A_Buff_Silenced")]),
    // Buff_Cheese @382 / @1014
    ("Cheese_SpawnedActor_C", &[c!("Blueprints/Abilities/Shield", "Buff_Cheese")]),
    // Buff_C017_Ally_Pilot @6880
    ("Comp_C017_Ally_Turret_C", &[c!("Characters/C017/BluePrints", "Buff_C017_Ally_Pilot")]),
    // C023A_AttachCooldown @1499, C023A_ShareDamageBuff @4414, Buff_C023A_AttachShrink @4588, Buff_MoveToActor2 @466 / @569 / @104
    (
        "Comp_C023A_AllyAttach_C",
        &[
            c!("Characters/C023A/BluePrints", "C023A_AttachCooldown"),
            c!("Characters/C023A/BluePrints", "C023A_ShareDamageBuff"),
            c!("Characters/C023A/BluePrints", "Buff_C023A_AttachShrink"),
            c!("Blueprints/Abilities/Shield", "Buff_MoveToActor2"),
        ],
    ),
    // Buff_Fire @320
    ("Comp_C023B_Bomb_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")]),
    // Buff_JakEatSlow @6091, Buff_ArmorPack @7773, Buff_Weakened @7957
    (
        "Comp_JakeEat_C",
        &[
            c!("Blueprints/Buffs/Navigation/BL_Jake", "Buff_JakEatSlow"),
            c!("Blueprints/Abilities/Shield", "Buff_ArmorPack"),
            c!("Blueprints/Abilities/Shield", "Buff_Weakened"),
        ],
    ),
    // Buff_Teleporting3 @1173 / @1257, Buff_PortalBuff @51
    (
        "Comp_Pawn_Portal_C",
        &[c!("Characters/C020/Blueprints", "Buff_Teleporting3"), c!("Blueprints/Abilities/Shield", "Buff_PortalBuff")],
    ),
    // Buff_TunnelTeleport @470 / @576
    ("Comp_Pawn_Tunnel_C", &[c!("Blueprints/Abilities/Shield", "Buff_TunnelTeleport")]),
    // Buff_CatchUpSpeedBuff @786 (BaseFighter's component, every character): the catch-up loop's spawn while the
    // game mode's BPI_IsCatchUpSpeedBuffEnabled is true. PandaGameMode sets CatchUpSpeedBuffEnabled in its
    // EnableCatchUpSpeedBuff (@20285), the delegate of the timer BPI_SetMatchStarted sets at Duration - 120 s (@4590,
    // @5095): a match longer than 120 s reaches it.
    ("Comp_State_Manager_C", &[c!("Blueprints/Abilities/Shield", "Buff_CatchUpSpeedBuff")]),
    // Buff_StevenSharedShield @94 / @291 / @706 / @321
    ("Comp_StevenShield_C", &[c!("Blueprints/Abilities/Shield/BL_Steven", "Buff_StevenSharedShield")]),
    // Buff_C016_Ball_Conditional @246, Buff_C016_IgnitedDunk @1171
    (
        "DashToBasketball_C",
        &[
            c!("Characters/c016/Blueprints", "Buff_c016_Ball_Conditional", "Buff_C016_Ball_Conditional_C"),
            c!("Characters/c016/Blueprints", "Buff_c016_IgnitedDunk", "Buff_C016_IgnitedDunk_C"),
        ],
    ),
    // PortalParent's: Buff_Elemental_Fire @12524
    ("ExitPortalParent_C", &[c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire")]),
    // Buff_Perk_SpeedUp_Medium @2519 / @2674
    ("FinnHighFiveListener_C", &[c!("Blueprints/Perks/Buffs/MoveSpeed", "Buff_Perk_SpeedUp_Medium")]),
    // FinnTreasureBuff @121 / @929
    ("FinnTreasure_C", &[c!("Characters/Finn/BluePrints", "FinnTreasureBuff")]),
    // Buff_ProjectileBlock @1628 / @2573 / @150
    ("Finn_Shop_ArmorBuns_Buff_C", &[c!("Blueprints/Abilities/Shield", "Buff_ProjectileBlock")]),
    // Finn_Shop_Boots_Buff_Ally @4563, Finn_Shop_ArmorBuns_Buff @4544 / @10251, Finn_Shop_Boots_Buff @5324 / @10270, Bmo_Chop_Buff @10232
    (
        "Finn_Shop_C",
        &[
            c!("Characters/Finn/BluePrints", "Finn_Shop_Boots_Buff_Ally"),
            c!("Characters/Finn/BluePrints", "Finn_Shop_ArmorBuns_Buff"),
            c!("Characters/Finn/BluePrints", "Finn_Shop_Boots_Buff"),
            c!("Characters/Finn/BluePrints", "Bmo_Chop_Buff"),
        ],
    ),
    // Buff_MoveToPosition_LassoGrapple @4313 / @4397
    ("FishingLure_C", &[c!("Blueprints/Abilities/Shield", "Buff_MoveToPosition_LassoGrapple")]),
    // Buff_GarnetFullPowerGauntlets @3821, Buff_GarnetElecInsideBubble @4384 / @4867, Buff_GarnetElecOutsideBubble @5137
    (
        "GarnetElectricZone_C",
        &[
            c!("Characters/Garnet/Blueprints", "Buff_GarnetFullPowerGauntlets"),
            c!("Characters/Garnet/Blueprints", "Buff_GarnetElecInsideBubble"),
            c!("Characters/Garnet/Blueprints", "Buff_GarnetElecOutsideBubble"),
        ],
    ),
    // Buff_HealOverTime @1286 / @1676 / @3825 / @5106
    ("HealOverTimeZoneBP_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_HealOverTime")]),
    // Buff_Weakened @842, Buff_DelayKnockUp @6944
    (
        "Item_BatarangV2_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_Weakened"),
            c!("Blueprints/Abilities/DamageOverTime", "Buff_DelayKnockUp"),
        ],
    ),
    // Buff_AttachToAllyHorse @661 / @18812
    ("JakeHorse_Actor3_C", &[c!("Blueprints/Buffs/Navigation/BL_Jake", "Buff_AttachToAllyHorse")]),
    // Buff_Elemental_Iceproof @216 / @2800 / @23410, Buff_Elemental_Fireproof @312 / @2896 / @23506, Buff_Cheese @1128 / @3543, Buff_SpeedUp @3680
    (
        "Jerry_SpawnedActor_C",
        &[
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Iceproof"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fireproof"),
            c!("Blueprints/Abilities/Shield", "Buff_Cheese"),
            c!("Blueprints/Abilities/Shield", "Buff_SpeedUp"),
        ],
    ),
    // Jerry_SpawnedActor's: Buff_Elemental_Iceproof @216 / @2800 / @23410, Buff_Elemental_Fireproof @312 / @2896 / @23506, Buff_Cheese @1128 / @3543, Buff_SpeedUp @3680
    (
        "Jerry_SpawnedActor_TempInvincibility_C",
        &[
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Iceproof"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fireproof"),
            c!("Blueprints/Abilities/Shield", "Buff_Cheese"),
            c!("Blueprints/Abilities/Shield", "Buff_SpeedUp"),
        ],
    ),
    // Buff_MoveToPosition_LassoGrapple @1988 / @2072, Buff_ArmorPack @14652, Buff_MoveCompTo_WithStun @15013
    (
        "LassoProjectileFastV3_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_MoveToPosition_LassoGrapple"),
            c!("Blueprints/Abilities/Shield", "Buff_ArmorPack"),
            c!("Blueprints/Abilities/Shield", "Buff_MoveCompTo_WithStun"),
        ],
    ),
    // Buff_FearStack @201 / @2349, Buff_ScoobySnack @3821 / @11632, Buff_MoveToPosition @6586 / @6674, Buff_Weakened @12585
    (
        "MarkTeleportProjectile_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_FearStack"),
            c!("Blueprints/Abilities/Shield/BL_Shaggy", "Buff_ScoobySnack"),
            c!("Blueprints/Abilities/Shield", "Buff_MoveToPosition"),
            c!("Blueprints/Abilities/Shield", "Buff_Weakened"),
        ],
    ),
    // Buff_TurnEnemyAwayOnHit @575 / @8866, Buff_AryaBackStab @8553, Buff_ScoobySnack @11246
    (
        "Pawn_Arya_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_TurnEnemyAwayOnHit"),
            c!("Blueprints/Abilities/Shield/BL_Arya", "Buff_AryaBackStab"),
            c!("Blueprints/Abilities/Shield/BL_Shaggy", "Buff_ScoobySnack"),
        ],
    ),
    // Buff_SpeedUp @5148, Buff_DecrementEvadeCount @5290
    (
        "Pawn_BatmanV2_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_SpeedUp"),
            c!("Blueprints/Abilities/DamageOverTime", "Buff_DecrementEvadeCount"),
        ],
    ),
    // C015ControllingDogpile @3982 / @4116, Buff_C015Tornado @5651 / @5719, Buff_C015Eating @9988 / @10056, Buff_EatenByC015 @9207 / @14120 / @869, Buff_AllyTornadoed @15259 / @15489, Buff_TastyOnHit @17782, Buff_TastySpeedBoost @100 / @268 / @864 / @940
    (
        "Pawn_C015_C",
        &[
            c!("Characters/C015/Blueprints", "C015ControllingDogpile"),
            c!("Characters/C015/Blueprints", "Buff_C015Tornado"),
            c!("Characters/C015/Blueprints", "Buff_C015Eating"),
            c!("Characters/C015/Blueprints", "Buff_EatenByC015"),
            c!("Characters/C015/Blueprints", "Buff_AllyTornadoed"),
            c!("Characters/C015/Blueprints", "Buff_TastyOnHit"),
            c!("Characters/C015/Blueprints", "Buff_TastySpeedBoost"),
        ],
    ),
    // Buff_C018ProjectileWarpEnemy @2667, Buff_C018ReverseProjectile @46
    (
        "Pawn_C018_C",
        &[
            c!("Characters/C018/Blueprints/Buffs", "Buff_C018ProjectileWarpEnemy"),
            c!("Characters/C018/Blueprints/Buffs", "Buff_C018ReverseProjectile"),
        ],
    ),
    // FinnTreasureBuff @1417 / @4606 / @9418 / @9613
    ("Pawn_Finn_C", &[c!("Characters/Finn/BluePrints", "FinnTreasureBuff")]),
    // Buff_GarnetStopProjectile @926 / @1025 / @9074 / @9353, Buff_SafeMoveToPosition @6786, Buff_GarnetStackingSinging @145 / @326 / @520 / @596
    (
        "Pawn_Garnet_C",
        &[
            c!("Characters/Garnet/Blueprints", "Buff_GarnetStopProjectile"),
            c!("Blueprints/Abilities/Shield", "Buff_SafeMoveToPosition"),
            c!("Characters/Garnet/Blueprints", "Buff_GarnetStackingSinging"),
        ],
    ),
    // ReindogSpeedBoost @235 / @4672 / @4811
    ("Pawn_Reindog_C", &[c!("Characters/Creature/Blueprints", "ReindogSpeedBoost")]),
    // Buff_StevenWatermelonDebuff @3737, Buff_Steven_GreenThumbDamageBoost @4005
    (
        "Pawn_StevenUniverse_C",
        &[
            c!("Blueprints/Abilities/Shield/BL_Steven", "Buff_StevenWatermelonDebuff"),
            c!("Blueprints/Perks/Buffs/Steven", "Buff_Steven_GreenThumbDamageBoost"),
        ],
    ),
    // Buff_IsPoliceCar @3051, Buff_Arrested @385 / @469
    (
        "Pawn_VelmaAIBotPolice_C",
        &[c!("Characters/Velma/BluePrints", "Buff_IsPoliceCar"), c!("Blueprints/Abilities/Shield", "Buff_Arrested")],
    ),
    // Buff_EvidenceCount @5369 / @5519 / @8749 / @8913
    ("Pawn_Velma_C", &[c!("Blueprints/Abilities/Shield", "Buff_EvidenceCount")]),
    // Buff_C016_Ball_Conditional @35 / @1231 / @1314 (the class's name: its package is Pawn_c016; the passive buff
    // manager's row names the buff first all the same)
    ("Pawn_C016_C", &[c!("Characters/c016/Blueprints", "Buff_c016_Ball_Conditional", "Buff_C016_Ball_Conditional_C")]),
    // Buff_HealOverTime @132 / @216
    ("PieProjectile_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_HealOverTime")]),
    // Buff_Enlarge @949, Buff_Polymorph_ProjectileEnlarge_WithHieghtChange @2348, Buff_Polymorph_ProjectileEnlarge @2452
    (
        "PolymorphGrenade_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_Enlarge"),
            c!("Blueprints/Abilities/Shield", "Buff_Polymorph_ProjectileEnlarge_WithHieghtChange"),
            c!("Blueprints/Abilities/Shield", "Buff_Polymorph_ProjectileEnlarge"),
        ],
    ),
    // Buff_SpeedUp @577, Buff_SpeedUpProjectile @947
    (
        "ProjectileAccelerator_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_SpeedUp"),
            c!("Blueprints/Abilities/Shield", "Buff_SpeedUpProjectile"),
        ],
    ),
    // Buff_Damage_Electric @6205, Buff_AllyElectricGauntlets @6397 / @6898
    (
        "SeismicReactor_C",
        &[
            c!("Blueprints/Buffs/Elemental", "Buff_Damage_Electric"),
            c!("Blueprints/Abilities/Shield/BL_Superman", "Buff_AllyElectricGauntlets"),
        ],
    ),
    // ShaggySandwichProjectile's: Buff_HealOverTime @3119 / @3203
    ("ShaggyRageSandwichProjectile_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_HealOverTime")]),
    // Buff_HealOverTime @3119 / @3203
    ("ShaggySandwichProjectile_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_HealOverTime")]),
    // Buff_HealOverTime @1842
    ("ScoobySnackBox_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_HealOverTime")]),
    // Buff_SlowIntoStun @3055 / @3418 / @3506 / @158
    ("SlowtoStunGrenade_C", &[c!("Blueprints/Abilities/Shield", "Buff_SlowIntoStun")]),
    // Buff_Invisibility_Fog @145 / @481, Buff_SpeedUp @1271 / @1182, Buff_Invulnerable @1406, Buff_DecrementEvadeCount @1541 / @834, Buff_SmokeGrenadeProjectileBlock @1676 / @1974 / @1008
    (
        "SmokeGrenadeOcclusionBP_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_Invisibility_Fog"),
            c!("Blueprints/Abilities/Shield", "Buff_SpeedUp"),
            c!("Blueprints/Abilities/Shield", "Buff_Invulnerable"),
            c!("Blueprints/Abilities/DamageOverTime", "Buff_DecrementEvadeCount"),
            c!("Blueprints/Abilities/Shield", "Buff_SmokeGrenadeProjectileBlock"),
        ],
    ),
    // TetherProjectile's: Buff_StunnedMoveToPositionThroughGeo @1759 / @1865 / @3050 / @3156
    ("SuperSaveTetherProjectile_C", &[c!("Blueprints/Abilities/Shield", "Buff_StunnedMoveToPositionThroughGeo")]),
    // Buff_HeldByEnemy @3290
    ("SupermanFlightHit_C", &[c!("Blueprints/Buffs/Navigation", "Buff_HeldByEnemy")]),
    // Buff_HeldByEnemy @993
    ("SupermanGrabListener_C", &[c!("Blueprints/Buffs/Navigation", "Buff_HeldByEnemy")]),
    // SupermanGroundLazer's: Buff_Fire @3355
    ("SupermanGroundLazerAir_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")]),
    // Buff_Fire @3355
    ("SupermanGroundLazer_C", &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")]),
    // Superman_IceBreath's: Buff_IceSlowProjectile @1870, Buff_SupermanAllyIceGauntlet @3349
    (
        "Superman_IceBreathEducated_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_IceSlowProjectile"),
            c!("Blueprints/Abilities/Shield/BL_Superman", "Buff_SupermanAllyIceGauntlet"),
        ],
    ),
    // Buff_IceSlowProjectile @1870, Buff_SupermanAllyIceGauntlet @3349
    (
        "Superman_IceBreath_C",
        &[
            c!("Blueprints/Abilities/Shield", "Buff_IceSlowProjectile"),
            c!("Blueprints/Abilities/Shield/BL_Superman", "Buff_SupermanAllyIceGauntlet"),
        ],
    ),
    // Buff_StunnedMoveToPositionThroughGeo @1759 / @1865 / @3050 / @3156
    ("TetherProjectile_C", &[c!("Blueprints/Abilities/Shield", "Buff_StunnedMoveToPositionThroughGeo")]),
    // Buff_Velma_Carry_Speed @547 / @5410, Buff_VelmaAttachToAlly @2959 / @7254 / @14785 / @17466, Buff_ArmorPack @7432 / @7538
    (
        "VelmaCarryRun_Actor_C",
        &[
            c!("Characters/Velma/Velma_BP", "Buff_Velma_Carry_Speed"),
            c!("Blueprints/Buffs/BL_Velma", "Buff_VelmaAttachToAlly"),
            c!("Blueprints/Abilities/Shield", "Buff_ArmorPack"),
        ],
    ),
    // Buff_SpeedUp @7103, Buff_Weakened @8052
    (
        "VelmaMarkProjectile_C",
        &[c!("Blueprints/Abilities/Shield", "Buff_SpeedUp"), c!("Blueprints/Abilities/Shield", "Buff_Weakened")],
    ),
    // PortalParent's: Buff_Elemental_Fire @12524
    ("VisualOnlyPortal_C", &[c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire")]),
    // Buff_NullifyNextHit @999 / @3331, Buff_Cleanse @1109
    (
        "WW_BracerShield_C",
        &[c!("Blueprints/Abilities/Shield", "Buff_NullifyNextHit"), c!("Blueprints/Abilities/Shield", "Buff_Cleanse")],
    ),
]);
/// Where a mode's buffs are listed: the FFA game mode's class default object and its member.
pub struct ModeBuffsFrom {
    pub cdo: &'static str,
    pub member: &'static str,
}

/// The buffs the FFA mode's respawn catch-up applies.
pub const MODE_BUFFS_FROM: ModeBuffsFrom = ModeBuffsFrom {
    cdo: "/Game/Panda_Main/Blueprints/GameModes/FFAGameMode.Default__FFAGameMode_C",
    member: "RespawnCatchUpBuffs",
};
