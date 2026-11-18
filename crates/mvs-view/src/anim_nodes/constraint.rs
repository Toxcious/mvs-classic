//! FAnimNode_Constraint (exe 142452870 / EvaluateSkeletalControl 142462150 /
//! SolveConstraints 1424450e0): the bone's component-space transform made from its targets' component-space
//! transforms, per setup row (Translation, Rotation, Scale, Parent; per axis), weighted, and blended into the pose by
//! the node's alpha (LocalBlendCSBoneTransforms: CsEval::local_blend).
//!
//! Made from the node's class default values (FighterData.anim_nodes, or the caller's copy of them) on a view's
//! skeleton.
//!   - A row counts when its weight is over 1e-5, its target is in the skeleton and an axis is on; the node works in
//!     component space with an identity base.
//!   - Translation: the target's translation with the axes that are off at 0; Rotation: the target's rotation through
//!     FRotator (rotator::through_rotator), an axis off clearing Roll (X), Pitch (Y) or Yaw (Z); Scale: the axes off
//!     at 0; Parent: the whole transform.
//!   - Per list, m = 1 / the weight sum when it is over 1, else 1. A Parent list over 1e-5: translation, rotation and
//!     scale are its weighted sums; else each channel on its own: a translation or rotation list over 1e-5 replaces
//!     the bone's own by its weighted sum (rotations on the running sum's side, normalised, the identity under 1e-8),
//!     a scale list by the product of m x w x s; an empty channel keeps the bone's.
//!
//! Refused where the node is made (no data walked has them): OffsetOption other than None (the reference pose's
//! offsets); a row with no axis on or a weight at or under 1e-5 (the build keeps such a row's last values, whose start
//! is not read); a bound ConstraintWeights (the caller has only the alpha).
//! The rotation is normalised before the rotator: FQuat::Rotator on the
//! component-space product, near unit length.

use mvs_data::value::{Props, Value};

use super::{
    AnimNode, AnimNodeSkel, BoneXfs, CsEval, Q4, UeXf, bone_of, check_control_alpha, clamp01, hypot, text, truthy,
};
use crate::rotator::{quat_euler, rotator_quat};

const ZERO_W: f64 = 1e-5;
const TYPES: [&str; 4] = [
    "ETransformConstraintType::Translation",
    "ETransformConstraintType::Rotation",
    "ETransformConstraintType::Scale",
    "ETransformConstraintType::Parent",
];
const T: usize = 0;
const R: usize = 1;
const S: usize = 2;
const P: usize = 3;
const TY: &str = "AnimNode_Constraint";

#[derive(Clone, Debug)]
struct Row {
    to: usize,
    ty: usize,
    x: bool,
    y: bool,
    z: bool,
    w: f64,
}

#[derive(Debug)]
pub struct ConstraintNode {
    pub bone: Option<usize>,
    rows: Vec<Row>,
    alpha: f64,
    xf: BoneXfs,
}

impl ConstraintNode {
    pub fn new(props: &Props, skel: &AnimNodeSkel<'_>) -> ConstraintNode {
        check_control_alpha(TY, props);
        let bone = bone_of(TY, skel, props.get("BoneToModify"));
        let setup = props.get("ConstraintSetup").and_then(Value::as_arr).unwrap_or(&[]);
        let weights = props.get("ConstraintWeights").and_then(Value::as_arr).unwrap_or(&[]);
        let mut rows = Vec::new();
        for (i, c) in setup.iter().enumerate() {
            let Some(c) = c.as_obj() else { continue };
            let off = text(c, "OffsetOption").unwrap_or("undefined");
            assert!(off == "EConstraintOffsetOption::None", "{TY}: OffsetOption {off} is not ported (only None)");
            let tt = text(c, "TransformType").unwrap_or("undefined");
            let ty = TYPES.iter().position(|t| *t == tt).unwrap_or_else(|| panic!("{TY}: TransformType {tt}"));
            let ax = c.get("PerAxis").and_then(Value::as_obj);
            let on = |k: &str| truthy(ax.and_then(|a| a.get(k)));
            let w = weights.get(i).and_then(Value::as_f64).unwrap_or(0.0);
            assert!(
                w > ZERO_W && (on("bX") || on("bY") || on("bZ")),
                "{TY}: a setup row of weight {w} or with no axis on is not ported"
            );
            if let Some(to) = bone_of(TY, skel, c.get("TargetBone")) {
                rows.push(Row { to, ty, x: on("bX"), y: on("bY"), z: on("bZ"), w });
            }
        }
        ConstraintNode { bone, rows, alpha: 0.0, xf: BoneXfs::default() }
    }
}

impl AnimNode for ConstraintNode {
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
        let Some(b) = self.bone else { return };
        if !(self.alpha > ZERO_W) || self.rows.is_empty() {
            return;
        }
        let bx = cs.get(b);
        let (mut ot, mut or, mut os) = (bx.t, bx.q, bx.s);
        let mut sum = [0.0f64; 4];
        for r in &self.rows {
            sum[r.ty] += r.w;
        }
        let m = sum.map(|s| if s > 1.0 { 1.0 / s } else { 1.0 });
        let parent = sum[P] > ZERO_W;
        // translation (or the Parent list's translation)
        let tl = if parent { P } else { T };
        if sum[tl] > ZERO_W {
            ot = [0.0; 3];
            for r in self.rows.iter().filter(|r| r.ty == tl) {
                let t = cs.get(r.to).t;
                let k = m[tl] * r.w;
                let all = r.ty == P;
                ot[0] += k * if all || r.x { t[0] } else { 0.0 };
                ot[1] += k * if all || r.y { t[1] } else { 0.0 };
                ot[2] += k * if all || r.z { t[2] } else { 0.0 };
            }
        }
        let rl = if parent { P } else { R };
        if sum[rl] > ZERO_W {
            let mut first = true;
            or = [0.0; 4];
            for r in self.rows.iter().filter(|r| r.ty == rl) {
                let tq = cs.get(r.to).q;
                let q: Q4 = if r.ty == R {
                    // FQuat -> FRotator, the axes off cleared, -> FQuat (the round trip with every axis on too)
                    let l = hypot(&tq);
                    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
                    let e = quat_euler(tq.map(|c| c / l));
                    rotator_quat(
                        if r.y { e[1] } else { 0.0 },
                        if r.z { e[2] } else { 0.0 },
                        if r.x { e[0] } else { 0.0 },
                    )
                } else {
                    tq // Parent: the whole transform
                };
                let k = m[rl] * r.w;
                let dot = or[0] * q[0] + or[1] * q[1] + or[2] * q[2] + or[3] * q[3];
                let sg = if first || dot >= 0.0 { k } else { -k };
                first = false;
                for j in 0..4 {
                    or[j] += q[j] * sg;
                }
            }
            let l2 = or[0] * or[0] + or[1] * or[1] + or[2] * or[2] + or[3] * or[3];
            if l2 < 1e-8 {
                or = [0.0, 0.0, 0.0, 1.0];
            } else {
                let l = l2.sqrt();
                or = or.map(|c| c / l);
            }
        }
        if parent {
            os = [0.0; 3];
            for r in self.rows.iter().filter(|r| r.ty == P) {
                let s = cs.get(r.to).s;
                let k = m[P] * r.w;
                for j in 0..3 {
                    os[j] += k * s[j];
                }
            }
        } else if sum[S] > ZERO_W {
            let mut first = true;
            for r in self.rows.iter().filter(|r| r.ty == S) {
                let s = cs.get(r.to).s;
                let k = m[S] * r.w;
                let c = [
                    if r.x { k * s[0] } else { 0.0 },
                    if r.y { k * s[1] } else { 0.0 },
                    if r.z { k * s[2] } else { 0.0 },
                ];
                for j in 0..3 {
                    os[j] = if first { c[j] } else { os[j] * c[j] };
                }
                first = false;
            }
        }
        self.xf.clear();
        self.xf.push(b, UeXf { t: ot, q: or, s: os });
        cs.local_blend(&self.xf, self.alpha);
    }
}
