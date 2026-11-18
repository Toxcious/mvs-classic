//! A pose in the build's bone-local space by skeleton bone index: what the clip
//! sampler writes and its start pose comes from.

#[derive(Debug, Clone, PartialEq)]
pub struct BonePose {
    pub n: usize,
    /// Translation, 3 a bone (cm).
    pub t: Vec<f32>,
    /// Rotation, 4 a bone (x y z w).
    pub r: Vec<f32>,
    /// Scale, 3 a bone.
    pub s: Vec<f32>,
}

impl BonePose {
    pub fn new(n: usize) -> BonePose {
        BonePose { n, t: vec![0.0; n * 3], r: vec![0.0; n * 4], s: vec![0.0; n * 3] }
    }

    /// `copy`: `src`'s values (the same bone count).
    pub fn copy(&mut self, src: &BonePose) {
        assert_eq!(src.n, self.n, "pose of {} bones copied into one of {}", src.n, self.n);
        self.t.copy_from_slice(&src.t);
        self.r.copy_from_slice(&src.r);
        self.s.copy_from_slice(&src.s);
    }

    /// `additiveIdentity` (FUN_1404d63e0): identity rotation, translation 0, scale 0.
    pub fn additive_identity(&mut self) {
        self.t.fill(0.0);
        self.s.fill(0.0);
        for q in self.r.as_chunks_mut::<4>().0 {
            q.copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
        }
    }
}

/// `poseFrom10`: a pose from 10 numbers a bone (translation xyz, rotation xyzw, scale xyz).
pub fn pose_from10(pose: &[f64]) -> BonePose {
    assert!(pose.len().is_multiple_of(10), "a pose of {} numbers (10 a bone)", pose.len());
    let mut p = BonePose::new(pose.len() / 10);
    for i in 0..p.n {
        let o = i * 10;
        for k in 0..3 {
            p.t[i * 3 + k] = pose[o + k] as f32;
            p.s[i * 3 + k] = pose[o + 7 + k] as f32;
        }
        for k in 0..4 {
            p.r[i * 4 + k] = pose[o + 3 + k] as f32;
        }
    }
    p
}
