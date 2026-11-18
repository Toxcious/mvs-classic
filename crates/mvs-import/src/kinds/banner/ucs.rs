//! The banner element classes' UserConstructionScripts (the banner bind's UCS arrays), read from each class's
//! UserConstructionScript bytecode (the object kind holds no script bytecode).
//!
//! Per class, by its object path (two packages each define a BP_Banner_Bravos_C and a BP_Banner_DeadMoon_C), the
//! statements of its UCS in order. Each is an EX_SetArray of a K2Node_MakeArray local (its null entries, EX_NoObject,
//! dropped) followed by an EX_Let of MaskedComponents / UnmaskedComponents / MirroredComponents from that local, or
//! the call of the parent's UCS where it stands (`parent`: the nearest parent class with a UCS, the call's owner;
//! Actor's, empty, is noted and not listed). Nothing else stands in these scripts. A class with a UCS and no row here
//! fails at bind; a class without one runs its parent's (BP_Banner_Template_C has none). Read for the 65 classes on
//! the released banners' chains (62 with a UCS).

/// The array a step sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UcsSet {
    Masked,
    Unmasked,
    Mirrored,
}

/// One statement of a UCS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UcsStep {
    /// The call of the parent class's UCS.
    Parent,
    /// An array set to these component names.
    Set(UcsSet, &'static [&'static str]),
}

use UcsSet::{Masked, Mirrored, Unmasked};
use UcsStep::{Parent, Set};

/// A class's UCS statements, by its object path.
pub fn ucs_of(class_path: &str) -> Option<&'static [UcsStep]> {
    UCS.iter().find(|(p, _)| *p == class_path).map(|(_, s)| *s)
}

#[rustfmt::skip]
static UCS: &[(&str, &[UcsStep])] = &[
    (
        "/Game/Panda_Main/PreMatch/Banners/Arkham_Asylum/BP_Banner_Arkham_Asylum.BP_Banner_Arkham_Asylum_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer3", "Layer4", "Layer5", "Layer6"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Banner_HouseTargaryen_01/BP_Banner_BannerHouseTargaryen_01.BP_Banner_BannerHouseTargaryen_01_C",
        &[
            Set(Masked, &["Layer1"]),
            Set(Unmasked, &["Frame"]),
            Set(Mirrored, &["Frame"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Batman_Icon/BP_Banner_Batman_Icon.BP_Banner_Batman_Icon_C",
        &[
            Set(Masked, &["BG", "Layer1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Bravos/BP_Banner_Bravos.BP_Banner_Bravos_C",
        &[
            Set(Masked, &["BG", "Overlay1", "Overlay2", "Overlay3"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Bugs_Bunny_Tunnel/BP_Banner_Bugs_Bunny_Tunnel.BP_Banner_Bugs_Bunny_Tunnel_C",
        &[
            Set(Masked, &["BG", "Layer1", "Layer2", "Layer3", "Layer4", "Layer5", "Layer6", "Layer7"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/CheesyTemptations/BP_Banner_Cheesy_Temptations.BP_Banner_Cheesy_Temptations_C",
        &[
            Set(Masked, &["BG", "Layer3", "Layer2", "Layer1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Daily_Planet/BP_Banner_Daily_Planet.BP_Banner_Daily_Planet_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer3"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/DangerHighVoltage/BP_Banner_DangerHighVoltage.BP_Banner_DangerHighVoltage_C",
        &[
            Set(Masked, &["BG", "Overlay 1", "Overlay 2", "Overlay 3"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Default/BP_Banner_Default.BP_Banner_Default_C",
        &[
            Set(Masked, &["Layer1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/EVO_2022/BP_Banner_EVO2022.BP_Banner_EVO_2022_C",
        &[
            Set(Masked, &["BG", "Layer 1", "Layer 2"]),
            Set(Unmasked, &["Frame"]),
            Set(Mirrored, &["Frame"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/EncoreEncore/BP_Banner_Bravos.BP_Banner_Bravos_C",
        &[
            Set(Masked, &["BG", "Overlay1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/ExtraToastie/BP_Banner_ExtraToastie.BP_Banner_ExtraToastie_C",
        &[
            Parent,
            Set(Masked, &["Background", "Layer2", "PS_Stars"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &["Background", "Layer2", "PS_Stars"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Fire/BP_Banner_Fire.BP_Banner_Fire_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer3", "Layer4", "Layer5", "Layer6", "Layer7", "Layer8", "Layer9"]),
            Set(Unmasked, &["Layer10"]),
            Set(Mirrored, &["Layer10"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/FortressOfSolitude/BP_Banner_FortressOfSolitude.BP_Banner_FortressOfSolitude_C",
        &[
            Set(Masked, &["BG", "Overlay1", "Overlay2"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Founders/BP_Banner_Founders_Epic.BP_Banner_Founders_Epic_C",
        &[
            Set(Masked, &["Layer1", "PS_Toast_Floating", "Layer3", "PS_BAN_Toast_Glow", "Layer2", "PS_BAN_Toast_Sparkle"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &["PS_Toast_Floating"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Founders/BP_Banner_Founders_Legendary.BP_Banner_Founders_Legendary_C",
        &[
            Set(Masked, &["Layer1", "PS_Toast_Floating", "Layer3", "Layer2", "Layer4", "PS_BAN_Toast_Godray", "PS_BAN_Toast_Sparkle"]),
            Set(Unmasked, &["PS_BAN_Toast_Smoke"]),
            Set(Mirrored, &["PS_Toast_Floating", "PS_BAN_Toast_Smoke", "PS_BAN_Toast_Sparkle"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Founders/BP_Banner_Founders_Rare.BP_Banner_Founders_Rare_C",
        &[
            Set(Masked, &["Layer1", "Layer4", "PS_Toast_Floating"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &["PS_Toast_Floating"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Full_Court/BP_Banner_Full_Court.BP_Banner_Full_Court_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer3", "Layer4"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/GhostInThemThereHills/BP_Banner_GhostInThemThereHills.BP_Banner_GhostInThemThereHills_C",
        &[
            Set(Masked, &["BG", "Overlay 1", "PS_DustSweep", "PS_DustSweep1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/GhostInThemThereHills/BP_Banner_GhostInThemThereHills_Gold.BP_Banner_GhostInThemThereHills_Gold_C",
        &[
            Set(Masked, &["BG", "Overlay 1", "PS_DustSweep"]),
            Set(Unmasked, &["Frame"]),
            Set(Mirrored, &["Frame"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Groovy/BP_Banner_Groovy.BP_Banner_Groovy_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer3", "Layer4"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Halloween_DeadMoon/BP_Banner_DeadMoon.BP_Banner_DeadMoon_C",
        &[
            Set(Masked, &["Layer1", "PS_Glow", "Layer3", "Layer2"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Halloween_GhoulCave/BP_Banner_GhoulCave.BP_Banner_GhoulCave_C",
        &[
            Set(Masked, &["Layer1", "PS_Smoke", "Layer3"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Halloween_Scarecrow/BP_Banner_Scarecrow.BP_Banner_Scarecrow_C",
        &[
            Set(Masked, &["Layer1", "PS_Glow", "Layer3", "Layer2"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Harley_Hienna/BP_Banner_Harley_Hienna.BP_Banner_Harley_Hienna_C",
        &[
            Set(Masked, &["BG", "Smoke"]),
            Set(Unmasked, &["Frame", "Hienna 1", "Hienna 2"]),
            Set(Mirrored, &["Frame"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/HeartZenifeer/BP_Banner_HeartZanifeer.BP_Banner_HeartZanifeer_C",
        &[
            Set(Masked, &["BG", "Overlay1", "Overlay2"]),
            Set(Unmasked, &["Frame"]),
            Set(Mirrored, &["Frame", "Overlay1", "Overlay2"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Holiday_FirePlace/BP_Banner_HolidayFireplace.BP_Banner_HolidayFireplace_C",
        &[
            Set(Masked, &["Layer1", "PS_Glow", "Layer3", "Layer4", "Layer5"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Holiday_SlushyPlushy/BP_Banner_SlushyPlushy.BP_Banner_SlushyPlushy_C",
        &[
            Set(Masked, &["Layer1", "PS_Snowman_Stars", "Layer3", "Layer4"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Holiday_SnowmanFirstGames/BP_Banner_SnowFirstGames.BP_Banner_SnowFirstGames_C",
        &[
            Set(Masked, &["Layer1", "PS_Snowman_Stars", "Layer3", "Layer4", "Layer5", "Layer6"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Hunted_Mansion/BP_Banner_Hunted_Mansion.BP_Banner_Hunted_Mansion_C",
        &[
            Set(Masked, &["BG", "Layer1", "Layer2"]),
            Set(Unmasked, &["Layer3", "Layer4", "Layer5", "Layer6"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/IceKing/BP_Banner_IceKing.BP_Banner_IceKing_C",
        &[
            Set(Masked, &["BG", "Layer 1", "Layer 2", "Layer 3", "BG1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/IceKingdom/BP_Banner_IceKingdom.BP_Banner_IceKingdom_C",
        &[
            Set(Masked, &["BG", "Layer 1", "Layer 2", "Layer 3", "Layer 4"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/IronGiant/BP_Banner_IronGiant.BP_Banner_IronGiant_C",
        &[
            Set(Masked, &["BG", "Layer 1", "Layer 2", "Layer 3", "Layer 4"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/IronGiantBluePrint/BP_Banner_IronGiantBlueprint.BP_Banner_IronGiantBlueprint_C",
        &[
            Set(Masked, &["BG", "Layer 1", "Layer 2", "Layer 3"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/ItsHarleyQuinnTime/BP_Banner_HarleyTime.BP_Banner_HarleyTime_C",
        &[
            Set(Masked, &["BG", "Layer 1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/KnightBanner/BP_Banner_Knight.BP_Banner_Knight_C",
        &[
            Set(Masked, &["Background", "PS_KnightBanner_Fog1", "KnightBanner", "PS_KnightBanner_Fog"]),
            Set(Unmasked, &["PS_SmokeHaze", "Chains"]),
            Set(Mirrored, &["Chains"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/LadyRainicornsTrail/BP_Banner_LadyRainicorn.BP_Banner_LadyRainicorn_C",
        &[
            Set(Masked, &["BG", "Layer 1", "Layer 2", "Layer 3"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/LannisterBanner/BP_Banner_LannisterBanner.BP_Banner_LannisterBanner_C",
        &[
            Set(Masked, &["BG", "Layer 1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Loony_Tunes/BP_Banner_Loony_Tunes.BP_Banner_Loony_Tunes_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer3", "Layer4", "Layer5", "Layer6", "Layer7", "Layer8", "Layer9"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/LoveStorm/BP_Banner_LoveStorm.BP_Banner_LoveStorm_C",
        &[
            Parent,
            Set(Masked, &["Background"]),
            Set(Mirrored, &["Background"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/LunarNewYear23/BP_Banner_LunarNewYear23.BP_Banner_LunarNewYear23_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "PS_LunarNewYear_Glow"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Lunar_Spire/BP_Banner_Lunar_Spire.BP_Banner_Lunar_Spire_C",
        &[
            Set(Masked, &["BG", "Layer1", "Layer2", "Layer3"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/PSPlus_Rare/BP_Banner_PSPlusRare.BP_Banner_PSPlusRare_C",
        &[
            Set(Masked, &["Layer1", "PS_Glow", "Layer3", "Layer4"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Reward/BP_Banner_Reward.BP_Banner_Reward_C",
        &[
            Set(Masked, &["BG", "Stars", "Layer1"]),
            Set(Unmasked, &["Layer2"]),
            Set(Mirrored, &["Layer1", "Layer2"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Scoobtober_Castle/BP_Banner_Castle.BP_Banner_Castle_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer4", "PS_Bats"]),
            Set(Unmasked, &["Layer4"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Scoobtober_GhostlyNight/BP_Banner_GhostlyNight.BP_Banner_GhostlyNight_C",
        &[
            Set(Masked, &["Layer1", "Layer2"]),
            Set(Unmasked, &["Layer3", "Layer4"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Scooby_Icon/BP_Banner_Scooby_Icon.BP_Banner_Scooby_Icon_C",
        &[
            Set(Masked, &["BG", "Layer1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/StarkBanner/BP_Banner_StarkBanner.BP_Banner_StarkBanner_C",
        &[
            Set(Masked, &["BG", "Layer 1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Stark_Icon/BP_Banner_Stark_Icon.BP_Banner_Stark_Icon_C",
        &[
            Set(Masked, &["BG", "Layer1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Steven_Planets/BP_Banner_Steven_Planets.BP_Banner_Steven_Planets_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer3", "Layer4", "Layer5", "Layer6", "Layer7", "Layer8", "Layer9", "Layer10"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Superman_Icon/BP_Banner_Superman_Icon.BP_Banner_Superman_Icon_C",
        &[
            Set(Masked, &["BG", "Layer1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/TestAssets/BP_Banner_Base.BP_Banner_Base_C",
        &[
            Set(Masked, &["Background"]),
            Set(Unmasked, &["Border"]),
            Set(Mirrored, &["Border"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Themiscyra/BP_Banner_Themiscyra.BP_Banner_Themiscyra_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer3", "Layer4"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/ToolsOfTheTrade/BP_Banner_ToolsOfTheTrade.BP_Banner_ToolsOfTheTrade_C",
        &[
            Set(Masked, &["Layer1", "Layer2", "Layer3", "Layer4"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Tree_Fort/BP_Banner_Tree_Fort.BP_Banner_Tree_Fort_C",
        &[
            Set(Masked, &["BG", "Layer1", "Layer2", "Layer3", "Layer4", "Layer5", "Layer6", "Layer7", "Layer8", "Layer9", "Layer10"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Waterfall/BP_Banner_SparkleFalls_Legendary.BP_Banner_SparkleFalls_Legendary_C",
        &[
            Set(Masked, &["waterfall", "PS_WaterfallFoam"]),
            Set(Mirrored, &["waterfall"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Waterfall/BP_Banner_WaterfallStyle.BP_Banner_WaterfallStyle_C",
        &[
            Set(Masked, &["waterfall", "PS_WaterfallFoam"]),
            Set(Mirrored, &["waterfall"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Wayne_Tower/BP_Banner_Wayne_Tower.BP_Banner_Wayne_Tower_C",
        &[
            Set(Masked, &["BG", "Layer1", "Layer2", "Layer3", "Layer4", "Layer5", "Layer6", "Layer7"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/WhatsOperaDoc/BP_Banner_WhatsOperaDoc.BP_Banner_WhatsOperaDoc_C",
        &[
            Set(Masked, &["BG", "Layer 1", "Layer 2", "Layer 3"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/Wonderwoman_Icon/BP_Banner_Wonderwoman_Icon.BP_Banner_Wonderwoman_Icon_C",
        &[
            Set(Masked, &["BG", "Layer1"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/WorkInProgress/BP_Banner_WorkInProgress.BP_Banner_WorkInProgress_C",
        &[
            Set(Masked, &["BG", "Layer 1", "Layer 2", "Layer 3", "Layer 4"]),
        ],
    ),
    (
        "/Game/Panda_Main/PreMatch/Banners/ZanifeersLastHope/BP_Banner_ZanifeersLastHope.BP_Banner_ZanifeersLastHope_C",
        &[
            Set(Masked, &["BG", "Overlay 1", "Overlay 2"]),
            Set(Unmasked, &[]),
            Set(Mirrored, &[]),
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows() {
        assert_eq!(UCS.len(), 62);
        let p = "/Game/Panda_Main/PreMatch/Banners/LoveStorm/BP_Banner_LoveStorm.BP_Banner_LoveStorm_C";
        assert_eq!(ucs_of(p).unwrap(), [Parent, Set(Masked, &["Background"]), Set(Mirrored, &["Background"])]);
        assert!(ucs_of("/Game/None.None_C").is_none());
        let mut paths: Vec<_> = UCS.iter().map(|(p, _)| *p).collect();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), 62);
        let _ = Unmasked;
    }
}
