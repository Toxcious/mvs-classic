//! The port's own tables of its fighters, which the game kinds read beside the build (the fighter draws, ui,
//! particles and menuscene): which character of the build each playable key is (its CharacterData), the port's
//! filters over each key's build data, and the skin helpers (paths, the CharacterData, the
//! port's default skin).

use mvs_uasset::value::Value;

use crate::kind::{Cx, KindError};
use crate::objview::{ObjView, string};

/// A key's filters. Paths are package paths (`/Game/...`, no object name) unless a
/// field names a class (`pawn`).
#[derive(Clone, Copy, Debug)]
pub struct FighterPort {
    /// The character's folder; montages and sequences are found under it.
    pub folder: &'static str,
    /// Only these subfolders of `folder` (`.` the folder itself).
    pub anim_dirs: Option<&'static [&'static str]>,
    /// Subpaths left out, a trailing `/` a folder.
    pub anim_skip: Option<&'static [&'static str]>,
    /// Montages are found by class anywhere under the folder, not by the name pattern alone.
    pub montages_by_type: bool,
    /// Path prefixes of the BranchGroup data assets the character reads.
    pub branch_groups: Option<&'static [&'static str]>,
    /// A further base anim set the AnimBP `graph` blends with `animbp`'s (LeBron, Tom and Jerry, Stripe).
    pub anim_sets: Option<AnimSets>,
    /// The base anim set class (Superman's is Base_AnimBP_Superman, not his AnimClass's).
    pub animbp: &'static str,
    /// The bind writes the AnimBP's node kinds.
    pub anim_node_kinds: bool,
    /// Further skin assets of the same pawn, built beside the default.
    pub extra_skins: Option<&'static [&'static str]>,
    /// The default skin's package where it is not the CharacterData's IsDefaultSkin skin (or the key has none).
    pub default_skin: Option<&'static str>,
    /// A data-only key's CharacterData package (no roster entry names it).
    pub character_data: Option<&'static str>,
    /// A data-only key's pawn class (no CharacterData names it).
    pub pawn: Option<&'static str>,
    /// Folders below `folder` whose clips the boxes bind samples too, only those a montage's slot track names.
    pub box_dirs: Option<&'static [&'static str]>,
    /// A data-only key that shares this owner's folder: its clips are only those of its own `anim_dirs`.
    pub companion_of: Option<&'static str>,
}

/// `FighterPort.animSets`: the AnimBP that blends, and the further sets by name.
#[derive(Clone, Copy, Debug)]
pub struct AnimSets {
    pub graph: &'static str,
    pub sets: &'static [(&'static str, AnimSet)],
}

#[derive(Clone, Copy, Debug)]
pub struct AnimSet {
    pub animbp: &'static str,
    pub tag: &'static str,
}

/// The filters of `key` (a playable or a data-only key).
pub fn fighter_port(key: &str) -> Option<&'static FighterPort> {
    FIGHTER_PORT.iter().find(|(k, _)| *k == key).map(|(_, p)| p)
}

/// The CharacterData package of playable key `key` (`CHARACTER_DATA[key]`).
pub fn character_data(key: &str) -> Option<&'static str> {
    CHARACTER_DATA.iter().find(|(k, _)| *k == key).map(|(_, p)| *p)
}

// paths

/// The package of an object path, cut at the first `.` after its last `/` (`/Game/A/B.B:C` ->
/// `/Game/A/B`).
pub fn pkg_of(path: &str) -> &str {
    let slash = path.rfind('/').unwrap_or(0);
    path[slash..].find('.').map_or(path, |d| &path[..slash + d])
}

/// A package's asset name (`/Game/A/B.B` or `/Game/A/B` -> `B`).
pub fn asset_name(path: &str) -> &str {
    let p = pkg_of(path);
    &p[p.rfind('/').map_or(0, |i| i + 1)..]
}

/// A package's asset object (`/Game/A/B` -> `/Game/A/B.B`).
pub fn asset_of(pkg: &str) -> String {
    format!("{}.{}", pkg_of(pkg), asset_name(pkg))
}

// skins

/// A key's CharacterData object, `None` for a key without one.
pub fn character_data_of<'a>(cx: &Cx<'a>, character_data: Option<&str>) -> Result<Option<ObjView<'a>>, KindError> {
    character_data.map(|p| cx.object(&asset_of(p))).transpose()
}

/// The default skin's package: the port table's `default_skin`, else the one skin of the
/// CharacterData's Skins the build holds whose IsDefaultSkin is true (its package as the build spells it: LeBron's
/// c016, which his CharacterData names C016).
pub fn port_default_skin(
    cx: &Cx<'_>,
    key: &str,
    port: &FighterPort,
    cd: Option<&ObjView<'_>>,
) -> Result<String, KindError> {
    if let Some(d) = port.default_skin {
        return Ok(d.to_owned());
    }
    let Some(cd) = cd else {
        return Err(KindError::Failed(format!(
            "{key}: no CharacterData and no defaultSkin in the port's fighter table"
        )));
    };
    let skins: Vec<Option<String>> = match cd.prop("Skins") {
        Some(Value::Array(a)) => a.iter().map(|v| string(cx.cache, v)).collect(),
        _ => Vec::new(),
    };
    let mut defaults: Vec<String> = Vec::new();
    for s in &skins {
        let Some(s) = s else {
            return Err(KindError::Failed(format!("{key}: a Skins entry of {} is no reference", cd.path())));
        };
        if !cx.exists(pkg_of(s)) {
            continue;
        }
        let o = cx.object(s)?;
        if matches!(o.prop("IsDefaultSkin"), Some(Value::Bool(true))) {
            defaults.push(pkg_of(o.path()).to_owned());
        }
    }
    if defaults.len() != 1 {
        return Err(KindError::Failed(format!("{key}: {} skins with IsDefaultSkin in {}", defaults.len(), cd.path())));
    }
    Ok(defaults.remove(0))
}

/// The port's own: per key, the BlendSpacePlayer nodes its drawing runs, as (AnimBP class, node); the fighter kind
/// binds them into its entry's `blendSpaces` (keyed `<AnimBP class>:<node>`), the view names them by its own names
pub const BLEND_SPACES: &[(&str, &[(&str, &str)])] = &[
    // Batman_AnimBP: the glide stack under BlendListByBool(IsGliding): GlideLoop and GlideAdd
    (
        "batman",
        &[(BATMAN_ANIMBP, "AnimGraphNode_BlendSpacePlayer_1"), (BATMAN_ANIMBP, "AnimGraphNode_BlendSpacePlayer")],
    ),
    // C017_AnimBP: the machine HoverState's two states and the additive input of ApplyAdditive_3
    // (BlendSpacePlayer_2, C017_LaserEye_Look_BlendSpace, is not drawn)
    (
        "irongiant",
        &[
            (IRON_GIANT_ANIMBP, "AnimGraphNode_BlendSpacePlayer"),
            (IRON_GIANT_ANIMBP, "AnimGraphNode_BlendSpacePlayer_1"),
            (IRON_GIANT_ANIMBP, "AnimGraphNode_BlendSpacePlayer_3"),
        ],
    ),
    // JakeHorse_Actor3's mesh AnimBP: its machine's Run state
    (
        "jake",
        &[(
            "/Game/Panda_Main/Characters/Jake/SubMeshes/JakeHorse/Jake_Horse_Skeleton_AnimBlueprint.Jake_Horse_Skeleton_AnimBlueprint_C",
            "AnimGraphNode_BlendSpacePlayer",
        )],
    ),
    (
        "superman",
        &[(
            "/Game/Panda_Main/Characters/Superman/Animations/Superman_AnimBP.Superman_AnimBP_C",
            "AnimGraphNode_BlendSpacePlayer",
        )],
    ),
];

const BATMAN_ANIMBP: &str = "/Game/Panda_Main/Characters/BatmanV2/Animations/Batman_AnimBP.Batman_AnimBP_C";
const IRON_GIANT_ANIMBP: &str = "/Game/Panda_Main/Characters/C017/Animations/C017_AnimBP.C017_AnimBP_C";

/// The BLEND_SPACES nodes of `key` (none: empty).
pub fn blend_spaces(key: &str) -> &'static [(&'static str, &'static str)] {
    BLEND_SPACES.iter().find(|(k, _)| *k == key).map_or(&[], |(_, n)| n)
}

// the tables, written out by hand

/// The port's playable characters by key, in roster order.
pub const CHARACTERS: &[&str] = &mvs_protocol::roster::CHARACTERS;

/// The CharacterData class.
pub const CHARACTER_DATA_CLASS: &str = "/Script/MultiVersus.CharacterData";

/// Each playable character's CharacterData package, in roster order.
pub const CHARACTER_DATA: &[(&str, &str)] = &[
    ("shaggy", "/Game/Panda_Main/Characters/Shaggy/character_shaggy"),
    ("morty", "/Game/Panda_Main/Characters/C019/character_C019"),
    ("irongiant", "/Game/Panda_Main/Characters/C017/character_C017"),
    ("wonderwoman", "/Game/Panda_Main/Characters/WonderWomanV2/character_wonder_woman"),
    ("harley", "/Game/Panda_Main/Characters/HarleyQuinn/character_harley"),
    ("superman", "/Game/Panda_Main/Characters/Superman/character_superman"),
    ("lebron", "/Game/Panda_Main/Characters/c016/character_c016"),
    ("arya", "/Game/Panda_Main/Characters/Arya/character_arya"),
    ("batman", "/Game/Panda_Main/Characters/Batman/Character_Batman"),
    ("creature", "/Game/Panda_Main/Characters/Creature/character_creature"),
    ("blackadam", "/Game/Panda_Main/Characters/C021/character_C021"),
    ("bugs", "/Game/Panda_Main/Characters/BugsBunnyV2/Character_BugsBunny"),
    ("rick", "/Game/Panda_Main/Characters/C020/character_C020"),
    ("taz", "/Game/Panda_Main/Characters/C015/character_C015"),
    ("steven", "/Game/Panda_Main/Characters/Steven/Character_Steven"),
    ("garnet", "/Game/Panda_Main/Characters/Garnet/CharData/character_garnet"),
    ("finn", "/Game/Panda_Main/Characters/Finn/character_Finn"),
    ("jake", "/Game/Panda_Main/Characters/Jake/character_jake"),
    ("tomandjerry", "/Game/Panda_Main/Characters/TomAndJerry/character_TomAndJerry"),
    ("velma", "/Game/Panda_Main/Characters/Velma/Character_Velma"),
    ("marvin", "/Game/Panda_Main/Characters/C018/Character_C018"),
    ("stripe", "/Game/Panda_Main/Characters/C023B/character_C023B"),
    ("gizmo", "/Game/Panda_Main/Characters/C023A/character_C023A"),
];

/// The port's filters over each key's build data, in its order.
pub const FIGHTER_PORT: &[(&str, FighterPort)] = &[
    (
        "shaggy",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Shaggy",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Blueprints/Characters/BranchGroups/BranchGroup_Shaggy"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Shaggy/Base_AnimBP_Shaggy",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "morty",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/C019",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Characters/C019/Branchgroups/BranchGroup_C019"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/C019/Animations/Base_AnimBP_c019",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    // Deviation: the Classic skin stays the port's default (a user decision); the build's IsDefaultSkin is C017_S01.
    (
        "irongiant",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/C017",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Characters/C017/BranchGroups/BranchGroup_C017"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/C017/Animations/Base_AnimBP_C017",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: Some("/Game/Panda_Main/Characters/C017/Skins/S00/C017_S00"),
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "wonderwoman",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/WonderWomanV2",
            anim_dirs: Some(&["Animations", "OldLasso", "."]),
            anim_skip: None,
            montages_by_type: true,
            branch_groups: Some(&[
                "/Game/Panda_Main/Blueprints/Characters/BranchGroups/WonderWomanV2/BranchGroup_WonderWoman",
            ]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/WonderWomanV2/Base_AnimBP_WonderWoman",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "harley",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/HarleyQuinn",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Characters/HarleyQuinn/BranchGroups/BranchGroup_Harley"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/HarleyQuinn/Animations/Base_AnimBP_HarleyQuinn",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "superman",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Superman",
            anim_dirs: Some(&["Animations", "."]),
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Blueprints/Characters/BranchGroups/Superman/BranchGroup_Superman"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Common/ANim/Base_AnimBP_Superman",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "lebron",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/c016",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Characters/c016/BranchGroups/BranchGroup_c016"]),
            anim_sets: Some(AnimSets {
                graph: "/Game/Panda_Main/Characters/c016/Animations/c016_AnimBP",
                sets: &[(
                    "noBall",
                    AnimSet {
                        animbp: "/Game/Panda_Main/Characters/c016/Animations/Base_AnimBP_NoBall_c016",
                        tag: "MovementAndHitReact",
                    },
                )],
            }),
            animbp: "/Game/Panda_Main/Characters/c016/Animations/Base_AnimBP_c016",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "arya",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Arya",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Blueprints/Characters/BranchGroups/Arya/BranchGroup_Arya"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Arya/Animations/Base_AnimBP_Arya",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "batman",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/BatmanV2",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Blueprints/Characters/BranchGroups/BranchGroup_Batman"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/BatmanV2/Animations/Base_AnimBP_Batman",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "creature",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Creature",
            anim_dirs: Some(&["Animation"]),
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Blueprints/Characters/BranchGroups/BranchGroup_RexCreature"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Creature/Animation/Base_AnimBP_Reindog",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "blackadam",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/C021",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: true,
            branch_groups: Some(&["/Game/Panda_Main/Characters/C021/BranchGroups/BranchGroup_C021"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/C021/Animations/Base_AnimBP_C021",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "bugs",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/BugsBunnyV2",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&[
                "/Game/Panda_Main/Blueprints/Characters/BranchGroups/BugsBunny/BranchGroup_BugsBunny",
                "/Game/Panda_Main/Blueprints/Characters/BranchGroups/BranchGroup_BugsBunny",
            ]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/BugsBunnyV2/Animations/Base_AnimBP_BugsBunny",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "rick",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/C020",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Characters/C020/BranchGroups/BranchGroup_C020"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/C020/Base_AnimBP_C020",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: Some(&["Skins/S02/Animations"]),
            companion_of: None,
        },
    ),
    (
        "taz",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/C015",
            anim_dirs: Some(&["Animations", "Chicken/Animations"]),
            anim_skip: Some(&[
                "Animations/TestFiles/",
                "Animations/Specials/Ground/C015_Tornado_Idle",
                "Chicken/Animations/Attacks/",
            ]),
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Characters/C015/BranchGroups/BranchGroup_C015"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/C015/Animations/Base_AnimBP_C015",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "steven",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Steven",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: true,
            branch_groups: Some(&[
                "/Game/Panda_Main/Blueprints/Characters/BranchGroups/StevenUniverse/BranchGroup_Steven",
            ]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Steven/Animations/Base_AnimBP_Steven",
            anim_node_kinds: false,
            extra_skins: Some(&["/Game/Panda_Main/Characters/Steven/WatermelonSteven"]),
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "garnet",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Garnet",
            anim_dirs: None,
            anim_skip: Some(&["Animations/Proto/", "Animations/SpecialAttacks/Old/"]),
            montages_by_type: true,
            branch_groups: Some(&[
                "/Game/Panda_Main/Characters/Garnet/BranchGroups/BranchGroup_GarnetBase",
                "/Game/Panda_Main/Characters/Garnet/BranchGroups/BranchGroup_GarnetAir",
                "/Game/Panda_Main/Characters/Garnet/BranchGroups/BranchGroup_GarnetSpecialAir",
            ]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Garnet/Animations/Base_AnimBP_Garnet",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "finn",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Finn",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Characters/Finn/BranchGroups/BranchGroup_Finn"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Finn/Animations/Base_AnimBP_Finn",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "jake",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Jake",
            anim_dirs: Some(&["Animation"]),
            anim_skip: Some(&[
                "Animation/Jake_Air_BatarangThrow_Montage",
                "Animation/Jake_Air_HeavyF_Montage",
                "Animation/Jake_Air_HeavyU_Montage",
                "Animation/Jake_Air_Heavy_DownSpike_Montage",
                "Animation/Jake_FlyingUppercut_Montage",
                "Animation/Jake_Special_JakeSuit_Montage",
            ]),
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Blueprints/Characters/BranchGroups/BranchGroup_Jake"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Jake/Animation/Base_AnimBP_Jake",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "tomandjerry",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/TomAndJerry",
            anim_dirs: None,
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&[
                "/Game/Panda_Main/Blueprints/Characters/BranchGroups/TomAndJerry/BranchGroup_TomAndJerry_",
            ]),
            anim_sets: Some(AnimSets {
                graph: "/Game/Panda_Main/Characters/TomAndJerry/Animations/Tom_and_Jerry_AnimBP",
                sets: &[(
                    "noJerry",
                    AnimSet {
                        animbp: "/Game/Panda_Main/Characters/TomAndJerry/Animations/Base_AnimBP_Tom_and_Jerry_No_Jerry",
                        tag: "MovementAndHitReact",
                    },
                )],
            }),
            animbp: "/Game/Panda_Main/Characters/TomAndJerry/Animations/Base_AnimBP_Tom_and_Jerry",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "velma",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Velma",
            anim_dirs: Some(&["Animation"]),
            anim_skip: Some(&[
                "Animation/BasicAttacks/Velma_Education_Montage",
                "Animation/BasicAttacks/Velma_Emote_Taunt1_Montage",
                "Animation/BasicAttacks/Velma_SummonPolice_Neutral_Montage",
                "Animation/Navigation/Velma_Nav_Crouch_Walk_Montage",
                "Animation/Special/Velma_Special_BookEquip_Montage",
                "Animation/Car/",
                "Animation/Jenkins/",
            ]),
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Blueprints/Characters/BranchGroups/Velma/BranchGroup_Velma"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Velma/Animation/Base_AnimBP_Velma",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "velmapolice",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/Velma",
            anim_dirs: Some(&["Animation/Car", "Animation/Jenkins"]),
            anim_skip: Some(&[
                "Animation/Jenkins/Velma_JenkinsPin_Captured_Montage",
                "Animation/Jenkins/Velma_JenkinsPin_Enter_Montage",
                "Animation/Jenkins/Velma_JenkinsPin_Exit_Montage",
            ]),
            montages_by_type: false,
            branch_groups: None,
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/Velma/Animation/Car/Velma_PoliceCar_AnimBP",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: Some("/Game/Panda_Main/Characters/Velma/Character_VelmaAIBotPolice"),
            pawn: Some(
                "/Game/Panda_Main/Blueprints/Characters/VelmaPrototype/Pawn_VelmaAIBotPolice.Pawn_VelmaAIBotPolice_C",
            ),
            box_dirs: None,
            companion_of: Some("velma"),
        },
    ),
    (
        "marvin",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/C018",
            anim_dirs: None,
            anim_skip: Some(&[
                "Animations/Prototype/",
                "Animations/C018_Ship_Enter_Montage",
                "Animations/C018_Ship_Reload_Montage",
                "Animations/C018_Ship_Idle_Montage",
                "Animations/C018_Ship_Fire_Montage",
                "Animations/C018_Ship_turnAround_Montage",
                "Animations/C018_Ship_Bounce_Montage",
                "Animations/C018_Ship_HR_F_Montage",
                "Animations/C018_Ship_HR_B_Montage",
                "Animations/C018_Ship_Exit_Montage",
                "Animations/C018_Ship_Destroy_Montage",
            ]),
            montages_by_type: true,
            branch_groups: Some(&[
                "/Game/Panda_Main/Characters/C018/BranchGroups/C018_Base+Combo_BranchGroup",
                "/Game/Panda_Main/Characters/C018/BranchGroups/C018_Base_BranchGroup",
                "/Game/Panda_Main/Characters/C018/BranchGroups/C018_Air_BranchGroup",
                "/Game/Panda_Main/Characters/C018/BranchGroups/C018_SpecialGround_BranchGroup",
                "/Game/Panda_Main/Characters/C018/BranchGroups/C018_SpecialAir_BranchGroup",
            ]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/C018/Animations/Base_AnimBP_C018",
            anim_node_kinds: false,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "stripe",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/C023B",
            anim_dirs: Some(&["Animation"]),
            anim_skip: None,
            montages_by_type: false,
            branch_groups: Some(&["/Game/Panda_Main/Characters/C023B/BranchGroups/BranchGroup_C023B_"]),
            anim_sets: Some(AnimSets {
                graph: "/Game/Panda_Main/Characters/C023B/Animation/C023B_AnimBP",
                sets: &[(
                    "altKB",
                    AnimSet {
                        animbp: "/Game/Panda_Main/Characters/C023B/Animation/Base_AnimBP_AltKB_C023B",
                        tag: "MovementAndHitReact",
                    },
                )],
            }),
            animbp: "/Game/Panda_Main/Characters/C023B/Animation/Base_AnimBP_C023B",
            anim_node_kinds: true,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
    (
        "gizmo",
        FighterPort {
            folder: "/Game/Panda_Main/Characters/C023A",
            anim_dirs: Some(&["Animation"]),
            anim_skip: Some(&["Animation/Car/C023A_Car_Damage_Montage"]),
            montages_by_type: true,
            branch_groups: Some(&["/Game/Panda_Main/Characters/C023A/BranchGroups/BranchGroup_C023A_"]),
            anim_sets: None,
            animbp: "/Game/Panda_Main/Characters/C023A/Animation/Base_AnimBP_C023A",
            anim_node_kinds: true,
            extra_skins: None,
            default_skin: None,
            character_data: None,
            pawn: None,
            box_dirs: None,
            companion_of: None,
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(pkg_of("/Game/A/B.B:C"), "/Game/A/B");
        assert_eq!(pkg_of("/Game/A.x/B"), "/Game/A.x/B");
        assert_eq!(asset_name("/Game/A/B.B"), "B");
        assert_eq!(asset_of("/Game/A/B"), "/Game/A/B.B");
        assert_eq!(asset_of("/Game/A/B.B"), "/Game/A/B.B");
    }

    #[test]
    fn tables() {
        let keys: Vec<&str> = CHARACTER_DATA.iter().map(|e| e.0).collect();
        assert_eq!(keys, CHARACTERS);
        let ported: Vec<&str> = FIGHTER_PORT.iter().map(|e| e.0).filter(|k| *k != "velmapolice").collect();
        assert_eq!(ported, CHARACTERS);
        assert_eq!(fighter_port("velmapolice").and_then(|p| p.companion_of), Some("velma"));
        assert_eq!(character_data("garnet"), Some("/Game/Panda_Main/Characters/Garnet/CharData/character_garnet"));
    }
}
