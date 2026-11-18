//! The boxes bind's hand-kept tables of the frame half: the AnimBP node choices that are graph wiring, not data, with
//! their sources.

/// Base_AnimBP AnimGraphNode_ModifyBone_5.BoneToModify (every character's AnimBP derives from it): the bone the hit
/// rotation turns, and the one COM Replace sets (BoxBindInput.com_replaced).
pub const COM_BONE: &str = "fml_un_COM";

/// A change of a skel track's value under this (glTF metres, a quaternion component, a scale) is no change (the flags
/// of SkelIndex.clips[].v).
pub const SKEL_EPS: f64 = 1e-6;

/// The characters whose AnimBP layers poses under collision shapes (the sim's skel pose reads the block):
/// masks: the bone branches of the layered nodes (BranchFilters, BlendDepth 0: the bone and everything under it);
/// keep: bones the sim's graph reads beside the shapes' own;
/// blend: the BlendSpace assets its additive nodes play, by the sim's name;
/// nav: only the clips the anim graph plays outside montages are kept (the fighter data's animSet strings,
///   animAssets / fallSpace / wallSpaces samples, the blend spaces' samples) and these; `None`: every clip.
#[derive(Debug)]
pub struct SkelSpec {
    pub masks: &'static [(&'static str, &'static [&'static str])],
    pub keep: &'static [&'static str],
    pub blend: &'static [(&'static str, &'static str)],
    pub nav: Option<&'static [&'static str]>,
}

/// The SKEL table, by key.
pub const SKEL: &[(&str, SkelSpec)] = &[
    // Iron Giant, C017_AnimBP (the nodes' CDO values):
    // tentacle = LayeredBoneBlend_1 / _5 / _6, arm = LayeredBoneBlend_10, neck = LayeredBoneBlend_2, spine =
    // LayeredBoneBlend_8 and ModifyBone_5's bone. LayeredBoneBlend_9's two bones (fml_un_wp_?_clav_plate_offset,
    // BlendDepth 1) carry no shape and no bone that does. fml_un_C_head: the bone BlueprintUpdateAnimation reads for
    // the tentacle aim.
    (
        "irongiant",
        SkelSpec {
            masks: &[
                ("tentacle", &["fml_un_tentacle_att"]),
                (
                    "arm",
                    &[
                        "fml_un_L_uparm_att",
                        "fml_un_R_uparm_att",
                        "fml_un_C_neck_a",
                        "fml_un_wp_neck1",
                        "fml_un_wp_head",
                        "fml_un_wp_neck2",
                        "fml_un_wp_neck3",
                    ],
                ),
                ("neck", &["fml_un_C_neck_a", "fml_un_wp_neck_att"]),
                ("spine", &["fml_un_C_spine_a"]),
            ],
            keep: &["fml_un_C_head", "fml_un_C_spine_a", "fml_un_tentacle_att"],
            blend: &[(
                "tenticleAim",
                "/Game/Panda_Main/Characters/C017/Animations/Specials/WeaponAimTenticle/C017_TenticleAim_BlendSpace.C017_TenticleAim_BlendSpace",
            )],
            nav: None,
        },
    ),
    // Batman, Batman_AnimBP's glide stack: the BlendListByBool on IsGliding over BlendSpacePlayer_1 (GlideLoop, full
    // poses), the GlideAdditive machine's two additive sequences and BlendSpacePlayer (GlideAdd, additive). No layered
    // node. The sim poses from the skel only the source under the Slot nodes, so only the clips the anim graph plays
    // outside montages are kept, with the GlideAdditive machine's two sequences.
    (
        "batman",
        SkelSpec {
            masks: &[],
            keep: &[],
            blend: &[
                (
                    "glideLoop",
                    "/Game/Panda_Main/Characters/BatmanV2/Animations/Batman_Nav_GlideLoop_BlendSpace.Batman_Nav_GlideLoop_BlendSpace",
                ),
                (
                    "glideAdd",
                    "/Game/Panda_Main/Characters/BatmanV2/Animations/Batman_Nav_GlideAdd_BlendSpace.Batman_Nav_GlideAdd_BlendSpace",
                ),
            ],
            nav: Some(&["Batman_Nav_GlideEnterAdd", "Batman_Nav_GlideLoopAdd"]),
        },
    ),
    // Tom and Jerry, Tom_and_Jerry_AnimBP's AnimGraphNode_LayeredBoneBlend (LayerSetup[0].BranchFilters, BlendDepth
    // 0): the rocket arms over the aim tree. Only Tom_and_Jerry_Rocket_Ground_Montage has tracks in its layer's slots
    // (ControlL / ControlR): its clips are kept beside the anim graph's.
    (
        "tomandjerry",
        SkelSpec {
            masks: &[("arms", &["tom_un_L_clav", "tom_un_R_clav"])],
            keep: &[],
            blend: &[],
            nav: Some(&[
                "Tom_and_Jerry_Rocket_Ground",
                "Tom_and_Jerry_Rocket_Ground_Control",
                "Tom_and_Jerry_Rocket_Ground_Control_Up",
                "Tom_and_Jerry_Rocket_Ground_Control_Down",
                "Tom_and_Jerry_Rocket_Ground_Control_02",
            ]),
        },
    ),
];

/// The SKEL entry of `key`.
pub fn skel_spec(key: &str) -> Option<&'static SkelSpec> {
    SKEL.iter().find(|(k, _)| *k == key).map(|(_, s)| s)
}

/// A character whose mesh AnimBP lays an additive sequence over every pose: the AnimBP class, its ApplyAdditive node
/// and the SequencePlayer node feeding the node's Additive pin (graph wiring, hand-decoded from the links:
/// Creature_AnimBP LinkID 11).
#[derive(Debug)]
pub struct PoseAdditive {
    pub anim_bp: &'static str,
    pub node: &'static str,
    pub player: &'static str,
}

pub const POSE_ADDITIVE: &[(&str, PoseAdditive)] = &[(
    "creature",
    PoseAdditive {
        anim_bp: "/Game/Panda_Main/Characters/Creature/Animation/Creature_AnimBP.Creature_AnimBP_C",
        node: "AnimGraphNode_ApplyAdditive",
        player: "AnimGraphNode_SequencePlayer_1",
    },
)];

/// The POSE_ADDITIVE entry of `key`.
pub fn pose_additive(key: &str) -> Option<&'static PoseAdditive> {
    POSE_ADDITIVE.iter().find(|(k, _)| *k == key).map(|(_, s)| s)
}
