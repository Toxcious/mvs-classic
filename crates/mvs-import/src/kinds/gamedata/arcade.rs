//! The arcade buff classes' defaults: the buffs an arcade round gives its bots
//! (PGI_CoOpVsAIManager's Arcade Get Round Buff through Comp_Pawn_PassiveBuffManager ApplyArcadeModeBuffs) and the
//! classes their graphs spawn (Buff_Arcade_Secret_Boss: Buff_ReindogIce, Buff_Arcade_Enlarge; Buff_Enlarge's graph,
//! which Buff_Arcade_Enlarge runs: the two projectile enlarge buffs). Each its class defaults, bound as the fighter
//! data's buffs are (ActorClasses::class_defaults).

use mvs_data::ArcadeBuffs;

use super::actor_class::ActorClasses;
use super::reach::c;
use super::shape::short_name;
use crate::kind::{Cx, KindError};

/// Arcade Get Round Buff's classes (AXFunctions @500-2227), then those the Secret_Boss graph spawns (@559, @1084), then
/// those Buff_Enlarge's graph gives the enraged boss's projectiles (Buff_Enlarge @1712-2361).
pub const ARCADE_BUFF_CLASSES: [&str; 11] = [
    c!("Blueprints/Abilities/Shield", "Buff_AXBotAlly"),
    c!("Blueprints/Abilities/Shield", "Buff_AXBotEnemyHard"),
    c!("Blueprints/Abilities/Shield", "Buff_AXBotMediumNemesis"),
    c!("Blueprints/Abilities/Shield", "Buff_AXBotEnemyHardNemesis"),
    c!("Blueprints/Arcade", "Buff_Arcade_Miniature"),
    c!("Blueprints/Arcade", "Buff_Arcade_MidBoss"),
    c!("Blueprints/Arcade", "Buff_Arcade_Secret_Boss"),
    c!("Blueprints/Arcade", "Buff_Arcade_Enlarge"),
    c!("Characters/Creature/Blueprints", "Buff_ReindogIce"),
    c!("Blueprints/Abilities/Shield", "Buff_Polymorph_ProjectileEnlarge_WithHieghtChange"),
    c!("Blueprints/Abilities/Shield", "Buff_Polymorph_ProjectileEnlarge"),
];

/// The arcade buff classes' defaults by class name (`Buff_AXBotAlly_C`).
pub fn arcade(cx: &Cx<'_>) -> Result<ArcadeBuffs, KindError> {
    let classes = ActorClasses::new(*cx, "arcade buffs");
    let mut out = ArcadeBuffs::new();
    for p in ARCADE_BUFF_CLASSES {
        out.insert(short_name(p).to_owned(), classes.class_defaults(p)?);
    }
    Ok(out)
}
