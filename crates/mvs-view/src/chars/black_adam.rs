//! Black Adam's render module: the entry's head scale nodes. The AnimBP's
//! other nodes are the shared path's (the hover stack and AimGrab blends from the sim's slot mix, the mirror as a
//! reflection of the right-facing pose, the virtual bones); the belt Trail and AnimDynamics move no drawn vertex.

use super::CharRender;
use crate::actors::MeshRule;
use crate::anim::HeadScaleNode;

pub(crate) const RENDER: CharRender = CharRender { head_scale: &HEAD, ..CharRender::GENERIC };

/// The classes' own mesh drawing, by class name: none.
pub(crate) const RULES: &[(&str, MeshRule)] = &[];

/// C021_AnimBP's ModifyBone_4, ModifyBone_1 and ModifyBone (scale Additive, bone space, from HeadScale; Pawn_C021
/// @610); badm_s2_un_C_Face_att is the S02 skin's face.
const HEAD: [HeadScaleNode; 3] = [
    head("ModifyBone_4", "fml_un_C_head"),
    head("ModifyBone_1", "fml_un_C_Face_att"),
    head("ModifyBone", "badm_s2_un_C_Face_att"),
];

const fn head(node: &'static str, bone: &'static str) -> HeadScaleNode {
    HeadScaleNode { node, bone, factor: 1.0, gated: false, moves: [0.0; 3], scale: true }
}
