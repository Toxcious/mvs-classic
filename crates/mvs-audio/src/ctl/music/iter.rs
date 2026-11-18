//! The order a music playlist plays its segments (the build's AkRSIterator: Init 1800151e0, JumpNext 180015430), with
//! the play state of its groups (CAkSequenceInfo, CAkRandomInfo).

use std::collections::BTreeMap;

use mvs_data::sound::{Id, MusicItem};

use super::World;

/// The weight of an item that sets none.
const STD_WEIGHT: u32 = 50_000;

/// Play state of a group: the index of a sequence (CAkSequenceInfo) or, of a random group (CAkRandomInfo), the items
/// left in the cycle, the items and the weight that can be picked, the avoid list and the two flag sets.
#[derive(Debug, Default)]
pub(super) struct RsInfo {
    i: i64,
    left: i64,
    rem_n: i64,
    rem_w: i64,
    avoid: Vec<usize>,
    played: Vec<bool>,
    blocked: Vec<bool>,
}

impl Clone for RsInfo {
    fn clone(&self) -> RsInfo {
        let mut x = RsInfo::default();
        x.clone_from(self);
        x
    }

    /// Into the lists already there (a step group's state written back at each segment start).
    fn clone_from(&mut self, s: &RsInfo) {
        (self.i, self.left, self.rem_n, self.rem_w) = (s.i, s.left, s.rem_n, s.rem_w);
        self.avoid.clone_from(&s.avoid);
        self.played.clone_from(&s.played);
        self.blocked.clone_from(&s.blocked);
    }
}

/// The step groups' states between the activations of a playlist (the container's global info,
/// CAkMusicRanSeqCntr +0x160, written when a segment starts: AkRSIterator::NotifySegmentPlay 180015a70).
pub(super) type StepInfo = BTreeMap<Id, RsInfo>;

fn weight(n: &MusicItem) -> i64 {
    i64::from(n.weight)
}

fn is_step(n: &MusicItem) -> bool {
    n.rs == 1 || n.rs == 3
}

fn has_leaf(n: &MusicItem) -> bool {
    n.seg != 0 || n.children.iter().any(has_leaf)
}

fn flag(v: &[bool], k: usize) -> bool {
    v.get(k).copied().unwrap_or(false)
}

fn set_flag(v: &mut Vec<bool>, k: usize, on: bool) {
    if v.len() <= k {
        v.resize(k + 1, false);
    }
    v[k] = on;
}

/// A group on the stack: its child index in the group below it (the root: none).
struct Frame {
    child: Option<usize>,
    loops: i64,
    inf: bool,
    /// A step group's state is in the iterator's `own`, by the item's id; any other group's is here.
    info: RsInfo,
}

/// One activation of a playlist's order.
pub(super) struct Iter {
    root: Option<MusicItem>,
    stack: Vec<Frame>,
    /// This activation's copies of the step groups' states.
    own: StepInfo,
    seg: Id,
    /// Plays left of the current segment, 0 = endless.
    reps: i64,
    begun: bool,
}

impl Iter {
    pub(super) fn new(root: Option<MusicItem>) -> Iter {
        Iter { root, stack: Vec::new(), own: StepInfo::new(), seg: 0, reps: 1, begun: false }
    }

    /// CAkRSNode::GetInitialLoopCountLoop (1800150a0): Loop, or a count drawn in [max(Loop + LoopMin, 1), Loop + LoopMax].
    fn count(n: &MusicItem, w: &mut dyn World) -> i64 {
        let [a, b] = n.loop_range.map_or([0, 0], |r| r.map(i64::from));
        let loops = i64::from(n.loop_count);
        if loops == 0 || (a == 0 && b == 0) {
            return loops;
        }
        let lo = (loops + a).max(1);
        let hi = loops + b;
        if hi <= lo {
            return loops;
        }
        lo + (w.draw() * (hi - lo + 1) as f64).floor() as i64
    }

    /// The group of the stack's frame `depth`.
    fn node(&self, depth: usize) -> &MusicItem {
        node_at(self.root.as_ref().expect("a root"), &self.stack[..=depth])
    }

    /// StackItem (180016610): a step group never ends by itself (PopObsoleteStackedItems counts its visits). `child`:
    /// the group's index in the top group (None: the root).
    fn push(&mut self, child: Option<usize>, step: &StepInfo, w: &mut dyn World) {
        let root = self.root.as_ref().expect("a root");
        let top = if self.stack.is_empty() { root } else { node_at(root, &self.stack) };
        let n = match child {
            Some(k) => &top.children[k],
            None => root,
        };
        let fresh = RsInfo {
            i: -1,
            left: n.children.len() as i64,
            rem_n: n.children.len() as i64,
            rem_w: n.children.iter().map(weight).sum(),
            ..RsInfo::default()
        };
        let mut info = RsInfo::default();
        if is_step(n) {
            self.own.entry(n.id).or_insert_with(|| step.get(&n.id).cloned().unwrap_or(fresh));
        } else {
            info = fresh;
        }
        let loops = Self::count(n, w);
        let inf = is_step(n) || n.loop_count == 0;
        self.stack.push(Frame { child, loops, inf, info });
    }

    /// Select (180015f10) / SelectRandomly (180016010) with UpdateRandomItem (180016b30): the next child, -1 when the
    /// group has played its loops.
    fn select(&mut self, w: &mut dyn World) -> i64 {
        let root = self.root.as_ref().expect("a root");
        let n = node_at(root, &self.stack);
        let top = self.stack.last_mut().expect("select on an empty stack");
        let info = if is_step(n) { self.own.entry(n.id).or_default() } else { &mut top.info };
        select_in(n, &mut top.loops, top.inf, info, w)
    }

    /// PopObsoleteStackedItems (180015e50): a step group gives one item per visit and is left after Loop visits.
    fn unstep(&mut self) {
        while let Some(d) = self.stack.len().checked_sub(1) {
            let step = is_step(self.node(d));
            let top = &mut self.stack[d];
            if !(step && top.loops != 0) {
                break;
            }
            if top.loops > 1 {
                top.loops -= 1;
                return;
            }
            self.stack.pop();
        }
    }

    fn descend(&mut self, step: &StepInfo, w: &mut dyn World) -> Option<Id> {
        while !self.stack.is_empty() {
            let k = self.select(w);
            if k < 0 {
                self.stack.pop();
                self.unstep();
                continue;
            }
            let k = k as usize;
            let c = &self.node(self.stack.len() - 1).children[k];
            if c.seg != 0 {
                let seg = c.seg;
                self.reps = Self::count(c, w);
                self.seg = seg;
                return Some(seg);
            }
            // (a group with no segment below it is passed over; the bank has none)
            if has_leaf(c) {
                self.push(Some(k), step, w);
            }
        }
        None
    }

    /// The next segment, None when the playlist has played out.
    pub(super) fn next(&mut self, step: &StepInfo, w: &mut dyn World) -> Option<Id> {
        if !self.begun {
            self.begun = true;
            let r = self.root.as_ref()?;
            if !has_leaf(r) {
                return None;
            }
            if r.seg != 0 {
                let seg = r.seg;
                self.reps = Self::count(r, w);
                self.seg = seg;
                return Some(seg);
            }
            self.push(None, step, w);
            return self.descend(step, w);
        }
        if self.seg == 0 {
            return None;
        }
        if self.reps == 0 {
            return Some(self.seg);
        }
        if self.reps > 1 {
            self.reps -= 1;
            return Some(self.seg);
        }
        self.seg = 0;
        self.unstep();
        self.descend(step, w)
    }

    /// The segment picked last starts playing: the step groups' states become the container's.
    pub(super) fn played(&self, step: &mut StepInfo) {
        for (id, x) in &self.own {
            match step.get_mut(id) {
                Some(s) => s.clone_from(x),
                None => {
                    step.insert(*id, x.clone());
                }
            }
        }
    }
}

/// The group a stack of frames leads to from the root.
fn node_at<'a>(root: &'a MusicItem, frames: &[Frame]) -> &'a MusicItem {
    frames.iter().filter_map(|f| f.child).fold(root, |n, k| &n.children[k])
}

fn select_in(n: &MusicItem, loops: &mut i64, inf: bool, x: &mut RsInfo, w: &mut dyn World) -> i64 {
    let c = &n.children;
    let over = |loops: &mut i64| {
        if inf {
            false
        } else {
            *loops -= 1;
            *loops == 0
        }
    };
    if n.rs < 2 {
        if x.i + 1 != c.len() as i64 {
            x.i += 1;
            return x.i;
        }
        x.i = 0;
        return if over(loops) { -1 } else { 0 };
    }
    let lim = n.avoid as usize;
    let shuffle = n.shuffle;
    if x.left == 0 {
        // every item has played: a new cycle, the avoid list still standing
        if over(loops) {
            return -1;
        }
        x.left = c.len() as i64;
        x.played.clear();
        x.rem_n = c.len() as i64 - x.avoid.len() as i64;
        x.rem_w = c.iter().map(weight).sum::<i64>() - x.avoid.iter().map(|&k| weight(&c[k])).sum::<i64>();
    }
    let ok = |x: &RsInfo, k: usize| {
        if shuffle { !flag(&x.played, k) && !flag(&x.blocked, k) } else { lim == 0 || !flag(&x.blocked, k) }
    };
    let by_weight = c.iter().any(|n| n.weight != STD_WEIGHT);
    let pool = if by_weight { x.rem_w } else { x.rem_n };
    if pool == 0 {
        x.left = 0;
        return 0;
    }
    let r = (w.draw() * pool as f64).floor() as i64;
    let mut k = 0usize;
    let mut acc = -1i64;
    while acc < r && k < c.len() {
        if ok(x, k) {
            acc += if by_weight { weight(&c[k]) } else { 1 };
        }
        k += 1;
    }
    k -= 1;
    let cap = (if shuffle { lim.max(1) } else { lim }).min(c.len() - 1);
    if shuffle {
        // each item once per cycle
        x.rem_w -= weight(&c[k]);
        x.rem_n -= 1;
        x.left -= 1;
        set_flag(&mut x.played, k, true);
    } else {
        if !flag(&x.played, k) {
            set_flag(&mut x.played, k, true);
            x.left -= 1;
        }
        if lim == 0 {
            return k as i64;
        }
        x.rem_w -= weight(&c[k]);
        x.rem_n -= 1;
    }
    x.avoid.push(k);
    set_flag(&mut x.blocked, k, true);
    if x.avoid.len() > cap {
        let f = x.avoid.remove(0);
        set_flag(&mut x.blocked, f, false);
        if !shuffle || !flag(&x.played, f) {
            x.rem_w += weight(&c[f]);
            x.rem_n += 1;
        }
    }
    k as i64
}
