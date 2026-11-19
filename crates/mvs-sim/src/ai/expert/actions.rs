//! The Expert's action list: every plan a decision can weigh, each with a fixed index, the same for every fighter
//! and situation. A script's stick is given relative to the decision: toward the target (`T`), away from it (`-T`),
//! toward the stage's centre (`S`) or straight. The hand-made shortlist (`plan::shortlist`) picks from it by
//! situation; the network (`net`) scores all of it, and the look-ahead tries its best few.

use super::plan::{self, FULL_JUMP_HOLD, Plan, Script};
use crate::Real;
use crate::ai::pad::Btn;

/// The stick's x of an action: toward the target, away from it, toward the stage's centre, or none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    T,
    Away,
    S,
    No,
}

/// An action of the list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// What the tree does.
    Tree,
    /// A press of the button with the stick at (`x`, `y`), then neutral (`plan::attack`).
    Press(Side, i8, Btn),
    /// An attack held `CHARGE_HOLD` ticks: a charged attack.
    Charge(Side, i8),
    /// A press with the stick at (`x`, `y`), then drifting toward the third side (an aerial on the move, an up
    /// special back to the stage).
    Drift(Side, i8, Btn, Side),
    /// The stick held toward a side.
    Walk(Side),
    /// A full jump drifting toward a side.
    Jump(Side),
    /// A short hop drifting toward a side.
    Hop(Side),
    /// A dodge with the stick at (`x`, `y`).
    Dodge(Side, i8),
    /// The stick down.
    FastFall,
    /// Nothing pressed, the stick neutral.
    Idle,
}

/// The ticks a charged attack holds the button, and lasts.
const CHARGE_HOLD: i32 = 10;
const CHARGE_LEN: i32 = 14;
/// The ticks the idle action lasts.
const IDLE_LEN: i32 = plan::MOVE_LEN;

use Action::{Charge, Dodge, Drift, FastFall, Hop, Idle, Jump, Press, Tree, Walk};
use Btn::{Attack, Special};
use Side::{Away, No, S, T};

/// The list, in its fixed order (the network's outputs follow it). "What the tree does" is first.
pub const ACTIONS: [Action; 38] = [
    Tree,
    Press(T, 0, Attack),
    Press(Away, 0, Attack),
    Press(No, 1, Attack),
    Press(No, -1, Attack),
    Press(No, 0, Attack),
    Press(T, 0, Special),
    Press(Away, 0, Special),
    Press(No, 1, Special),
    Press(No, -1, Special),
    Press(No, 0, Special),
    Charge(T, 0),
    Charge(No, 1),
    Charge(No, -1),
    Drift(No, 1, Special, S),
    Drift(S, 0, Special, S),
    Drift(T, 0, Attack, T),
    Drift(No, -1, Attack, T),
    Drift(No, 1, Attack, T),
    Drift(No, 0, Attack, T),
    Walk(T),
    Walk(Away),
    Walk(S),
    Jump(T),
    Jump(Away),
    Jump(No),
    Jump(S),
    Hop(T),
    Hop(Away),
    Hop(No),
    Dodge(T, 0),
    Dodge(Away, 0),
    Dodge(No, 0),
    Dodge(S, 1),
    Dodge(No, 1),
    Dodge(No, -1),
    FastFall,
    Idle,
];
/// The count of actions.
pub const COUNT: usize = ACTIONS.len();

/// The stick's x of `side`: `t` the target's side and `s` the centre's (-1 or 1).
fn x_of(side: Side, t: Real, s: Real) -> Real {
    match side {
        T => t,
        Away => -t,
        S => s,
        No => 0.0,
    }
}

/// Action `a`'s plan, `t` the side of the target and `s` the side of the stage's centre (-1 or 1).
pub fn plan_of(a: Action, t: Real, s: Real) -> Plan {
    let x = |side| x_of(side, t, s);
    match a {
        Tree => Plan::Tree { len: plan::TREE_LEN },
        Press(sx, y, btn) => plan::attack(x(sx), Real::from(y), btn),
        Charge(sx, y) => Plan::Script(Script {
            x: x(sx),
            y: Real::from(y),
            stick: 2,
            drift: 0.0,
            btn: Some(Attack),
            hold: CHARGE_HOLD,
            len: CHARGE_LEN,
        }),
        Drift(sx, y, btn, d) => {
            let Plan::Script(mut sc) = plan::attack(x(sx), Real::from(y), btn) else { unreachable!("a script") };
            sc.drift = x(d);
            Plan::Script(sc)
        }
        Walk(sx) => plan::walk(x(sx)),
        Jump(sx) => plan::jump(x(sx), FULL_JUMP_HOLD),
        Hop(sx) => plan::jump(x(sx), 1),
        Dodge(sx, y) => plan::dodge(x(sx), Real::from(y)),
        FastFall => plan::fast_fall(),
        Idle => Plan::Script(Script { x: 0.0, y: 0.0, stick: 0, drift: 0.0, btn: None, hold: 0, len: IDLE_LEN }),
    }
}

/// Every action's plan into `out` (cleared first), in the list's order.
pub fn plans(t: Real, s: Real, out: &mut Vec<Plan>) {
    out.clear();
    out.extend(ACTIONS.iter().map(|&a| plan_of(a, t, s)));
}
