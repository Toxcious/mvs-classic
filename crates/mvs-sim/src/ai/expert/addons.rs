//! Per-fighter add-ons to the Expert's shortlist: a move of a fighter's kit that the generic scripts leave out and the
//! arena shows paying takes the place of one of a situation's scripts, so the shortlist keeps its length and a
//! decision its cost.

use super::plan::{Plan, Situation, attack};
use crate::Real;
use crate::ai::pad::Btn;

/// In `situation`, fighter `key`'s script at `slot` (of the situation's scripts: "what the tree does" is not
/// counted) becomes a press of `btn` with the stick at (`x` toward the target, `y`).
struct AddOn {
    key: &'static str,
    situation: Situation,
    slot: usize,
    x: Real,
    y: Real,
    btn: Btn,
}

const ADD_ONS: &[AddOn] = &[
    // Stripe: Dynamite Boom (C023B_Special_Bomb_Equip_Montage, the bomb his next down special sets off) for the
    // short hop at a distance.
    AddOn { key: crate::chars::stripe::KEY, situation: Situation::Far, slot: 4, x: 0.0, y: -1.0, btn: Btn::Special },
];

/// The add-ons of fighter `key` in `situation` put in the shortlist `out` ("what the tree does" first); `t` is the
/// side of the target (-1 or 1).
pub fn apply(key: &str, situation: Situation, t: Real, out: &mut [Plan]) {
    for a in ADD_ONS {
        if a.key == key
            && a.situation == situation
            && let Some(p) = out.get_mut(a.slot + 1)
        {
            *p = attack(t * a.x, a.y, a.btn);
        }
    }
}
