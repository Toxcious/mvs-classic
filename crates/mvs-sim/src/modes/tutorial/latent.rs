//! The latent actions and timers of the tutorial's own objects (`Latent`): Delay,
//! MoveComponentTo and K2_SetTimer by object, in world ticks, for owners that never tick (each has an entry at the
//! leftover latent pass and none is processed earlier in the frame). An entry names its object and node (a Delay's
//! or a move's call site, its `node`) or its timer's function name; at its end the object's class runs the
//! continuation of that node (`classes.rs`), with what a closure would capture kept in the object's own state.
//! The same rules as the stage's scheduler (`world/stage.rs` delay_at, due_timers).

use crate::Real;
use crate::State;
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::math::{dbl, to_int32};
use crate::world::World;
use crate::world::stage::ComponentMove;

use super::TutorialObj;
use super::classes::{self, Fired};

/// `Latent`: the `delays`, `moves`, `timers` and `passed`.
#[derive(State, Debug, Default)]
pub struct Latent {
    pub delays: Vec<DelayEntry>,
    pub moves: Vec<MoveEntry>,
    pub timers: Vec<TimerEntry>,
    /// The tick whose leftover pass has run (a Delay started after it counts from the next frame); -1 none.
    pub passed: i32,
}

/// A Delay node's action: due at world tick `at`.
#[derive(State, Copy, PartialEq, Debug)]
pub struct DelayEntry {
    pub owner: TutorialObj,
    pub node: &'static str,
    pub at: i32,
}

/// What a MoveComponentTo moves (`MoveTarget`): a fighter's capsule (`fighterTarget`) or a run-time solid
///, by its spec (`RuntimeSolid::spec`). Its root place is in sim space (x = -UE Y,
/// y = UE Z), written with no sweep.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum MoveTarget {
    Fighter(FighterId),
    Solid(i32),
}

/// A MoveComponentTo's action; its Completed pin is the owner's continuation of `node`.
#[derive(State, Debug)]
pub struct MoveEntry {
    pub owner: TutorialObj,
    pub node: &'static str,
    pub t: MoveTarget,
    pub m: ComponentMove,
}

/// A K2_SetTimer handle of `owner` by function `name`.
#[derive(State, Copy, PartialEq, Debug)]
pub struct TimerEntry {
    pub owner: TutorialObj,
    pub name: &'static str,
    /// The world tick whose timer tick started it.
    pub from: i32,
    /// Its Rate, float32 seconds.
    pub rate: Real,
    /// The pulse due at `at` (1 for the first).
    pub n: i32,
    pub at: i32,
    pub looping: bool,
}

impl Latent {
    /// The scheduler of a new game mode: no pass has run.
    pub fn new() -> Self {
        Latent { passed: -1, ..Latent::default() }
    }
}

fn sched(w: &World) -> &Latent {
    &w.tutorial.as_ref().expect("a tutorial World").sched
}

fn sched_mut(w: &mut World) -> &mut Latent {
    &mut w.tutorial.as_mut().expect("a tutorial World").sched
}

// ---- the move targets (`fighterTarget`, `RuntimeSolid` as a MoveTarget)

/// The target's root place: a fighter's actor place, a run-time solid's box centre.
fn target_at(w: &World, t: MoveTarget) -> (Real, Real) {
    match t {
        MoveTarget::Fighter(f) => (w.fighter(f).body.x, crate::fighter::actor_y(w, f)),
        MoveTarget::Solid(spec) => crate::world::stage::runtime_solid_at(w, spec).unwrap_or((0.0, 0.0)),
    }
}

/// `gone`: destroyed (the action ends with no link, as for a pending-kill target).
fn target_gone(w: &World, t: MoveTarget) -> bool {
    match t {
        MoveTarget::Fighter(f) => w.fighter(f).life.dead == Real::INFINITY,
        MoveTarget::Solid(spec) => crate::fighter::body::runtime_solid(w, spec).is_none(),
    }
}

/// `place`: a fighter's recalled root write with its overlaps updated (World.rootMoved); a solid's root write, its box
/// following.
fn target_place(w: &mut World, t: MoveTarget, x: Real, y: Real) {
    match t {
        MoveTarget::Fighter(f) => {
            crate::fighter::moves::recall_move(w, f, x, y);
            w.fighter_mut(f).capsule.cap_resized = true;
            crate::combat::phases::root_moved(w, f);
        }
        MoveTarget::Solid(spec) => crate::world::stage::runtime_solid_place(w, spec, x, y),
    }
}

// ---- the calls

/// Whether the frame's leftover pass is still to come: a Delay started in the tick groups counts this frame, one
/// started in the pass, in the timer pass or between two steps from the next one.
fn now(w: &World) -> bool {
    w.scratch.step_at != 0 && sched(w).passed != w.flow.tick
}

/// `delay`: Delay(s) at `node` of `owner` (execDelay: a call while that node's action exists starts nothing).
pub fn delay(w: &mut World, owner: TutorialObj, node: &'static str, s: Real) {
    if sched(w).delays.iter().any(|d| d.owner == owner && d.node == node) {
        return;
    }
    let at = w.flow.tick + to_int32(crate::fighter::common::latent_frames(s)) - i32::from(now(w));
    sched_mut(w).delays.push(DelayEntry { owner, node, at });
}

/// `moveTo` / `moveTargetTo`: MoveComponentTo(the target, (tx, ty), s, easeOut, easeIn) at `node` of `owner`, Move:
/// its first update is the next leftover pass; its Completed pin is the owner's `Fired::Move(node)`. A call while
/// that node's action runs moves it to the new target (`ComponentMove::call`).
#[allow(clippy::too_many_arguments, reason = "the node's pins")]
pub fn move_to(
    w: &mut World,
    owner: TutorialObj,
    node: &'static str,
    t: MoveTarget,
    tx: Real,
    ty: Real,
    s: Real,
    ease_out: bool,
    ease_in: bool,
) {
    let (x, y) = target_at(w, t);
    let l = sched_mut(w);
    let cur = l.moves.iter().position(|e| e.owner == owner && e.node == node);
    let m = ComponentMove::call(cur.map(|i| l.moves[i].m), x, y, tx, ty, ease_out, ease_in, s, 0)
        .expect("a Move call makes an action");
    match cur {
        Some(i) => {
            l.moves[i].t = t;
            l.moves[i].m = m;
        }
        None => l.moves.push(MoveEntry { owner, node, t, m }),
    }
}

/// `delayPending`: whether `owner` has a Delay pending at `node`.
pub fn delay_pending(w: &World, owner: TutorialObj, node: &'static str) -> bool {
    sched(w).delays.iter().any(|d| d.owner == owner && d.node == node)
}

/// `dropOwner`: K2_DestroyActor of `owner`: its latent actions go (FLatentActionManager::RemoveActionsForObject) and
/// its timers are cleared (UWorld's timer manager: ClearAllTimersForObject at the destroy).
pub fn drop_owner(w: &mut World, owner: TutorialObj) {
    let l = sched_mut(w);
    l.delays.retain(|d| d.owner != owner);
    l.moves.retain(|e| e.owner != owner);
    l.timers.retain(|e| e.owner != owner);
}

/// `setTimer`: K2_SetTimer(name, s, loop) of `owner`: a running timer of the name is cleared first; a rate <= 0 only
/// clears. Its pulses are the owner's `Fired::Timer(name)`.
pub fn set_timer(w: &mut World, owner: TutorialObj, name: &'static str, s: Real, looping: bool) {
    clear_timer(w, owner, name);
    let rate = s;
    if rate.is_nan() || rate <= 0.0 {
        return;
    }
    let from = w.flow.tick + i32::from(w.scratch.step_at == 0);
    let at = from + to_int32(crate::combat::hit::timer_ticks(Some(rate)));
    sched_mut(w).timers.push(TimerEntry { owner, name, from, rate, n: 1, at, looping });
}

/// `clearTimer`: K2_ClearTimer / ClearTimerByFunctionName.
pub fn clear_timer(w: &mut World, owner: TutorialObj, name: &'static str) {
    let l = sched_mut(w);
    if let Some(i) = l.timers.iter().position(|e| e.owner == owner && e.name == name) {
        l.timers.remove(i);
    }
}

/// The world tick of a timer's pulse n (`pulseAt`, as the stage's).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated test: a NaN rate loops as it does there")]
fn pulse_at(from: i32, n: i32, rate: Real) -> i32 {
    // (doubles: the expire time n x rate and the tick count, as the manager's clock)
    let x: crate::Clock = f64::from(n) * f64::from(rate);
    let d = f64::from(DELTA_SECONDS);
    let mut k = dbl::max(1.0, (x / d).floor());
    while !(k * d > x) {
        k += 1.0;
    }
    while k > 1.0 && (k - 1.0) * d > x {
        k -= 1.0;
    }
    from + dbl::to_int32(k)
}

/// `run`: the leftover latent pass (the moves' updates and links, then the due Delays in their start order), then the
/// timer pass (by expire time; a one-shot is gone before its event runs, a loop runs each due pulse).
// Unverified: the mode's timers are taken in its latent call, ahead of the stage's leftover pass and timers.
pub fn run(w: &mut World) {
    let t = w.flow.tick;
    sched_mut(w).passed = t;
    if !sched(w).moves.is_empty() {
        // (the walk sees a move a link of this walk started, as an array walk does)
        let mut done: Vec<(TutorialObj, &'static str, MoveTarget)> = Vec::new();
        let mut k = 0;
        while k < sched(w).moves.len() {
            let (owner, node, tg) = {
                let e = &sched(w).moves[k];
                (e.owner, e.node, e.t)
            };
            k += 1;
            if target_gone(w, tg) {
                done.push((owner, node, tg));
                continue;
            }
            let (end, x, y) = {
                let m = &mut sched_mut(w).moves[k - 1].m;
                let end = m.update(DELTA_SECONDS);
                (end, m.x, m.y)
            };
            target_place(w, tg, x, y);
            if end {
                done.push((owner, node, tg));
            }
        }
        if !done.is_empty() {
            sched_mut(w).moves.retain(|e| !done.iter().any(|&(o, n, _)| e.owner == o && e.node == n));
            for (owner, node, tg) in done {
                if !target_gone(w, tg) {
                    classes::fired(w, owner, Fired::Move(node));
                }
            }
        }
    }
    if sched(w).delays.iter().any(|d| d.at <= t) {
        let l = sched_mut(w);
        let due: Vec<DelayEntry> = l.delays.iter().copied().filter(|d| d.at <= t).collect();
        l.delays.retain(|d| d.at > t);
        for d in due {
            classes::fired(w, d.owner, Fired::Delay(d.node));
        }
    }
    if !sched(w).timers.iter().any(|e| e.at <= t) {
        return;
    }
    let exp = |e: &TimerEntry| crate::consts::clock(crate::real(e.from)) + f64::from(e.n) * f64::from(e.rate);
    let mut due: Vec<TimerEntry> = sched(w).timers.iter().copied().filter(|e| e.at <= t).collect();
    due.sort_by(|a, b| exp(a).total_cmp(&exp(b)));
    let has = |w: &World, e: &TimerEntry| sched(w).timers.iter().position(|x| x == e);
    for e in due {
        let Some(k) = has(w, &e) else { continue };
        if !e.looping {
            sched_mut(w).timers.remove(k);
            classes::fired(w, e.owner, Fired::Timer(e.name));
            continue;
        }
        let mut n = e.n;
        while pulse_at(e.from, n, e.rate) <= t && has(w, &e).is_some() {
            classes::fired(w, e.owner, Fired::Timer(e.name));
            n += 1;
        }
        // (the object is written even when an event cleared it: a cleared one is out of the list)
        if let Some(k) = has(w, &e) {
            let x = &mut sched_mut(w).timers[k];
            x.n = n;
            x.at = pulse_at(e.from, n, e.rate);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pulse_at;

    #[test]
    fn pulses_count_strictly_past_the_rate() {
        // 1.0 s at DeltaSeconds 0.0166668: 59 x dt < 1.0 < 60 x dt
        assert_eq!(pulse_at(10, 1, 1.0), 10 + 60);
        assert_eq!(pulse_at(0, 2, 1.0), 120);
    }
}
