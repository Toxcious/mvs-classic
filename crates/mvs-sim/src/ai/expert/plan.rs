//! The Expert's candidates: a plan is a short input script or "what the tree does" for some ticks, and the
//! shortlist a decision weighs is picked by the situation (in hitstun, off stage, the other in hitstun, on the
//! ground near or far, in the air), with the fighter's add-ons (`addons`) in it.

use crate::Real;
use crate::ai::pad::Btn;
use crate::fighter::FighterId;
use crate::input::{InputState, NO_INPUT};
use crate::math;
use crate::world::World;

/// The ticks a "what the tree does" plan lasts, and a chase.
pub(super) const TREE_LEN: i32 = 8;
pub(super) const CHASE_LEN: i32 = 10;
/// The chase's reach: within this of the target (|dx|, |dy|) it attacks, aimed up or down when the target is more
/// than half the height away. One reach for every fighter.
const CHASE_REACH_X: Real = 300.0;
const CHASE_REACH_Y: Real = 260.0;
/// The chase jumps (with a jump left) when the target is this much above it.
const CHASE_JUMP_Y: Real = 150.0;
/// The ticks an attack plan lasts: the gap, the press and the neutral stick while the move plays.
pub(super) const ATTACK_LEN: i32 = 10;
/// The ticks a move plan holds the stick.
pub(super) const MOVE_LEN: i32 = 8;
/// The ticks a jump plan lasts, and the ticks a full jump holds the button (a short hop holds it one).
const JUMP_LEN: i32 = 8;
pub(super) const FULL_JUMP_HOLD: i32 = 7;
/// The ticks a dodge plan lasts.
const DODGE_LEN: i32 = 8;
/// The ticks a fast fall holds the stick down.
const FAST_FALL_LEN: i32 = 6;
/// Under this horizontal distance (and `NEAR_Y` vertical) the other is near enough to attack from the ground.
const NEAR_X: Real = 450.0;
const NEAR_Y: Real = 300.0;
/// Below the floor's top by more than this, the bot is off stage even inside the floor's span.
const BELOW_FLOOR: Real = 50.0;

/// A candidate's input script. Its first tick is a gap (every button up, the stick already set), so the press on the
/// second tick is an edge whatever the last input held. The stick (`x`, `y`) is held for `stick` ticks, then `drift`
/// on x alone to the end; the button is held `hold` ticks from the second tick.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Script {
    pub x: Real,
    pub y: Real,
    pub stick: i32,
    pub drift: Real,
    pub btn: Option<Btn>,
    pub hold: i32,
    pub len: i32,
}

/// A plan: a script, the tree's own input for `len` ticks, or a chase (`chase`) for `len` ticks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Plan {
    Tree { len: i32 },
    Chase { len: i32 },
    Script(Script),
}

impl Plan {
    pub fn len(&self) -> i32 {
        match self {
            Plan::Tree { len } | Plan::Chase { len } => *len,
            Plan::Script(s) => s.len,
        }
    }

    /// The input of the plan's tick `k`; `tree` is the tree's input of that tick, `chase` the chase's.
    pub fn input(&self, k: i32, tree: InputState, chase: InputState) -> InputState {
        let s = match self {
            Plan::Tree { .. } => return tree,
            Plan::Chase { .. } => return chase,
            Plan::Script(s) => s,
        };
        let mut i = NO_INPUT;
        if k < s.stick {
            i.x = s.x;
            i.y = s.y;
        } else {
            i.x = s.drift;
        }
        if let Some(b) = s.btn
            && k >= 1
            && k < 1 + s.hold
        {
            match b {
                Btn::Attack => i.attack = true,
                Btn::Special => i.special = true,
                Btn::Jump => i.jump = true,
                Btn::Dodge => i.dodge = true,
                Btn::Item => i.item = true,
            }
        }
        i
    }
}

/// What a shortlist is picked by, in the order the tests are made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Situation {
    /// The bot in hitstun.
    Hitstun,
    /// The bot off the stage (`off_stage`).
    OffStage,
    /// The other in hitstun.
    Punish,
    /// On the ground, the other within reach.
    Near,
    /// On the ground, the other out of reach.
    Far,
    /// In the air.
    Air,
}

/// A plan queued from its first tick (a count of the bot's thinks).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Queued {
    pub start: i32,
    pub plan: Plan,
}

impl Queued {
    pub fn end(&self) -> i32 {
        self.start + self.plan.len()
    }
}

/// An attack or special: the stick (`x`, `y`) on the gap and the press, then neutral.
pub(super) fn attack(x: Real, y: Real, btn: Btn) -> Plan {
    Plan::Script(Script { x, y, stick: 2, drift: 0.0, btn: Some(btn), hold: 1, len: ATTACK_LEN })
}

/// A special pressed with the stick (`x`, `y`), then drifting toward `drift` (an up special back to the stage).
pub(super) fn special_drift(x: Real, y: Real, drift: Real) -> Plan {
    Plan::Script(Script { x, y, stick: 2, drift, btn: Some(Btn::Special), hold: 1, len: ATTACK_LEN })
}

/// The stick held toward `x`.
pub(super) fn walk(x: Real) -> Plan {
    Plan::Script(Script { x, y: 0.0, stick: MOVE_LEN, drift: 0.0, btn: None, hold: 0, len: MOVE_LEN })
}

/// A jump drifting toward `x`, the button held `hold` ticks.
pub(super) fn jump(x: Real, hold: i32) -> Plan {
    Plan::Script(Script { x, y: 0.0, stick: JUMP_LEN, drift: 0.0, btn: Some(Btn::Jump), hold, len: JUMP_LEN })
}

/// A dodge with the stick (`x`, `y`).
pub(super) fn dodge(x: Real, y: Real) -> Plan {
    Plan::Script(Script { x, y, stick: 2, drift: 0.0, btn: Some(Btn::Dodge), hold: 1, len: DODGE_LEN })
}

/// Nothing pressed, the stick neutral.
pub(super) fn idle() -> Plan {
    Plan::Script(Script { x: 0.0, y: 0.0, stick: 0, drift: 0.0, btn: None, hold: 0, len: MOVE_LEN })
}

/// The stick down: a fast fall.
pub(super) fn fast_fall() -> Plan {
    Plan::Script(Script { x: 0.0, y: -1.0, stick: FAST_FALL_LEN, drift: 0.0, btn: None, hold: 0, len: FAST_FALL_LEN })
}

/// The nearest fighter of another team that is alive (by |dx| + |dy|), the player list's order on a tie.
pub fn target(w: &World, me: FighterId) -> Option<FighterId> {
    let m = w.fighter(me);
    let mut best: Option<(Real, FighterId)> = None;
    for &f in &w.players {
        let o = w.fighter(f);
        if f == me || o.team.team == m.team.team || o.life.dead != 0.0 {
            continue;
        }
        let d = (o.body.x - m.body.x).abs() + (o.body.y - m.body.y).abs();
        if best.is_none_or(|(b, _)| d < b) {
            best = Some((d, f));
        }
    }
    best.map(|(_, f)| f)
}

/// The fighter is off the stage: in the air outside the floor's span or below its top.
pub fn off_stage(w: &World, f: FighterId) -> bool {
    let g = &w.stage.ground;
    let b = &w.fighter(f).body;
    !b.on_floor
        && (b.x < math::narrow(g.left) || b.x > math::narrow(g.right) || b.y < math::narrow(g.top) - BELOW_FLOOR)
}

/// The chase's input of fighter `f` at its tick `k` (a press on odd ticks, so each is an edge): toward its target,
/// jumping after one above it, and within reach an attack aimed at it. Fighter-agnostic: the attacker standing in for
/// the other player (`job`) and the bot's own `Chase` action.
pub fn chase(w: &World, f: FighterId, k: i32) -> InputState {
    let mut i = NO_INPUT;
    let Some(e) = target(w, f) else { return i };
    let m = w.fighter(f);
    let o = &w.fighter(e).body;
    let (dx, dy) = (o.x - m.body.x, o.y - m.body.y);
    let press = k % 2 == 1;
    if dx.abs() < CHASE_REACH_X && dy.abs() < CHASE_REACH_Y {
        if dy > CHASE_REACH_Y / 2.0 {
            i.y = 1.0;
        } else if dy < -CHASE_REACH_Y / 2.0 {
            i.y = -1.0;
        } else {
            i.x = side(dx, m.body.facing_dir);
        }
        i.attack = press;
    } else {
        i.x = side(dx, m.body.facing_dir);
        i.jump = press && dy > CHASE_JUMP_Y && m.jump.jumps < 2;
    }
    i
}

/// Whether fighter `f` is in the air with its target out of the chase's reach (an air attack there hits nothing but a
/// projectile's path).
pub fn air_out_of_reach(w: &World, f: FighterId) -> bool {
    let m = w.fighter(f);
    if m.body.on_floor {
        return false;
    }
    target(w, f).is_none_or(|e| {
        let o = &w.fighter(e).body;
        (o.x - m.body.x).abs() >= CHASE_REACH_X || (o.y - m.body.y).abs() >= CHASE_REACH_Y
    })
}

/// `-1` or `1`: the sign of `d`, `fallback` at 0.
pub(super) fn side(d: Real, fallback: Real) -> Real {
    if d > 0.0 || (d == 0.0 && fallback >= 0.0) { 1.0 } else { -1.0 }
}

/// What a decision is made from: the side of the stage's centre and of the target (-1 or 1), the target, and the
/// situation.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct View {
    pub s: Real,
    pub t: Real,
    pub target: Option<FighterId>,
    pub situation: Situation,
}

/// The view of fighter `me` in `w`.
pub fn view(w: &World, me: FighterId) -> View {
    let m = w.fighter(me);
    let (mx, my) = (m.body.x, m.body.y);
    let g = &w.stage.ground;
    let centre = math::narrow((g.left + g.right) / 2.0);
    // toward the stage's centre, toward the target, away from it
    let s = side(centre - mx, m.body.facing_dir);
    let e = target(w, me);
    let (dx, dy) = e.map_or((0.0, 0.0), |e| {
        let o = &w.fighter(e).body;
        (o.x - mx, o.y - my)
    });
    let t = side(dx, m.body.facing_dir);
    let situation = if crate::fighter::in_hitstun(w, me) {
        Situation::Hitstun
    } else if off_stage(w, me) {
        Situation::OffStage
    } else if e.is_some_and(|e| crate::fighter::in_hitstun(w, e)) {
        Situation::Punish
    } else if m.body.on_floor && dx.abs() < NEAR_X && dy.abs() < NEAR_Y {
        Situation::Near
    } else if m.body.on_floor {
        Situation::Far
    } else {
        Situation::Air
    };
    View { s, t, target: e, situation }
}

/// The decision's shortlist into `out` (cleared first): "what the tree does" first, then the situation's scripts
/// with the fighter's add-ons in place, at most `max` in all.
pub fn shortlist(w: &World, me: FighterId, max: usize, out: &mut Vec<Plan>) {
    out.clear();
    out.push(Plan::Tree { len: TREE_LEN });
    let View { s, t, situation, .. } = view(w, me);
    let m = w.fighter(me);
    let list: &[Plan] = match situation {
        Situation::Hitstun => &[walk(s), dodge(s, 1.0)],
        Situation::OffStage => &[jump(s, FULL_JUMP_HOLD), special_drift(0.0, 1.0, s), dodge(s, 1.0), walk(s), idle()],
        Situation::Punish => &[
            attack(t, 0.0, Btn::Attack),
            attack(0.0, 1.0, Btn::Attack),
            attack(t, 0.0, Btn::Special),
            jump(t, 1),
            walk(t),
        ],
        Situation::Near => &[
            attack(t, 0.0, Btn::Attack),
            attack(0.0, 1.0, Btn::Attack),
            attack(0.0, -1.0, Btn::Attack),
            attack(t, 0.0, Btn::Special),
            dodge(-t, 0.0),
        ],
        Situation::Far => &[
            walk(t),
            jump(t, FULL_JUMP_HOLD),
            attack(0.0, 0.0, Btn::Special),
            attack(t, 0.0, Btn::Special),
            jump(t, 1),
        ],
        Situation::Air => &[
            attack(t, 0.0, Btn::Attack),
            attack(0.0, -1.0, Btn::Attack),
            attack(0.0, 1.0, Btn::Attack),
            walk(t),
            fast_fall(),
        ],
    };
    out.extend(list.iter().take(max.saturating_sub(1)));
    super::addons::apply(w.names.str(m.ch), situation, t, out);
}
