//! FAnimNode_LegIK: per leg, the FK chain turned toward the IK foot, the
//! two-bone solve to it, the knee twisted to the IK foot's forward axis and the foot given the IK foot's rotation.

use mvs_data::value::{Props, Value};

use super::{
    AnimNode, AnimNodeSkel, BoneXfs, CsEval, Q4, UeXf, V3, bone_of, check_alpha_defaults, clamp01, cross, f32r,
    find_between_normals, hypot, num, qmul, qrot, quat_equals, safe_normal, text, truthy, vec_equals,
};

#[derive(Clone, Debug)]
struct Leg {
    /// The IK foot bone.
    ik: usize,
    /// Foot, knee, hip (index 0 = foot, NumBones - 1 = hip).
    fk: Vec<usize>,
    /// FootBoneForwardAxis.
    axis: V3,
    /// bEnableKneeTwistCorrection.
    twist: bool,
}

/// The cvars the node reads (a.AnimNode.LegIK.Enable 1, EnableTwoBone 1 at 14506fe40: no ini sets either).
const ENABLE: i32 = 1;
const ENABLE_TWO_BONE: i32 = 1;
const IDENT: Q4 = [0.0, 0.0, 0.0, 1.0];
const TY: &str = "AnimNode_LegIK";

#[derive(Debug)]
pub struct LegIkNode {
    pub reach_precision: f64,
    /// Read by FABRIK alone: not on the two-bone path.
    pub max_iterations: f64,
    legs: Vec<Leg>,
    pub actual_alpha: f64,
    out: BoneXfs,
    /// The leg in solve (kept between solves: scratch).
    chain: Chain,
}

/// One leg's FK transforms (t, r, s per bone, foot first) and the IK foot's; the reach's links and lengths.
#[derive(Debug, Default)]
struct Chain {
    t: Vec<V3>,
    r: Vec<Q4>,
    s: Vec<V3>,
    it: V3,
    ir: Q4,
    lk: Vec<V3>,
    len: Vec<f64>,
}

impl LegIkNode {
    pub fn new(props: &Props, skel: &AnimNodeSkel<'_>) -> LegIkNode {
        check_alpha_defaults(TY, props);
        let mut legs = Vec::new();
        for d in props.get("LegsDefinition").and_then(Value::as_arr).unwrap_or(&[]) {
            let Some(d) = d.as_obj() else { continue };
            let hinge = text(d, "HingeRotationAxis").unwrap_or("EAxis::None");
            assert!(
                hinge == "EAxis::None",
                "{TY}: a hinge axis ({hinge}) is not ported (the hinge cache of SolveTwoBoneIK)"
            );
            let ik = bone_of(TY, skel, d.get("IKFootBone"));
            let foot = bone_of(TY, skel, d.get("FKFootBone"));
            let (Some(ik), Some(foot)) = (ik, foot) else { continue };
            // InitializeBoneReferences 142480c90: the foot and max(NumBonesInLimb, 1) parents; a leg of 2 bones or
            // fewer is dropped
            let want = num(d, "NumBonesInLimb").unwrap_or(2.0).max(1.0) as usize + 1;
            let mut fk = vec![foot];
            while fk.len() < want && skel.parents[*fk.last().expect("the foot")] >= 0 {
                fk.push(skel.parents[*fk.last().expect("the foot")] as usize);
            }
            if fk.len() <= 2 {
                continue;
            }
            assert!(fk.len() <= 3, "{TY}: a leg of {} bones takes FABRIK (14248d140), which is not read", fk.len());
            let axis = match text(d, "FootBoneForwardAxis").unwrap_or("EAxis::Y") {
                "EAxis::X" => [1.0, 0.0, 0.0],
                "EAxis::Y" => [0.0, 1.0, 0.0],
                "EAxis::Z" => [0.0, 0.0, 1.0],
                a => panic!("{TY}: FootBoneForwardAxis {a}"),
            };
            // MinRotationAngle and bEnableRotationLimit are read by FABRIK alone (N2): nothing on this path.
            let twist = d.get("bEnableKneeTwistCorrection").filter(|v| !v.is_null()).is_none_or(|v| truthy(Some(v)));
            legs.push(Leg { ik, fk, axis, twist });
        }
        LegIkNode {
            reach_precision: f32r(num(props, "ReachPrecision").unwrap_or(0.01)),
            max_iterations: num(props, "MaxIterations").unwrap_or(12.0),
            legs,
            actual_alpha: 0.0,
            out: BoneXfs::default(),
            chain: Chain::default(),
        }
    }

    fn valid(&self) -> bool {
        ENABLE == 1 && !self.legs.is_empty()
    }

    /// RotateLegByQuat 14248abe0: q to every FK transform about the hip, foot first. False when q is the identity.
    fn rotate_leg(c: &mut Chain, q: Q4) -> bool {
        if quat_equals(q, IDENT, 1e-4) {
            return false;
        }
        let h = *c.t.last().expect("a hip");
        for i in 0..c.t.len() {
            c.r[i] = qmul(q, c.r[i]);
            let v = qrot(q, [c.t[i][0] - h[0], c.t[i][1] - h[1], c.t[i][2] - h[2]], false);
            c.t[i] = [v[0] + h[0], v[1] + h[1], v[2] + h[2]];
        }
        true
    }

    fn solve_leg(&mut self, cs: &mut CsEval<'_>, leg: &Leg) {
        let nb = leg.fk.len();
        // 1. the IK foot and the FK chain from the pose
        let ikx = cs.get(leg.ik);
        let mut c = std::mem::take(&mut self.chain);
        c.t.clear();
        c.r.clear();
        c.s.clear();
        for &b in &leg.fk {
            let x = cs.get(b);
            c.t.push(x.t);
            c.r.push(x.q);
            c.s.push(x.s);
        }
        c.it = ikx.t;
        c.ir = ikx.q;
        let h = nb - 1;
        let it = c.it;
        let mut changed = false;
        // 2. OrientLegTowardsIK 1424869d0
        let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let a = safe_normal(sub(c.t[0], c.t[h]), 1e-8);
        let b = safe_normal(sub(it, c.t[h]), 1e-8);
        if a != [0.0; 3] && !vec_equals(a, b, 1e-4) && Self::rotate_leg(&mut c, find_between_normals(a, b)) {
            changed = true;
        }
        // 3. DoLegReachIK 14245e0a0
        if !vec_equals(c.t[0], it, self.reach_precision) {
            Self::reach(&mut c, nb);
            changed = true;
        }
        // 4. AdjustKneeTwist 1424591e0
        if leg.twist {
            let n = safe_normal(sub(it, c.t[h]), 1e-8);
            let afk = qrot(c.r[0], leg.axis, false);
            let aik = qrot(c.ir, leg.axis, false);
            let fk_dir = cross(cross(n, afk), n);
            let ik_dir = cross(cross(n, aik), n);
            // the unnormalised directions, as the exe (FindBetweenNormals gets non-unit input)
            if fk_dir != [0.0; 3]
                && !vec_equals(fk_dir, ik_dir, 1e-4)
                && Self::rotate_leg(&mut c, find_between_normals(fk_dir, ik_dir))
            {
                changed = true;
            }
        }
        // 5. the foot takes the IK foot's rotation; the whole chain goes out
        if changed || !quat_equals(c.r[0], c.ir, 1e-4) {
            c.r[0] = c.ir;
            for i in 0..nb {
                self.out.push(leg.fk[i], UeXf { t: c.t[i], q: c.r[i], s: c.s[i] });
            }
        }
        self.chain = c;
    }

    /// FIKChain init 142481be0 and ReachTarget 142487de0 toward the IK foot, then the FK rotations and translations.
    fn reach(c: &mut Chain, nb: usize) {
        let it = c.it;
        let mut lk = std::mem::take(&mut c.lk);
        let mut len = std::mem::take(&mut c.len);
        lk.clear();
        lk.extend_from_slice(&c.t);
        len.clear();
        len.extend((0..nb).map(|i| {
            if i < nb - 1 {
                let (a, b) = (c.t[i], c.t[i + 1]);
                hypot3(b[0] - a[0], b[1] - a[1], b[2] - a[2])
            } else {
                0.0
            }
        }));
        let max_reach: f64 = len.iter().sum();
        let r = nb - 1;
        let (dx, dy, dz) = (it[0] - lk[r][0], it[1] - lk[r][1], it[2] - lk[r][2]);
        if nb < 3 || max_reach * max_reach <= dx * dx + dy * dy + dz * dz {
            // OrientAllLinksToDirection 1424867e0: the straight leg toward the target
            let d = safe_normal([dx, dy, dz], 1e-8);
            for i in (0..nb - 1).rev() {
                lk[i] = [0, 1, 2].map(|k| lk[i + 1][k] + d[k] * len[i]);
            }
        } else if nb == 3 && ENABLE_TWO_BONE == 1 {
            // SolveTwoBoneIK 14248dde0: hip = link 2, knee = link 1
            lk[0] = it;
            let (a, cc, b) = (len[1], len[0], hypot3(dx, dy, dz));
            let two_ab = 2.0 * a * b;
            let cos_c = if two_ab.abs() <= 1e-8 { 0.0 } else { (a * a + b * b - cc * cc) / two_ab };
            let angle = cos_c.clamp(-1.0, 1.0).acos();
            let ib = if b <= 1e-8 { 0.0 } else { 1.0 / b };
            let dd = [dx * ib, dy * ib, dz * ib];
            let k = [lk[1][0] - lk[2][0], lk[1][1] - lk[2][1], lk[1][2] - lk[2][2]];
            let proj = k[0] * dd[0] + k[1] * dd[1] + k[2] * dd[2];
            // knee - ProjKnee
            let v = safe_normal([k[0] - dd[0] * proj, k[1] - dd[1] * proj, k[2] - dd[2] * proj], 1e-4);
            let s = angle.sin();
            lk[1] = [0, 1, 2].map(|j| (v[j] * s + dd[j] * cos_c) * a + lk[2][j]);
        } else {
            panic!("LegIK: FABRIK is not ported");
        }
        // the rotations, from the hip down, then the translations (the hip keeps its place)
        for i in (0..nb - 1).rev() {
            let a =
                safe_normal([c.t[i][0] - c.t[i + 1][0], c.t[i][1] - c.t[i + 1][1], c.t[i][2] - c.t[i + 1][2]], 1e-8);
            let b = safe_normal([lk[i][0] - lk[i + 1][0], lk[i][1] - lk[i + 1][1], lk[i][2] - lk[i + 1][2]], 1e-8);
            c.r[i + 1] = qmul(find_between_normals(a, b), c.r[i + 1]);
        }
        for i in (0..nb - 1).rev() {
            c.t[i] = lk[i];
        }
        c.lk = lk;
        c.len = len;
    }
}

fn hypot3(x: f64, y: f64, z: f64) -> f64 {
    hypot(&[x, y, z])
}

impl AnimNode for LegIkNode {
    fn ty(&self) -> &'static str {
        TY
    }

    fn update(&mut self, _dt: f64, alpha: f64) {
        self.actual_alpha = clamp01(alpha);
    }

    fn evaluate(&mut self, cs: &mut CsEval<'_>, _comp: &UeXf) {
        if !(self.actual_alpha > 1e-5) || !self.valid() {
            return;
        }
        self.out.clear();
        let legs = std::mem::take(&mut self.legs);
        for leg in &legs {
            self.solve_leg(cs, leg);
        }
        self.legs = legs;
        self.out.sort();
        cs.local_blend(&self.out, self.actual_alpha);
    }
}
