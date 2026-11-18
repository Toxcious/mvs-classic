//! FAnimNode_CopyBone (vtable 143fd4e60, ctor 1424528e0, EvaluateSkeletalControl
//! 1424623c0, IsValid 142484640): the flagged channels (bCopyTranslation / bCopyRotation / bCopyScale) of the source
//! bone's transform set on the target bone, in the node's ControlSpace.
//!
//! Blended into the pose by the alpha (LocalBlendCSBoneTransforms: the target's children keep their local transforms
//! and follow it). Nothing without a flag on; valid when the target is in the skeleton, and the source too when it
//! differs. ControlSpace: ComponentSpace, or WorldSpace, which goes through the component's world transform both ways
//! and is the component-space copy for translation and rotation; for scale too while the component's scale is uniform
//! (a fighter's mesh). BoneSpace and ParentBoneSpace are refused where the node is made (no node of the data has them).
//! C023A_AnimBP's six nodes: translation only (CopyBone_3: translation and rotation), WorldSpace except CopyBone_5,
//! alpha 1.
// Unverified: the world-space scale copy is not read.

use mvs_data::value::Props;

use super::{AnimNode, AnimNodeSkel, BoneXfs, CsEval, UeXf, bone_of, check_control_alpha, clamp01, text, truthy};

const ZERO_W: f64 = 1e-5;
const TY: &str = "AnimNode_CopyBone";

#[derive(Debug)]
pub struct CopyBoneNode {
    pub source: Option<usize>,
    pub target: Option<usize>,
    tr: bool,
    rot: bool,
    sc: bool,
    alpha: f64,
    xf: BoneXfs,
}

impl CopyBoneNode {
    pub fn new(props: &Props, skel: &AnimNodeSkel<'_>) -> CopyBoneNode {
        check_control_alpha(TY, props);
        let space = text(props, "ControlSpace").unwrap_or("BCS_ComponentSpace");
        assert!(
            space == "BCS_WorldSpace" || space == "BCS_ComponentSpace",
            "{TY}: ControlSpace {space} is not ported (component or world space only)"
        );
        CopyBoneNode {
            source: bone_of(TY, skel, props.get("SourceBone")),
            target: bone_of(TY, skel, props.get("TargetBone")),
            tr: truthy(props.get("bCopyTranslation")),
            rot: truthy(props.get("bCopyRotation")),
            sc: truthy(props.get("bCopyScale")),
            alpha: 0.0,
            xf: BoneXfs::default(),
        }
    }
}

impl AnimNode for CopyBoneNode {
    fn ty(&self) -> &'static str {
        TY
    }

    fn initialize(&mut self) {
        self.alpha = 0.0;
    }

    fn update(&mut self, _dt: f64, alpha: f64) {
        self.alpha = clamp01(alpha);
    }

    fn evaluate(&mut self, cs: &mut CsEval<'_>, _comp: &UeXf) {
        let (Some(a), Some(b)) = (self.source, self.target) else { return };
        if !(self.alpha > ZERO_W) || !(self.tr || self.rot || self.sc) {
            return;
        }
        let ax = cs.get(a);
        let bx = cs.get(b);
        let x = UeXf {
            t: if self.tr { ax.t } else { bx.t },
            q: if self.rot { ax.q } else { bx.q },
            s: if self.sc { ax.s } else { bx.s },
        };
        self.xf.clear();
        self.xf.push(b, x);
        cs.local_blend(&self.xf, self.alpha);
    }
}
