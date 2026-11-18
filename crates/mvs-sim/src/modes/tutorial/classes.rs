//! The tutorial's Blueprint classes the port has: the demo controllers (a step's
//! DemoAIController), the minigame actors (a task's CustomTutorialActors) and the TutorialActor child classes' own
//! graphs, by the build's class name without the `_C`. The TutorialActor (`run.rs`) makes them from here; a latent
//! entry's end, a delegate broadcast or a controller's call reaches its object's class through here.
//!
//! One match per family: the beginner tutorial's arms in `demo.rs`, each advanced family's in its own file
//! (`advanced/*::make_demo` / `make_minigame` / `demo` / `minigame` / `actor_begin_play` / `drawn_with`), so no two
//! tracks edit one list. A class is looked up in every family: one entered in two is an error.

use crate::actors::ActorRef;
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::world::World;
use crate::world::flow::{Bound, DelegateCall, DelegateList};

use super::advanced::{batman, elements, fighters, geometry, meeseeks};
use super::demo::{self, Demo, DemoPad, DemoState};
use super::minigame::{Minigame, MinigameState};
use super::{TutorialBind, TutorialObj};

/// What ended in the scheduler (`latent.rs`): a Delay node, a MoveComponentTo's Completed pin, a timer's pulse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fired {
    Delay(&'static str),
    Move(&'static str),
    Timer(&'static str),
}

/// What reaches a tutorial object's class from outside its own graph's flow.
#[derive(Clone, Copy, Debug)]
pub enum Call<'a> {
    /// A demo controller's ReceivePossess.
    Possessed(FighterId),
    /// A demo controller's ReceiveUnPossess (`destroyed`: the controller goes with its pawn's K2_DestroyActor).
    Unpossessed { pawn: FighterId, destroyed: bool },
    /// A minigame's or stage actor's ReceiveBeginPlay. A minigame class runs `minigame::bind_steps` first (the
    /// base's binds of the step delegates).
    BeginPlay,
    /// K2_DestroyActor's ReceiveEndPlay (`onDestroyed`) of a minigame or stage actor.
    Destroyed,
    /// A latent entry of the object ended.
    Fired(Fired),
    /// A delegate the object bound was broadcast: its handler, the broadcaster (a pawn or an actor; `None` for the
    /// TutorialActor's and the game mode's own delegates) and the arguments.
    Delegate { handler: &'static str, src: Option<ActorRef>, call: &'a DelegateCall<'a> },
}

/// The family a class's state belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Beginner,
    Fighters,
    Geometry,
    Meeseeks,
    Elements,
    Batman,
}

fn demo_family(s: &DemoState) -> Family {
    use DemoState as D;
    match s {
        D::Aerial(_)
        | D::Heavy(_)
        | D::ShaggyKick(_)
        | D::Special(_)
        | D::Evade(_)
        | D::ChargeUp(_)
        | D::Combat(_)
        | D::AttackDecay(_) => Family::Beginner,
        D::DodgeBasics(_)
        | D::DodgeMeterDeplete(_)
        | D::DodgeMeterReplenish(_)
        | D::CooldownsBasics(_)
        | D::ArmorBasics(_)
        | D::ArmorBreak(_)
        | D::DodgeJump(_)
        | D::AttackDecayVariety(_) => Family::Fighters,
        D::WallSlide(_) | D::FastFall(_) | D::FastFallPlatform(_) | D::KbiSide(_) | D::KbiUp(_) => Family::Geometry,
        D::AerialCombatBasics(_) | D::AerialCombatHighUp(_) | D::EvadeToAttack(_) => Family::Meeseeks,
        D::KillBotOffPlatform(_) | D::ExtinguishFire(_) | D::Ignite(_) => Family::Elements,
        D::BatmanBatarang(_) | D::BatmanRetrieve(_) | D::BatmanDestroy(_) => Family::Batman,
    }
}

fn minigame_family(s: &MinigameState) -> Family {
    use MinigameState as M;
    match s {
        M::DodgeBasics(_) | M::DodgeRefill(_) | M::ArmorBreak(_) => Family::Fighters,
        M::SpawnWall(_) | M::SpawnPlatform(_) | M::KbiSide(_) | M::KbiUp(_) => Family::Geometry,
        M::AerialCombat(_) | M::PositioningTools(_) => Family::Meeseeks,
        M::DownSpike(_) | M::ExtinguishFire(_) | M::Ignite(_) => Family::Elements,
        M::BatmanBatarang(_) | M::BatmanRetrieve(_) | M::BatmanDestroy(_) => Family::Batman,
    }
}

/// The one entry of `cls` among the families' answers; a class entered twice is an error.
fn one<T>(cls: &str, found: [Option<T>; 6]) -> Option<T> {
    let mut it = found.into_iter().flatten();
    let first = it.next();
    if it.next().is_some() {
        panic!("tutorial: the class {cls} is entered twice");
    }
    first
}

/// `makeDemo`: the demo controller of a TutorialStep's DemoAIController class, `None` for none (SpawnDefaultController
/// with no class spawns nothing) or one the port lacks (warned): a new [`Demo`] in `TutorialRun::demos`, by its id.
pub fn make_demo(w: &mut World, cls: Option<&str>) -> Option<u32> {
    let cls = cls?;
    let found = one(
        cls,
        [
            demo::make_demo(cls),
            fighters::make_demo(cls),
            geometry::make_demo(cls),
            meeseeks::make_demo(cls),
            elements::make_demo(cls),
            batman::make_demo(cls),
        ],
    );
    let Some((cls, state)) = found else {
        warn_text(w, &format!("tutorial: the demo controller {cls} is not ported (no controller)"));
        return None;
    };
    let run = w.tutorial.as_mut().expect("a tutorial World");
    let id = run.next_id();
    run.demos.push(Demo { id, cls, pawn: None, pad: DemoPad::default(), state });
    meeseeks::demo_constructed(w, id);
    Some(id)
}

/// `makeMinigame`: a minigame actor of a CustomTutorialActors class, `None` where the port has none (warned): a new
/// [`Minigame`] at the end of the actor's list, by its id. The caller runs its BeginPlay and the setup.
pub fn make_minigame(w: &mut World, cls: &str) -> Option<u32> {
    let found = one(
        cls,
        [
            None,
            fighters::make_minigame(cls),
            geometry::make_minigame(cls),
            meeseeks::make_minigame(cls),
            elements::make_minigame(cls),
            batman::make_minigame(cls),
        ],
    );
    let Some((cls, state)) = found else {
        warn_text(w, &format!("tutorial: the minigame actor {cls} is not ported"));
        return None;
    };
    let run = w.tutorial.as_mut().expect("a tutorial World");
    let id = run.next_id();
    let a = run.actor.as_mut().expect("the TutorialActor");
    a.minigames.push(Minigame { id, cls, destroyed: false, step_index: -1, task_index: -1, state });
    Some(id)
}

/// `actorHooks(cls)?.beginPlay`: a TutorialActor child class's own ReceiveBeginPlay, after the parent's (none for
/// TutorialActor itself, the Intro's own graph is `run.rs`'s).
pub fn actor_begin_play(w: &mut World, cls: &str) {
    fighters::actor_begin_play(w, cls);
    geometry::actor_begin_play(w, cls);
    meeseeks::actor_begin_play(w, cls);
    elements::actor_begin_play(w, cls);
    batman::actor_begin_play(w, cls);
}

/// `classDrawnWith`: the characters a demo controller or minigame class's actors are drawn with (the UI loads
/// them with the tutorial's cast).
pub fn drawn_with(cls: &str) -> Vec<&'static str> {
    [
        fighters::drawn_with(cls),
        geometry::drawn_with(cls),
        meeseeks::drawn_with(cls),
        elements::drawn_with(cls),
        batman::drawn_with(cls),
    ]
    .concat()
}

// ---- the dispatch

/// A latent entry of `owner` ended: its class runs the continuation.
pub fn fired(w: &mut World, owner: TutorialObj, what: Fired) {
    call(w, owner, Call::Fired(what));
}

/// A call on a tutorial object: routed to its class.
pub fn call(w: &mut World, owner: TutorialObj, c: Call<'_>) {
    match owner {
        TutorialObj::Mode => super::run::mode_call(w, c),
        TutorialObj::Actor => super::run::actor_call(w, c),
        TutorialObj::Demo(id) => {
            let fam = demo_family(&demo_of(w, id).state);
            match fam {
                Family::Beginner => demo::call(w, id, c),
                Family::Fighters => fighters::demo(w, id, c),
                Family::Geometry => geometry::demo(w, id, c),
                Family::Meeseeks => meeseeks::demo(w, id, c),
                Family::Elements => elements::demo(w, id, c),
                Family::Batman => batman::demo(w, id, c),
            }
        }
        TutorialObj::Minigame(id) => {
            // (a destroyed minigame's entries are gone; a stale bind is dropped by the broadcast first)
            let Some(m) = minigame_of(w, id) else { return };
            match minigame_family(&m.state) {
                Family::Beginner => unreachable!("no beginner minigame"),
                Family::Fighters => fighters::minigame(w, id, c),
                Family::Geometry => geometry::minigame(w, id, c),
                Family::Meeseeks => meeseeks::minigame(w, id, c),
                Family::Elements => elements::minigame(w, id, c),
                Family::Batman => batman::minigame(w, id, c),
            }
        }
        TutorialObj::Stage(i) => meeseeks::stage(w, i, c),
    }
}

/// The demo controller of id `id`.
pub fn demo_of(w: &World, id: u32) -> &Demo {
    let run = w.tutorial.as_ref().expect("a tutorial World");
    run.demos.iter().find(|d| d.id == id).unwrap_or_else(|| panic!("tutorial: no demo controller {id}"))
}

/// The demo controller of id `id`, to write.
pub fn demo_mut(w: &mut World, id: u32) -> &mut Demo {
    let run = w.tutorial.as_mut().expect("a tutorial World");
    run.demos.iter_mut().find(|d| d.id == id).unwrap_or_else(|| panic!("tutorial: no demo controller {id}"))
}

/// The live minigame actor of id `id` (`None` once destroyed).
pub fn minigame_of(w: &World, id: u32) -> Option<&Minigame> {
    w.tutorial.as_ref()?.actor.as_ref()?.minigames.iter().find(|m| m.id == id)
}

/// The live minigame actor of id `id`, to write.
pub fn minigame_mut(w: &mut World, id: u32) -> Option<&mut Minigame> {
    w.tutorial.as_mut()?.actor.as_mut()?.minigames.iter_mut().find(|m| m.id == id)
}

/// `DelegateOwner.destroyed`: a binder destroyed (K2_DestroyActor) is stale: its entries are skipped and dropped
/// at the next broadcast. The game mode, the actor and the demo controllers are never destroyed.
pub fn stale(w: &World, owner: TutorialObj) -> bool {
    match owner {
        TutorialObj::Mode | TutorialObj::Actor | TutorialObj::Demo(_) => false,
        TutorialObj::Minigame(id) => minigame_of(w, id).is_none_or(|m| m.destroyed),
        TutorialObj::Stage(i) => meeseeks::stage_destroyed(w, i),
    }
}

/// A broadcast's call of one tutorial bind (`world::flow::broadcast_bound`).
pub fn delegate(w: &mut World, b: TutorialBind, src: Option<ActorRef>, c: &DelegateCall<'_>) {
    call(w, b.owner, Call::Delegate { handler: b.handler, src, call: c });
}

// ---- the binds (`Multicast.add` / `remove` on a pawn's or an actor's delegate)

/// `add(owner, fn)`: AddUnique on a pawn's or an actor's delegate (an actor's delegates made at the first bind).
pub fn bind(w: &mut World, at: DelegateList, owner: TutorialObj, handler: &'static str) {
    let b = Bound::Tutorial(TutorialBind { owner, handler });
    let l = crate::world::flow::delegate_list_or_new(w, at);
    if !l.contains(&b) {
        l.push(b);
    }
}

/// `remove(owner, fn)`: RemoveDynamic, the entry swapped out; `handler` `None`: every entry of the owner (Unbind All
/// Events from the owner's side).
pub fn unbind(w: &mut World, at: DelegateList, owner: TutorialObj, handler: Option<&'static str>) {
    let Some(l) = crate::world::flow::delegate_list_mut(w, at) else { return };
    for i in (0..l.len()).rev() {
        let Bound::Tutorial(b) = l[i] else { continue };
        if b.owner != owner || handler.is_some_and(|h| h != b.handler) {
            continue;
        }
        l.swap_remove(i);
        if handler.is_some() {
            return;
        }
    }
}

/// The TutorialActor's and the game mode's own delegates. The base handlers' names: "playerRespawned",
/// "onStepComplete", "onStepCleanup", "demoComplete" (a class may bind more of its own, AddUnique by owner and
/// handler).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunList {
    /// Parent_Game_Mode PlayerRespawned(PlayerIndex).
    PlayerRespawned,
    /// TutorialStepComplete.
    StepComplete,
    /// TutorialStepCleanup.
    StepCleanup,
    /// TutorialDemoComplete.
    DemoComplete,
}

impl RunList {
    /// The list's base handler name (what a class binds when it binds the list once).
    pub fn base_handler(self) -> &'static str {
        match self {
            RunList::PlayerRespawned => "playerRespawned",
            RunList::StepComplete => "onStepComplete",
            RunList::StepCleanup => "onStepCleanup",
            RunList::DemoComplete => "demoComplete",
        }
    }
}

fn run_list(w: &mut World, l: RunList) -> Option<&mut Vec<TutorialBind>> {
    let run = w.tutorial.as_mut()?;
    match l {
        RunList::PlayerRespawned => Some(&mut run.player_respawned),
        RunList::StepComplete => run.actor.as_mut().map(|a| &mut a.step_complete),
        RunList::StepCleanup => run.actor.as_mut().map(|a| &mut a.step_cleanup),
        RunList::DemoComplete => run.actor.as_mut().map(|a| &mut a.demo_complete),
    }
}

/// `add(owner, fn)` on one of the run's own delegates (AddUnique).
pub fn bind_run(w: &mut World, l: RunList, owner: TutorialObj, handler: &'static str) {
    let b = TutorialBind { owner, handler };
    if let Some(v) = run_list(w, l)
        && !v.contains(&b)
    {
        v.push(b);
    }
}

/// `remove(owner, fn)` on one of the run's own delegates: the entry swapped out; `handler` `None`: every entry of
/// the owner.
pub fn unbind_run(w: &mut World, l: RunList, owner: TutorialObj, handler: Option<&'static str>) {
    let Some(v) = run_list(w, l) else { return };
    for i in (0..v.len()).rev() {
        let b = v[i];
        if b.owner != owner || handler.is_some_and(|h| h != b.handler) {
            continue;
        }
        v.swap_remove(i);
        if handler.is_some() {
            return;
        }
    }
}

/// `broadcast` of one of the run's own delegates: the copy of the list in bind order, a stale entry swapped out of
/// the live list and not called.
pub fn broadcast_run(w: &mut World, l: RunList, c: &DelegateCall<'_>) {
    let Some(v) = run_list(w, l) else { return };
    if v.is_empty() {
        return;
    }
    let copy = v.clone();
    for b in copy {
        if stale(w, b.owner) {
            if let Some(v) = run_list(w, l)
                && let Some(i) = v.iter().position(|&x| x == b)
            {
                v.swap_remove(i);
            }
            continue;
        }
        delegate(w, b, None, c);
    }
}
