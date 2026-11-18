//! The AnimBP's Fall and Wall blend-space players (FAnimNode_BlendSpacePlayer's 1D path): the heaviest row as the
//! state's clip, its rate, and the other rows as the slot mix.
//!
//! AirState Fall with UseFallBlendspace (`AnimSet::fall_space`): Base_AnimBP's BlendListByBool_17 under
//! ModifyBone_8 plays its pose 0, a BlendSpacePlayer over FallBlendSpace (looped, StartPosition 0, no sync group:
//! the fighter import) on X = VerticalSpeedDown = FClamp(-Velocity.Z, 0, 10000) (SetHorizontalAndVerticalSpeed
//! @3165-3254, read in the update with the velocity of that moment), in place of Anim_Fall_01 at 1.5. Its blend
//! time is 0 both ways and the bool a class default, so the set plays one or the other for the whole match. The
//! player is FAnimNode_BlendSpacePlayer's 1D path: the input clamped to the axis and made a grid position, the two
//! cooked grid elements around it (weights 1 - frac and frac), the rows merged by clip, sorted by weight, cut at
//! 1e-5 and normalised; the normalised time advances by PlayRate x dt over the weighted length (sum of clip length /
//! |RateScale x clip RateScale| x weight), each row's clip at that time x its length.
//! In the port the state's clip (`Anim::loco_anim`) is the heaviest row, its time the normalised time x its length
//! and its rate PlayRate x its length over the weighted length (so the loco time moves as the player's time does);
//! a change of the heaviest row swaps the clip in place, as idleSwap does. The other rows are the character's slot
//! mix ([`fall_mix`]: CharHooks.slotMix, lerp in order), so the shapes and the drawn pose are the blend.
//! Unverified: the input filter (no interpolation in the asset: the value passes) is taken as the raw X; the time of
//! an AirState state held by a transition (smTimes) is in seconds of the clip it left with, so a change of the
//! heaviest row while held reads it in the other clip (Batman's two samples are both 0.5 s).
//! The selection, the weights and the weighted length take the build's float32 steps. Deviation: the time is kept
//! as the heaviest clip's seconds (`loco_time` at `loco_rate`), not as the node's float normalised time
//! (CurrentTime = t x Length + PlayRate x dt, t = CurrentTime / Length): the same advance in other roundings.
//!
//! The Wall machine with UseWallBlendSpace (`AnimSet::wall_spaces`, C018 alone): Wall Enter's and Wall Idle's
//! BlendListByBool (_21 / _22, bActiveValue UseWallBlendSpace) play pose 0, a BlendSpacePlayer over the set's
//! BlendSpace_WallEnter (not looped) / BlendSpace_WallIdle (looped), PlayRate 1, StartPosition 0, no sync group, on
//! X = WallBlendSpaceValue (the exposed inputs; the fighter import checks them). The player is the Fall player's 1D
//! path (the weighted length, the heaviest row as the loco clip, the other rows as the slot mix: [`wall_mix`]), with
//! the same two notes as the Fall player. A non-looped player's normalised time stops at 1: the loco time runs on
//! past the clip's length as a sequence's does, and the clip is read at min(locoTime, length) (updatePose) and the
//! rows at clamp01 of it. The other Wall machine's players (SM6, the AnimGraph's copy: _3 / _4) are the same;
//! WallJump's (BlendSpace_WallJump) is never shown (updatePose).

// `!(a < b)` is true for a NaN where `a >= b` is not; the written-out clamp keeps a NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::manual_clamp)]

use super::{AnimSet, GroundState, SlotPose};
use crate::Real;
use crate::fighter::FighterId;
use crate::math::{max, min, to_int32};
use crate::names::Name;
use crate::world::World;
use mvs_data::fighter::{FallSpace, SpaceSample, WallSpace};

/// AirState Fall's sequence player PlayRate.
const FALL_RATE: Real = 1.5;
/// Two grid elements of three slots.
const FS_MAX: usize = 6;
/// ZERO_ANIMWEIGHT_THRESH.
const MIN_FS_W: Real = 1e-5;

fn clamp01(x: Real) -> Real {
    if x < 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

/// A 1D blend space as the player reads it (FallSpace and WallSpace alike).
#[derive(Clone, Copy)]
pub struct Space<'a> {
    pub play_rate: Real,
    pub min: Real,
    pub max: Real,
    pub grid: Real,
    pub samples: &'a [SpaceSample],
    pub cells: &'a [f64],
}

impl<'a> From<&'a FallSpace> for Space<'a> {
    fn from(s: &'a FallSpace) -> Self {
        Space {
            play_rate: crate::math::narrow(s.play_rate),
            min: crate::math::narrow(s.min),
            max: crate::math::narrow(s.max),
            grid: crate::math::narrow(s.grid),
            samples: &s.samples,
            cells: &s.cells,
        }
    }
}

impl<'a> From<&'a WallSpace> for Space<'a> {
    fn from(s: &'a WallSpace) -> Self {
        Space {
            play_rate: crate::math::narrow(s.play_rate),
            min: crate::math::narrow(s.min),
            max: crate::math::narrow(s.max),
            grid: crate::math::narrow(s.grid),
            samples: &s.samples,
            cells: &s.cells,
        }
    }
}

/// The rows of one selection, heaviest first (`fsSample` / `fsW` / `fsRate` / `fsCount`; its float32
/// stores are `fround`s here).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rows {
    pub sample: [usize; FS_MAX],
    pub w: [Real; FS_MAX],
    pub rate: [Real; FS_MAX],
    pub count: usize,
}

impl Rows {
    fn swap(&mut self, a: usize, b: usize) {
        self.sample.swap(a, b);
        self.w.swap(a, b);
        self.rate.swap(a, b);
    }

    /// The heaviest row's clip.
    pub fn head<'a>(&self, sp: Space<'a>) -> Option<&'a str> {
        (self.count > 0).then(|| sp.samples[self.sample[0]].anim.as_str())
    }
}

/// `fsElement`: one grid element's three slots into the rows.
fn element(sp: Space, rows: &mut Rows, row: i32, w: Real) {
    let g = sp.cells;
    let Ok(row) = usize::try_from(row) else { return };
    if row * 6 >= g.len() {
        return;
    }
    for k in 0..3 {
        let si = g[row * 6 + k];
        if si < 0.0 {
            continue;
        }
        let si = usize::try_from(crate::math::dbl::to_int32(si)).expect("a blend space sample index");
        let mut r = 0;
        while r < rows.count && rows.sample[r] != si {
            r += 1;
        }
        if r == rows.count {
            rows.sample[r] = si;
            rows.w[r] = 0.0;
            rows.rate[r] = crate::math::narrow(sp.samples[si].rate);
            rows.count += 1;
        }
        rows.w[r] += w * crate::math::narrow(g[row * 6 + 3 + k]);
    }
}

/// `select1D`: the rows of a 1D blend space player at the input x (the Fall and the Wall states' players).
pub fn select_1d(sp: Space, x: Real) -> Rows {
    let mut rows = Rows::default();
    let px = ((min(max(x, sp.min), sp.max) - sp.min) * sp.grid) / (sp.max - sp.min);
    assert!(!px.is_nan(), "a blend space input of NaN");
    let ix = px.trunc();
    let fx = px - ix;
    let ix = to_int32(ix);
    element(sp, &mut rows, ix, 1.0 - fx);
    element(sp, &mut rows, ix + 1, fx);
    let mut i = 0;
    while i < rows.count {
        let mut j = i + 1;
        while j < rows.count {
            if sp.samples[rows.sample[i]].anim != sp.samples[rows.sample[j]].anim {
                j += 1;
                continue;
            }
            let w1 = clamp01(rows.w[i]);
            let w2 = clamp01(rows.w[j]);
            if (w1 + w2).abs() > 1e-8 {
                rows.rate[i] = ((w1 * rows.rate[i]) + (w2 * rows.rate[j])) / (w1 + w2);
                rows.w[i] += w2;
            }
            rows.count -= 1;
            let last = rows.count;
            rows.sample[j] = rows.sample[last];
            rows.w[j] = rows.w[last];
            rows.rate[j] = rows.rate[last];
        }
        i += 1;
    }
    let mut end = rows.count;
    while end > 1 {
        end -= 1;
        let mut m = 0;
        for k in 1..=end {
            if rows.w[m] > rows.w[k] {
                m = k;
            }
        }
        if m != end {
            rows.swap(m, end);
        }
    }
    let mut sum = 0.0;
    let mut keep = 0;
    while keep < rows.count && !(rows.w[keep] < MIN_FS_W) {
        sum += rows.w[keep];
        keep += 1;
    }
    rows.count = keep;
    let inv = 1.0 / sum;
    for k in 0..keep {
        rows.w[k] *= inv;
    }
    rows
}

/// The fighter's anim set (`f.animSet`, the getter's live hook call).
pub(super) fn set_of(w: &World, f: FighterId) -> &AnimSet {
    crate::fighter::anim_set_def(w, f)
}

/// A clip's sequence RateScale from the box index (1 without it).
fn rate_scale(w: &World, f: FighterId, a: &str) -> Real {
    super::montages::char_data(&w.data, &w.names, w.fighter(f).ch)
        .and_then(|c| c.boxes.anims.get(a))
        .map_or(1.0, |x| crate::math::narrow(x.rate_scale))
}

/// `animLen`, of the set the getter gives now.
fn anim_len(w: &World, f: FighterId, a: &str) -> Real {
    super::update::anim_len(&super::update::parts_of(&w.data, w, f), a)
}

/// The interned name of a clip of the data (`Name::NONE` for a clip the match never interned).
fn name_of(w: &World, s: &str) -> Name {
    super::update::nm(&w.names, s)
}

/// `fallLength`: the weighted length of the rows (`len`: a clip's `animLen`; `scale`: its RateScale).
fn rows_length(sp: Space, rows: &Rows, len_of: &dyn Fn(&str) -> Real, scale: &dyn Fn(&str) -> Real) -> Real {
    let mut len = 0.0;
    for k in 0..rows.count {
        let a = &sp.samples[rows.sample[k]].anim;
        let rate = rows.rate[k] * scale(a);
        len += (len_of(a) / if rate != 0.0 { rate.abs() } else { 1.0 }) * clamp01(rows.w[k]);
    }
    len
}

/// `fallSelect`: the rows of this update, on VerticalSpeedDown.
fn fall_select(w: &World, f: FighterId, sp: Space) -> Rows {
    select_1d(sp, min(max(-w.fighter(f).body.vy, 0.0), 10000.0))
}

/// `fallAnim`: the Fall state's clip where a state machine enters or reads it: Anim_Fall_01 at 1.5, or the heaviest
/// row of FallBlendSpace at this moment's X.
pub fn fall_anim(w: &World, f: FighterId) -> Name {
    let set = set_of(w, f);
    let fall = || set.fall.as_deref().map_or(Name::NONE, |a| name_of(w, a));
    let Some(sp) = &set.fall_space else { return fall() };
    let sp = Space::from(sp);
    match fall_select(w, f, sp).head(sp) {
        Some(a) => name_of(w, a),
        None => fall(),
    }
}

/// `fallRate`: the Fall state's rate (after the selection).
pub fn fall_rate(w: &World, f: FighterId) -> Real {
    let Some(sp) = &set_of(w, f).fall_space else { return FALL_RATE };
    let sp = Space::from(sp);
    let rows = fall_select(w, f, sp);
    rows_rate(w, f, sp, &rows)
}

/// PlayRate x the heaviest clip's length over the weighted length (0 with no row).
fn rows_rate(w: &World, f: FighterId, sp: Space, rows: &Rows) -> Real {
    rows_rate_by(sp, rows, &|a| anim_len(w, f, a), &|a| rate_scale(w, f, a))
}

/// [`rows_rate`] over a clip's `animLen` (`len_of`) and RateScale (`scale`).
pub fn rows_rate_by(sp: Space, rows: &Rows, len_of: &dyn Fn(&str) -> Real, scale: &dyn Fn(&str) -> Real) -> Real {
    let Some(head) = rows.head(sp) else { return 0.0 };
    let len = rows_length(sp, rows, len_of, scale);
    if len != 0.0 { (sp.play_rate * len_of(head)) / len } else { 0.0 }
}

/// The loco clip swapped in place to `head` at the same normalised time (`loop`: its looping), the rate set.
fn swap_head(w: &mut World, f: FighterId, head: Name, rate: Real, looping: bool) {
    let fi = w.fighter(f);
    let loco = fi.anim.loco_anim;
    let swap = (loco != head).then(|| {
        // clamp01 written out (keeps the sign of a -0).
        let x = fi.anim.loco_time / anim_len(w, f, w.names.str(loco));
        let t = if x < 0.0 {
            0.0
        } else if x > 1.0 {
            1.0
        } else {
            x
        };
        (head, t * anim_len(w, f, w.names.str(head)))
    });
    let a = &mut w.fighter_mut(f).anim;
    if let Some((n, time)) = swap {
        a.loco_anim = n;
        a.loco_time = time;
        a.loco_loop = looping;
    }
    a.loco_rate = rate;
}

/// `fallSpaceTick`: the Fall state's update (updatePose, after the machines, ahead of the time advance): the
/// heaviest row is the state's clip, swapped in place at the same normalised time, and the rate follows the
/// weighted length.
pub fn fall_space_tick(w: &mut World, f: FighterId) {
    let Some(sp) = &set_of(w, f).fall_space else { return };
    if w.fighter(f).sm.gs != GroundState::Fall {
        return;
    }
    let sp = Space::from(sp);
    let rows = fall_select(w, f, sp);
    let rate = rows_rate(w, f, sp, &rows);
    let Some(head) = rows.head(sp) else { return };
    let head = name_of(w, head);
    swap_head(w, f, head, rate, true);
}

/// `fallMix`: the other rows of the Fall state's blend over the fighter's anim (the heaviest row), into a
/// character's slot mix (`SlotMix`: pose = lerp(pose, mix, w) in order): written at the end of updatePose and by
/// the character's own slot code where no montage is shown (Batman's slotCompose). Empty outside the Fall state,
/// under a shown montage, or without the blend space.
pub fn fall_mix(w: &World, f: FighterId, out: &mut Vec<SlotPose>) {
    let fi = w.fighter(f);
    if let Some(sp) = &set_of(w, f).fall_space
        && fi.sm.gs == GroundState::Fall
        && crate::fighter::shown_montage(w, f).is_none()
        && fi.anim.anim == fi.anim.loco_anim
    {
        let sp = Space::from(sp);
        let rows = fall_select(w, f, sp);
        mix_rows(w, f, sp, &rows, out);
    } else {
        out.clear();
    }
}

/// `mixRows`: the selected rows but the fighter's anim's into `out`, each at the state's normalised time, lerp
/// weights in order.
fn mix_rows(w: &World, f: FighterId, sp: Space, rows: &Rows, out: &mut Vec<SlotPose>) {
    let fi = w.fighter(f);
    let anim = w.names.str(fi.anim.anim);
    let t = clamp01(fi.anim.loco_time / anim_len(w, f, w.names.str(fi.anim.loco_anim)));
    out.clear();
    let mut sum = 0.0;
    for k in 0..rows.count {
        if sp.samples[rows.sample[k]].anim == anim {
            sum += rows.w[k];
        }
    }
    for k in 0..rows.count {
        let a = &sp.samples[rows.sample[k]].anim;
        if a == anim {
            continue;
        }
        sum += rows.w[k];
        let rate = rows.rate[k] * rate_scale(w, f, a);
        let time = (if rate < 0.0 { 1.0 - t } else { t }) * anim_len(w, f, a);
        let wt = if sum > 0.0 { rows.w[k] / sum } else { 0.0 };
        out.push(SlotPose { anim: name_of(w, a), time, w: wt });
    }
}

/// `inSpace`: the clip is a sample of the blend space.
pub fn in_space(sp: &WallSpace, a: &str) -> bool {
    sp.samples.iter().any(|s| s.anim == a)
}

/// `wallValue`: WallBlendSpaceValue as the instance holds it (the class default 0 where no character writes it).
fn wall_value(w: &World, f: FighterId) -> Real {
    w.fighter(f).hooks.char.wall_blend_space_value.map_or(0.0, |h| h(w, f))
}

/// `wallHead`: a Wall state's clip where the machine enters it, the heaviest row of its blend space at this
/// moment's X.
pub fn wall_head(w: &World, f: FighterId, sp: &WallSpace) -> Name {
    let rows = select_1d(sp.into(), wall_value(w, f));
    let Some(a) = rows.head(sp.into()) else {
        panic!("{}: {} selects no sample", w.names.str(w.fighter(f).ch), sp.name)
    };
    name_of(w, a)
}

/// `wallRate`: PlayRate x the heaviest clip's length over the weighted length. wallHead's selection is not reused:
/// this selects again on the same input (WallBlendSpaceValue is a read of the World), the same rows.
pub fn wall_rate(w: &World, f: FighterId, sp: &WallSpace) -> Real {
    let rows = select_1d(sp.into(), wall_value(w, f));
    rows_rate(w, f, sp.into(), &rows)
}

/// `wallSpaceNow`: the Wall state's blend space of the loco clip (none outside the Wall machine's two states).
fn wall_space_now(w: &World, f: FighterId) -> Option<&WallSpace> {
    let ws = set_of(w, f).wall_spaces.as_ref()?;
    let fi = w.fighter(f);
    if fi.sm.gs != GroundState::Wall {
        return None;
    }
    let la = w.names.str(fi.anim.loco_anim);
    if in_space(&ws.enter, la) {
        Some(&ws.enter)
    } else if in_space(&ws.idle, la) {
        Some(&ws.idle)
    } else {
        None
    }
}

/// `wallSpaceTick`: the Wall state's update (updatePose, after the machines, ahead of the time advance), as
/// [`fall_space_tick`].
pub fn wall_space_tick(w: &mut World, f: FighterId) {
    let Some(sp) = wall_space_now(w, f) else { return };
    let head = wall_head(w, f, sp);
    let rate = wall_rate(w, f, sp);
    let looping = sp.looping;
    swap_head(w, f, head, rate, looping);
}

/// `wallMix`: the other rows of the Wall state's blend over the fighter's anim, into the character's slot mix, as
/// [`fall_mix`].
pub fn wall_mix(w: &World, f: FighterId, out: &mut Vec<SlotPose>) {
    let fi = w.fighter(f);
    if let Some(sp) = wall_space_now(w, f)
        && crate::fighter::shown_montage(w, f).is_none()
        && fi.anim.anim == fi.anim.loco_anim
    {
        let rows = select_1d(sp.into(), wall_value(w, f));
        mix_rows(w, f, sp.into(), &rows, out);
    } else {
        out.clear();
    }
}
