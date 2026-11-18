//! The float curves an AnimBP reads by name (GetCurveValue) from the sequences it plays, for a pose hook that indexes
//! them: `names` in the hook's order, read from a character's clips. A clip the set does not hold, or a curve the clip
//! lacks, reads 0 (an element of FBlendedCurve no player wrote).
//! Besides: [`rich_curve`] over [`CurveKey`]s, an FRichCurve a class holds in its defaults (a timeline's or a
//! component's curve).

use crate::anim::Clips;

/// A read is at time + CURVE_EPS: the double clock reaches a frame boundary k/60 just under it where the build's
/// float32 delta (0.0166668) is past it, so a key on the boundary (C018's show_mars_flag 1 -> 0 at 3.9 s) takes
/// effect on frame k.
// Unverified: the build's delta is native (not read).
const CURVE_EPS: f64 = 1e-6;

/// The curves a hook reads, in its order (`CurveTable`).
#[derive(Clone, Copy, Debug)]
pub struct CurveTable {
    pub names: &'static [&'static str],
}

/// `RenderClip.curve`: curve `name` of clip `c` at `time`, None when the sequence has none. The name compares
/// without case, as an FName (C023B_AnimBP's show_Sawblade is the sequences' show_sawblade): an exact name first,
/// else the last of the clip's names that folds to it.
pub fn clip_curve(clips: &Clips, c: usize, name: &str, time: f64) -> Option<f64> {
    let anim = &clips.clips[c].anim;
    if let Some(v) = anim.curve(name, time) {
        return Some(v);
    }
    let real = anim.curve_names().iter().rev().find(|n| n.eq_ignore_ascii_case(name))?;
    anim.curve(real, time)
}

impl CurveTable {
    pub const fn new(names: &'static [&'static str]) -> CurveTable {
        CurveTable { names }
    }

    /// The index of curve `name` in the table (a hook's own constant missing from it stops here).
    pub fn index(&self, name: &str) -> usize {
        self.names.iter().position(|n| *n == name).unwrap_or_else(|| panic!("curve table has no curve {name}"))
    }

    /// `curves.indexOf(name)` where a missing name is not an error: None (the caller reads such a curve as 0, as
    /// Rick's show_curled_whip, on no sequence).
    pub fn find(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| *n == name)
    }

    /// `value`: curve `k` of the clip named `anim` (the clips' key) at `time`.
    pub fn value(&self, clips: &Clips, anim: &str, k: usize, time: f64) -> f64 {
        self.value_of(clips, clips.get(anim), k, time)
    }

    /// `value` of clip index `clip` (None: not loaded).
    pub fn value_of(&self, clips: &Clips, clip: Option<usize>, k: usize, time: f64) -> f64 {
        clip.and_then(|c| clip_curve(clips, c, self.names[k], time + CURVE_EPS)).unwrap_or(0.0)
    }

    /// `fill`: every curve of the clip named `anim` at `time` into `out`, in the table's order.
    pub fn fill(&self, clips: &Clips, anim: &str, time: f64, out: &mut [f64]) {
        self.fill_of(clips, clips.get(anim), time, out);
    }

    /// `fill` of clip index `clip` (None: not loaded, every curve 0).
    pub fn fill_of(&self, clips: &Clips, clip: Option<usize>, time: f64, out: &mut [f64]) {
        for (k, o) in out.iter_mut().enumerate().take(self.names.len()) {
            *o = self.value_of(clips, clip, k, time);
        }
    }
}

/// A rich curve's key (a stage block's, a timeline's): time, value, arrive tangent, leave tangent, mode (0 linear,
/// 1 cubic, 2 constant).
pub type CurveKey = [f64; 5];

/// FRichCurve::Eval for keys without tangent weights and constant extrapolation: the first
/// key's mode decides a segment; a cubic one is the engine's Bezier form (control points a third of the key interval
/// along the leave and arrive tangents). A curve without keys is its default value 0.
pub fn rich_curve(keys: &[CurveKey], t: f64) -> f64 {
    let n = keys.len();
    if n == 0 {
        return 0.0;
    }
    if t <= keys[0][0] {
        return keys[0][1];
    }
    if t >= keys[n - 1][0] {
        return keys[n - 1][1];
    }
    let mut i = 0;
    while keys[i + 1][0] <= t {
        i += 1;
    }
    let (a, b) = (&keys[i], &keys[i + 1]);
    if a[4] == 2.0 {
        return a[1];
    }
    let diff = b[0] - a[0];
    let x = (t - a[0]) / diff;
    let (p0, p3) = (a[1], b[1]);
    if a[4] == 0.0 {
        return p0 + (p3 - p0) * x;
    }
    let p1 = p0 + (a[3] * diff) / 3.0;
    let p2 = p3 - (b[2] * diff) / 3.0;
    let (p01, p12, p23) = (p0 + (p1 - p0) * x, p1 + (p2 - p1) * x, p2 + (p3 - p2) * x);
    let (p012, p123) = (p01 + (p12 - p01) * x, p12 + (p23 - p12) * x);
    p012 + (p123 - p012) * x
}
