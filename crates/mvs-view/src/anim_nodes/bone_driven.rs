//! FAnimNode_BoneDrivenController (vtable 143fd4a58, ctor 1424526b0,
//! ExtractSourceValue 1424737e0, EvaluateSkeletalControl 142460ee0): a channel of the source bone's local transform,
//! measured from the reference pose, drives channels of the target bone's local transform.
//!
//! The result goes to component space and is blended into the pose by the alpha (LocalBlendCSBoneTransforms).
//!   value: SourceComponent 1-3 translation X-Z (current local - reference local), 4-6 rotation X-Z (FQuat::Euler of
//!     current x reference^-1, (Roll, Pitch, Yaw) as X, Y, Z), 7 Scale (the largest axis of the current scale minus
//!     the reference's), 8-10 ScaleX-Z. bUseRange: pct = (v - RangeMin) / (RangeMax - RangeMin) (when |range| <= 1e-8:
//!     0 under RangeMax, else 1), clamped 0..1, v = lerp(RemappedMin, RemappedMax, pct). Then v x Multiplier.
//!   target: its local transform in the input pose (AddToRefPose: its reference pose's) with each affected channel
//!     changed: AddToInput / AddToRefPose add v (a rotation: current x FQuat::MakeFromEuler(the delta), on the bone's
//!     own axes); ReplaceComponent sets it to v. Composed with the parent's component-space transform (the root's as
//!     it is) into the target's component-space transform.
//! The reference pose is the view's (AnimNodeSkel::rest: CharView::rest, the skeleton's bind transforms in the model
//! file). Refused where the node is made: a DrivingCurve (the curve's value, which the view does not sample), a
//! DestinationMode other than Bone (the morph target and material parameter modes), SourceComponent None.
//! C023B_AnimBP's node: fml_un_COM TranslationY x -2 -> fml_un_C_pelvis_att TranslationY, AddToInput, alpha bound.
// Unverified (stock 4.26, not in the native read): ReplaceComponent's rotation keeps the other Euler axes of the
// current rotation; a rotation delta that is nearly zero (|each| <= 1e-4) changes nothing; Scale's largest axis is
// the absolute largest (GetMaximumAxisScale).

use mvs_data::value::Props;

use super::{
    AnimNode, AnimNodeSkel, BoneXfs, CsEval, UeXf, bone_of, check_control_alpha, clamp01, local_ue, num, qmul, text,
    truthy,
};
use crate::rotator::{euler_quat, quat_euler};

const ZERO_W: f64 = 1e-5;
const COMPONENTS: [&str; 11] = [
    "None",
    "TranslationX",
    "TranslationY",
    "TranslationZ",
    "RotationX",
    "RotationY",
    "RotationZ",
    "Scale",
    "ScaleX",
    "ScaleY",
    "ScaleZ",
];
const MODES: [&str; 3] = ["AddToInput", "ReplaceComponent", "AddToRefPose"];
const TY: &str = "AnimNode_BoneDrivenController";

#[derive(Debug)]
pub struct BoneDrivenControllerNode {
    pub source: Option<usize>,
    pub target: Option<usize>,
    comp: usize,
    mode: usize,
    mult: f64,
    range: Option<[f64; 4]>,
    /// Translation X-Z, rotation X-Z, scale X-Z.
    aff: [bool; 9],
    alpha: f64,
    src_ref: UeXf,
    tgt_ref: UeXf,
    xf: BoneXfs,
}

impl BoneDrivenControllerNode {
    pub fn new(props: &Props, skel: &AnimNodeSkel<'_>) -> BoneDrivenControllerNode {
        check_control_alpha(TY, props);
        assert!(!truthy(props.get("DrivingCurve")), "{TY}: a DrivingCurve is not ported");
        let dm = text(props, "DestinationMode").unwrap_or("undefined");
        assert!(dm == "EDrivenDestinationMode::Bone", "{TY}: DestinationMode {dm} is not ported (Bone only)");
        let sc = text(props, "SourceComponent").unwrap_or("undefined");
        let comp = COMPONENTS.iter().position(|c| *c == sc.replace("EComponentType::", "")).unwrap_or(0);
        assert!(comp > 0, "{TY}: SourceComponent {sc} is not ported");
        let mm = text(props, "ModificationMode").unwrap_or("undefined");
        let mode = MODES
            .iter()
            .position(|m| *m == mm.replace("EDrivenBoneModificationMode::", ""))
            .unwrap_or_else(|| panic!("{TY}: ModificationMode {mm}"));
        let rest =
            skel.rest.unwrap_or_else(|| panic!("{TY}: made without the view's reference pose (AnimNodeSkel.rest)"));
        let source = bone_of(TY, skel, props.get("SourceBone"));
        let target = bone_of(TY, skel, props.get("TargetBone"));
        let n = |k: &str| num(props, k).unwrap_or(f64::NAN);
        let keys = [
            "TranslationX",
            "TranslationY",
            "TranslationZ",
            "RotationX",
            "RotationY",
            "RotationZ",
            "ScaleX",
            "ScaleY",
            "ScaleZ",
        ];
        BoneDrivenControllerNode {
            source,
            target,
            comp,
            mode,
            mult: num(props, "Multiplier").unwrap_or(1.0),
            range: truthy(props.get("bUseRange"))
                .then(|| [n("RangeMin"), n("RangeMax"), n("RemappedMin"), n("RemappedMax")]),
            aff: keys.map(|k| truthy(props.get(&format!("bAffectTarget{k}")))),
            alpha: 0.0,
            src_ref: source.map_or_else(UeXf::default, |b| local_ue(rest, b)),
            tgt_ref: target.map_or_else(UeXf::default, |b| local_ue(rest, b)),
            xf: BoneXfs::default(),
        }
    }

    /// ExtractSourceValue on the source's local transform, with the range and the multiplier.
    fn value(&self, l: &UeXf) -> f64 {
        let c = self.comp;
        let r = &self.src_ref;
        let mut v = if c <= 3 {
            l.t[c - 1] - r.t[c - 1]
        } else if c <= 6 {
            quat_euler(qmul(l.q, [-r.q[0], -r.q[1], -r.q[2], r.q[3]]))[c - 4]
        } else if c == 7 {
            let m = |s: &[f64; 3]| s[0].abs().max(s[1].abs()).max(s[2].abs());
            m(&l.s) - m(&r.s)
        } else {
            l.s[c - 8] - r.s[c - 8]
        };
        if let Some([lo, hi, a, b]) = self.range {
            let span = hi - lo;
            let pct = if span.abs() <= 1e-8 { if v < hi { 0.0 } else { 1.0 } } else { (v - lo) / span };
            v = a + (b - a) * clamp01(pct);
        }
        v * self.mult
    }
}

impl AnimNode for BoneDrivenControllerNode {
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
        if !(self.alpha > ZERO_W) {
            return;
        }
        let v = self.value(&cs.local_of(a));
        // the target's local transform (AddToRefPose: its reference pose's)
        let mut l = if self.mode == 2 { self.tgt_ref } else { cs.local_of(b) };
        let (aff, replace) = (self.aff, self.mode == 1);
        for k in 0..3 {
            if aff[k] {
                l.t[k] = if replace { v } else { l.t[k] + v };
            }
            if aff[6 + k] {
                l.s[k] = if replace { v } else { l.s[k] + v };
            }
        }
        if (aff[3] || aff[4] || aff[5]) && v.abs() > 1e-4 {
            if replace {
                let mut keep = quat_euler(l.q);
                for k in 0..3 {
                    if aff[3 + k] {
                        keep[k] = v;
                    }
                }
                l.q = euler_quat(keep);
            } else {
                let e = [0, 1, 2].map(|k| if aff[3 + k] { v } else { 0.0 });
                l.q = qmul(l.q, euler_quat(e));
            }
        }
        let x = cs.to_component(b, &l);
        self.xf.clear();
        self.xf.push(b, x);
        cs.local_blend(&self.xf, self.alpha);
    }
}
