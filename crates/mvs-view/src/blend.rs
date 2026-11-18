//! The blend spaces an AnimBP plays through its BlendSpacePlayer nodes and FAnimNode_BlendSpacePlayer over them as
//! the exe runs it.
//!
//! The players come bound from the fighter kind's entry (`blendSpaces`: the importer's bind of
//! the blend-space data, by `<AnimBP class>:<node>`); a character's table names the nodes its view runs.
//!
//! The player:
//!   select   the input filter's pass-through (the last output at dt <= 1e-4), the clamp and the grid position, the
//!            grid elements, the rows by sample index in append order, the merge by animation, the sort, the 1e-5 cut
//!            and the normalise;
//!   advance  the weighted length and the leader's time advance;
//!   pose / additive_pose  each row's clip at clamp01(t) x its length, blended in the cache's order by
//!            BlendPosesTogether's kernels; an additive sample's delta is its clip's own cooked tracks (the build's
//!            delta on its RefPoseSeq, so the data's `ref_pose` is not read).
//! Deviation: the sort is the exe's for 8 rows or fewer (a selection sort); past 8 rows the exe takes an introsort
//! whose order among equal weights is not reproduced (warned once). The exe's float32 steps are not kept beyond the
//! rows' float32 weights. Not here: the notifies of the highest weighted sample, root motion extraction, curves and
//! attributes.

use indexmap::IndexMap;

pub use mvs_anim::blend_space::{BlendAxis, BlendSample, BlendSpaceData, BlendSpacePlayerData};

use crate::anim::{Clips, Pose, Rig, add_weighted_pose, normalize_rotations, sample_clip, weight_pose};
use crate::{Error, Result};

/// The port's own: the BlendSpacePlayer nodes a character's view runs, by the port's name for each (`hoverIn` ->
/// `AnimGraphNode_BlendSpacePlayer`); the importer's `fighter_port::BLEND_SPACES` names their AnimBP.
#[derive(Debug)]
pub struct BlendSpaceTable {
    pub nodes: &'static [(&'static str, &'static str)],
}

/// A table's nodes bound, by the port's names.
#[derive(Clone, Debug, Default)]
pub struct BlendSpaces(IndexMap<String, BlendSpacePlayerData>);

impl BlendSpaces {
    /// A bound player by the port's name; a name the table did not bind is an error (no default).
    pub fn player(&self, id: &str) -> &BlendSpacePlayerData {
        self.0
            .get(id)
            .unwrap_or_else(|| panic!("blend space player {id}: not bound (its character's BlendSpaceTable lacks it)"))
    }

    /// The clips the players take (each sample's, by name and object path), in the table's order.
    pub fn clips(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.values().flat_map(|d| d.space.samples.iter().map(|s| (s.clip.as_str(), s.path.as_str())))
    }

    /// Binds `table` from the fighter kind's entry of `key` (its `blendSpaces`, the importer's bind by
    /// `<AnimBP class>:<node>`).
    pub fn bind(store: &mvs_store::Store, key: &str, table: &BlendSpaceTable) -> Result<BlendSpaces> {
        BlendSpaces::read(&*store.kind("fighter")?, key, table)
    }

    /// `bind` from the fighter kind's pack.
    pub fn read(pack: &mvs_store::Pack, key: &str, table: &BlendSpaceTable) -> Result<BlendSpaces> {
        #[derive(serde::Deserialize)]
        struct Entry {
            #[serde(rename = "blendSpaces", default)]
            blend_spaces: IndexMap<String, BlendSpacePlayerData>,
        }
        let tail = format!("/{key}.{key}").to_lowercase();
        let Some(path) = pack.paths().find(|p| p.to_lowercase().ends_with(&tail)).map(str::to_owned) else {
            return Err(format_err(&format!("fighter {key}"), "no entry in the fighter kind"));
        };
        let e: Entry = serde_json::from_slice(&pack.bytes(&path)?).map_err(|e| format_err(&path, e.to_string()))?;
        let mut out = IndexMap::new();
        for &(id, node) in table.nodes {
            let tail = format!(":{node}");
            let mut found = e.blend_spaces.iter().filter(|(k, _)| k.ends_with(&tail));
            let (Some((_, d)), None) = (found.next(), found.next()) else {
                return Err(format_err(&path, format!("blendSpaces: not one player of node {node}")));
            };
            out.insert(id.to_owned(), d.clone());
        }
        Ok(BlendSpaces(out))
    }
}

fn format_err(what: &str, why: impl Into<String>) -> Error {
    Error::Format { what: what.to_owned(), why: why.into() }
}

// ---- the player -------------------------------------------------------------------------------------------------

/// ZERO_ANIMWEIGHT_THRESH.
const MIN_W: f64 = 1e-5;
/// The input filter's test.
const MIN_DT: f64 = 1e-4;

// (written out: a NaN stays a NaN)
#[allow(clippy::manual_clamp)]
fn clamp01(x: f64) -> f64 {
    if x < 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

fn f32r(x: f64) -> f32 {
    x as f32
}

/// `BlendSpacePlayer`: one node's state (the normalised time and the cache's rows) over a character's clips.
#[derive(Debug)]
pub struct BlendSpacePlayer {
    pub data: BlendSpacePlayerData,
    /// The normalised time (InternalTimeAccumulator).
    pub t: f64,
    /// The cache: `count` rows of a sample index, a weight (TotalWeight), a rate (SamplePlayRate).
    pub count: usize,
    pub row_sample: Vec<i32>,
    pub row_w: Vec<f32>,
    pub row_rate: Vec<f32>,
    /// Each sample's clip (None: not loaded; warned, and `ok` is false: the caller does not use the player).
    pub clips: Vec<Option<usize>>,
    pub ok: bool,
    /// The additive samples whose clip holds one key a track: their delta poses, made once (cache_deltas).
    deltas: Option<Vec<Option<usize>>>,
    delta_poses: Vec<Pose>,
    last_x: f64,
    last_y: f64,
}

impl BlendSpacePlayer {
    pub fn new(data: &BlendSpacePlayerData, clips: &Clips) -> BlendSpacePlayer {
        let s = &data.space;
        let rows = if s.dims == 1 { 6 } else { 12 }; // two or four grid elements of three slots
        let ids: Vec<Option<usize>> = s.samples.iter().map(|x| clips.get(&x.clip)).collect();
        let mut ok = true;
        for (x, c) in s.samples.iter().zip(&ids) {
            match c {
                Some(c) if clips.clips[*c].additive == s.additive => {}
                Some(_) => {
                    ok = false;
                    let not = if s.additive { " is not additive" } else { " is additive" };
                    eprintln!("warning: blend space {}: the clip {}{not}", s.name, x.clip);
                }
                None => {
                    ok = false;
                    eprintln!("warning: blend space {}: no such clip: {}", s.name, x.clip);
                }
            }
        }
        let mut p = BlendSpacePlayer {
            data: data.clone(),
            t: 0.0,
            count: 0,
            row_sample: vec![0; rows],
            row_w: vec![0.0; rows],
            row_rate: vec![0.0; rows],
            clips: ids,
            ok,
            deltas: None,
            delta_poses: Vec::new(),
            last_x: 0.0,
            last_y: 0.0,
        };
        p.init();
        p
    }

    /// The node's Initialize, or its state entered at no weight.
    pub fn init(&mut self) {
        self.t = clamp01(self.data.start_position);
        self.count = 0;
        self.last_x = 0.0;
        self.last_y = 0.0;
    }

    pub fn uses(&self, clip: usize) -> bool {
        self.clips.contains(&Some(clip))
    }

    fn element(&mut self, row: i64, w: f64) {
        let g = &self.data.space.grid;
        if row < 0 || row as usize * 6 >= g.len() {
            return; // an element out of range: no sample, weight 0
        }
        let row = row as usize;
        for k in 0..3 {
            let si = g[row * 6 + k] as i32;
            if si < 0 {
                continue;
            }
            let mut r = 0;
            while r < self.count && self.row_sample[r] != si {
                r += 1;
            }
            if r == self.count {
                self.row_sample[r] = si;
                self.row_w[r] = 0.0;
                self.row_rate[r] = f32r(self.data.space.samples[si as usize].rate);
                self.count += 1;
            }
            self.row_w[r] = f32r(f64::from(self.row_w[r]) + w * g[row * 6 + 3 + k]);
        }
    }

    /// The cache for the inputs x, y of this update. False: no sample (the cache is empty).
    pub fn select(&mut self, x: f64, y: f64, dt: f64) -> bool {
        if dt > MIN_DT {
            self.last_x = x;
            self.last_y = y;
        }
        self.count = 0;
        let s = &self.data.space;
        let a0 = s.axes[0].clone();
        let a1 = s.axes.get(1).cloned();
        let dims = s.dims;
        let px = (self.last_x.clamp(a0.min, a0.max) - a0.min) * a0.grid / (a0.max - a0.min);
        let ix = px.trunc();
        let fx = px - ix;
        let ix = ix as i64;
        if dims == 1 {
            self.element(ix, 1.0 - fx);
            self.element(ix + 1, fx);
        } else {
            let a1 = a1.expect("a 2D space's second axis");
            let py = (self.last_y.clamp(a1.min, a1.max) - a1.min) * a1.grid / (a1.max - a1.min);
            let iy = py.trunc();
            let fy = py - iy;
            let iy = iy as i64;
            let ny = a1.grid as i64 + 1;
            self.element(ix * ny + iy, (1.0 - fx) * (1.0 - fy));
            self.element((ix + 1) * ny + iy, fx * (1.0 - fy));
            self.element(ix * ny + iy + 1, (1.0 - fx) * fy);
            self.element((ix + 1) * ny + iy + 1, fx * fy);
        }
        let s = &self.data.space;
        let (rs, rw, rr) = (&mut self.row_sample, &mut self.row_w, &mut self.row_rate);
        let clip = |k: i32| self.clips[k as usize];
        // rows of one animation into the earlier one; the later one is replaced by the last row and that index tested
        // again
        let mut i = 0;
        while i < self.count {
            let mut j = i + 1;
            while j < self.count {
                if clip(rs[i]) != clip(rs[j]) {
                    j += 1;
                    continue;
                }
                let (w1, w2) = (clamp01(f64::from(rw[i])), clamp01(f64::from(rw[j])));
                if (w1 + w2).abs() > 1e-8 {
                    rr[i] = f32r((w1 * f64::from(rr[i]) + w2 * f64::from(rr[j])) / (w1 + w2));
                    rw[i] = f32r(f64::from(rw[i]) + w2);
                }
                self.count -= 1;
                let last = self.count;
                rs[j] = rs[last];
                rw[j] = rw[last];
                rr[j] = rr[last];
            }
            i += 1;
        }
        // by weight, descending: the smallest of the unsorted part (the first found among equals) to its end
        if self.count > 8 {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                eprintln!("warning: blend space {}: {} rows: the exe sorts more than 8 another way", s.name, self.count)
            });
        }
        let mut end = self.count;
        while end > 1 {
            end -= 1;
            let mut m = 0;
            for k in 1..=end {
                if rw[m] > rw[k] {
                    m = k;
                }
            }
            if m != end {
                rs.swap(m, end);
                rw.swap(m, end);
                rr.swap(m, end);
            }
        }
        let mut sum = 0.0;
        let mut keep = 0;
        while keep < self.count && !(f64::from(rw[keep]) < MIN_W) {
            sum += f64::from(rw[keep]);
            keep += 1;
        }
        self.count = keep;
        for w in &mut rw[..keep] {
            *w = f32r(f64::from(*w) * (1.0 / sum));
        }
        keep > 0
    }

    /// The weighted length of the cache.
    pub fn length(&self, clips: &Clips) -> f64 {
        let mut len = 0.0;
        for k in 0..self.count {
            let Some(c) = self.clips[self.row_sample[k] as usize] else { continue };
            let a = &clips.clips[c].anim;
            let rate = f64::from(self.row_rate[k]) * a.rate_scale;
            len += a.length / (if rate != 0.0 { rate.abs() } else { 1.0 }) * clamp01(f64::from(self.row_w[k]));
        }
        len
    }

    /// The leader's time advance, after select. The tick that follows an init advances too.
    pub fn advance(&mut self, clips: &Clips, dt: f64) {
        if self.count == 0 {
            return;
        }
        let len = self.length(clips);
        let mut cur = self.t * len + self.data.play_rate * dt;
        if cur < 0.0 || cur > len {
            if !self.data.looped {
                cur = cur.clamp(0.0, len);
            } else if len == 0.0 {
                cur = 0.0;
            } else {
                cur %= len;
                if cur < 0.0 {
                    cur += len;
                }
            }
        }
        self.t = if len != 0.0 { cur / len } else { 0.0 };
    }

    /// A row's clip time: clamp01 of the normalised time, 1 - it at a negative rate, x the clip's length.
    fn row_time(&self, k: usize, clips: &Clips, c: usize) -> f64 {
        let u = clamp01(self.t);
        let a = &clips.clips[c].anim;
        (if f64::from(self.row_rate[k]) * a.rate_scale < 0.0 { 1.0 - u } else { u }) * a.length
    }

    /// The blend of full-pose samples into `out` (an empty cache: the reference pose). `s`: a scratch pose.
    pub fn pose(&self, clips: &mut Clips, rig: &Rig, rest: &Pose, out: &mut Pose, s: &mut Pose, root_index: i32) {
        if self.count == 0 {
            out.copy(rest);
            return;
        }
        for k in 0..self.count {
            let c = self.clips[self.row_sample[k] as usize].expect("a selected sample's clip");
            let w = clamp01(f64::from(self.row_w[k]));
            let t = self.row_time(k, clips, c);
            sample_clip(clips, c, rig, rest, t, s, root_index);
            if k == 0 {
                weight_pose(s, w, out);
            } else {
                add_weighted_pose(s, w, out);
            }
        }
        normalize_rotations(out);
    }

    /// The rig bones any sample's clip has a track for, in rig order: an additive blend's delta is the identity on the
    /// others (accumulate_held's `bones`). Made once, by the caller.
    pub fn tracked_bones(&self, rig: &Rig) -> Vec<usize> {
        let mut on = vec![false; rig.names.len()];
        for c in self.clips.iter().flatten() {
            for &i in rig.tracked(*c) {
                on[i] = true;
            }
        }
        (0..on.len()).filter(|&i| on[i]).collect()
    }

    /// The delta poses of the additive samples whose clips do not move (every track one key), once.
    pub fn cache_deltas(&mut self, clips: &mut Clips, rig: &Rig, rest: &Pose) {
        let mut made: Vec<(usize, usize)> = Vec::new();
        let mut poses = Vec::new();
        let mut out = Vec::new();
        for c in &self.clips {
            let Some(c) = *c else {
                out.push(None);
                continue;
            };
            if !clips.clips[c].anim.still {
                out.push(None);
                continue;
            }
            let k = match made.iter().find(|m| m.0 == c) {
                Some(m) => m.1,
                None => {
                    let mut p = Pose::new(rig.names.len());
                    assert!(clips.clips[c].additive, "{} is not an additive sequence", clips.clips[c].anim.path);
                    sample_clip(clips, c, rig, rest, 0.0, &mut p, -1);
                    poses.push(p);
                    made.push((c, poses.len() - 1));
                    poses.len() - 1
                }
            };
            out.push(Some(k));
        }
        self.deltas = Some(out);
        self.delta_poses = poses;
    }

    /// The blend of additive samples into `out` as a delta pose (an empty cache: the additive identity; Unverified:
    /// IsValidAdditive 1428a0840 was not read). `s`: a scratch pose.
    pub fn additive_pose(&self, clips: &mut Clips, rig: &Rig, rest: &Pose, out: &mut Pose, s: &mut Pose) {
        if self.count == 0 {
            out.t.fill(0.0);
            out.s.fill(0.0);
            for q in out.r.as_chunks_mut::<4>().0 {
                *q = [0.0, 0.0, 0.0, 1.0];
            }
            return;
        }
        for k in 0..self.count {
            let si = self.row_sample[k] as usize;
            let c = self.clips[si].expect("a selected sample's clip");
            let w = clamp01(f64::from(self.row_w[k]));
            let held = self.deltas.as_ref().and_then(|d| d[si]).map(|i| &self.delta_poses[i]);
            let d = match held {
                Some(d) => d,
                None => {
                    assert!(clips.clips[c].additive, "{} is not an additive sequence", clips.clips[c].anim.path);
                    let t = self.row_time(k, clips, c);
                    sample_clip(clips, c, rig, rest, t, s, -1);
                    &*s
                }
            };
            if k == 0 {
                weight_pose(d, w, out);
            } else {
                add_weighted_pose(d, w, out);
            }
        }
        normalize_rotations(out);
    }
}
