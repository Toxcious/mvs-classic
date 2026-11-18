//! The demo controllers of the Meeseeks tutorials:
//! TutorialController_AerialCombat_Basics and _HighUp (TutorialActor_AerialCombat steps 0 and 1) and
//! TutorialController_EvadeToAttack (TutorialActor_DashAttack step 0). Their looping timers' handles are never stored
//! (the handle variable is set to an empty struct after the K2_SetTimerDelegate), so ReceiveUnPossess's
//! K2_ClearAndInvalidateTimerHandle clears nothing and the timer keeps firing on a pawn that is gone (calls on it do
//! nothing).

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::fighter::FighterId;
use crate::input::{Btn, Dir};
use crate::world::World;
use crate::world::flow::DelegateCall;

use super::super::super::TutorialObj;
use super::super::super::classes::{self, Call, Fired, RunList, demo_mut};
use super::super::super::demo::{DemoPad, DemoState, Place, active, callable, get_pawn, place_of};
use super::super::super::latent::{self, MoveTarget};
use super::super::super::pawn::set_rotation;
use super::actor::{self, MeeseeksEnd};

/// TutorialController_AerialCombat_Basics. ReceivePossess (@1090): Possessed Pawn and Initial Location stored, a looping
/// 7.0 s timer to AerialCombat (@1284), AerialCombat now. AerialCombat (@1405), each step on Possessed Pawn, each Delay
/// its own node: JumpPressed; 0.5 (@1441) BPI_EvadeButtonPressed (@998); 0.4 (@1035) JumpPressed (@15); 0.5 (@51)
/// BPI_SpecialAttackPressed(0 Up) (@185); 0.5 (@224) BPI_EvadeButtonPressed (@358); 0.4 (@395) JumpPressed (@450); 0.5
/// (@486) BPI_SpecialAttackPressed(0) (@620); 1.0 (@659) MoveComponentTo(root, Initial Location, 0.1 s, no easing) with
/// no link (@826). No ActiveInTutorialIntro gate.
#[derive(State, Debug, Default)]
pub struct AerialCombatBasics {
    /// Possessed Pawn.
    pub p: Option<FighterId>,
    /// Initial Location.
    pub at: Option<Place>,
}

/// TutorialController_AerialCombat_HighUp. ReceivePossess (@457): Possessed Pawn stored, a looping 5.0 s timer to
/// AerialCombat (@574), AerialCombat now. AerialCombat (@695): JumpPressed; Delay 0.5 (@731) BPI_SpecialAttackPressed(0)
/// (@322); Delay 0.5 (@361) JumpPressed (@416); Delay 0.37 (@15) BPI_SpecialAttackPressed(0) (@149); Delay 0.5 (@188)
/// with no link.
#[derive(State, Debug, Default)]
pub struct AerialCombatHighUp {
    pub p: Option<FighterId>,
}

/// TutorialController_EvadeToAttack.
/// - ReceiveBeginPlay (@2157): TutorialDemoComplete += EvadeToAttackCleanup (@2013): K2_DestroyActor on every
///   Tutorial_Meeseeks (@15).
/// - ReceivePossess: Delay 0.5 (@2577, to @410): InitialLocation = the possessed pawn's, a looping 4.0 s timer to Attack
///   Loop (@510), then (@563) a Tutorial_Meeseeks at (0, 0, 0) with TeamIndex 1 (no minigame) and Delay 1.0 (@903, to
///   @958): K2_GetPawn as BaseFighter, ActiveInTutorialIntro: MoveX(1), EvadeButtonPressed(5); Delay 0.18 (@1164):
///   NormalAttackPressed(2 Fwd, false), MoveX(0); Delay 0.033 (@1299): NormalAttackReleased.
/// - Attack Loop (@2354): K2_GetPawn as BaseFighter: SetRotation(true), JumpPressed; Delay 0.1 (@2522, to @1433):
///   JumpReleasedLocal, MoveComponentTo(K2_GetPawn's root, InitialLocation, 0.6 s, ease out, ease in) (@1601), whose
///   Completed (@1391) is SetRotation(false) and back to @563 (a new Meeseeks; the Delay 1.0 node, ignored while it
///   runs).
/// - ReceiveUnPossess (@2632): the last spawned Meeseeks destroyed when valid.
#[derive(State, Debug, Default)]
pub struct EvadeToAttack {
    /// InitialLocation.
    pub at: Option<Place>,
    /// FinishSpawningActor's last return.
    pub last: Option<ActorRef>,
    /// Attack Loop's BaseFighter cast.
    pub cast: Option<FighterId>,
    /// @958's BaseFighter cast.
    pub hit: Option<FighterId>,
    /// ReceivePossess's pawn, which its Delay's closure holds (not in the record).
    pub possessed: Option<FighterId>,
}

/// `meeseeks::demo_constructed`: the constructor of EvadeToAttack (`host.demoComplete.add(this, ...)`): the
/// controller is spawned at the step's pawn spawn and possesses it at once, so a DemoComplete before any possession
/// finds nothing to clean up either way.
pub fn constructed(w: &mut World, id: u32) {
    if matches!(classes::demo_of(w, id).state, DemoState::EvadeToAttack(_)) {
        classes::bind_run(w, RunList::DemoComplete, TutorialObj::Demo(id), "demoComplete");
    }
}

/// A call on one of this family's demo controllers.
pub fn call(w: &mut World, id: u32, c: Call<'_>) {
    match &classes::demo_of(w, id).state {
        DemoState::AerialCombatBasics(_) => basics(w, id, c),
        DemoState::AerialCombatHighUp(_) => high_up(w, id, c),
        DemoState::EvadeToAttack(_) => evade_to_attack(w, id, c),
        _ => unreachable!("a demo of another family"),
    }
}

fn pad(w: &mut World, id: u32) -> &mut DemoPad {
    &mut demo_mut(w, id).pad
}

fn delay(w: &mut World, id: u32, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Demo(id), node, s);
}

fn unhandled(w: &World, id: u32, c: Call<'_>) {
    panic!("tutorial: {} has no handler for {c:?}", classes::demo_of(w, id).cls);
}

// ---- AerialCombat_Basics

fn basics_p(w: &mut World, id: u32) -> Option<FighterId> {
    match &demo_mut(w, id).state {
        DemoState::AerialCombatBasics(s) => s.p,
        _ => unreachable!(),
    }
}

fn basics(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let at = place_of(w, f);
            if let DemoState::AerialCombatBasics(s) = &mut demo_mut(w, id).state {
                s.p = Some(f);
                s.at = Some(at);
            }
            latent::set_timer(w, TutorialObj::Demo(id), "AerialCombat", 7.0, true);
            basics_chain(w, id);
        }
        Call::Fired(Fired::Timer("AerialCombat")) => basics_chain(w, id),
        Call::Fired(Fired::Delay("1441")) => {
            basics_press(w, id, Btn::Dodge);
            delay(w, id, "1035", 0.4);
        }
        Call::Fired(Fired::Delay("1035")) => {
            basics_press(w, id, Btn::Jump);
            delay(w, id, "51", 0.5);
        }
        Call::Fired(Fired::Delay("51")) => {
            basics_press(w, id, Btn::Special);
            delay(w, id, "224", 0.5);
        }
        Call::Fired(Fired::Delay("224")) => {
            basics_press(w, id, Btn::Dodge);
            delay(w, id, "395", 0.4);
        }
        Call::Fired(Fired::Delay("395")) => {
            basics_press(w, id, Btn::Jump);
            delay(w, id, "486", 0.5);
        }
        Call::Fired(Fired::Delay("486")) => {
            basics_press(w, id, Btn::Special);
            delay(w, id, "659", 1.0);
        }
        Call::Fired(Fired::Delay("659")) => {
            let (p, at) = match &demo_mut(w, id).state {
                DemoState::AerialCombatBasics(s) => (s.p, s.at),
                _ => unreachable!(),
            };
            if let (true, Some(p), Some(at)) = (callable(w, p), p, at) {
                latent::move_to(w, TutorialObj::Demo(id), "826", MoveTarget::Fighter(p), at.x, at.y, 0.1, false, false);
            }
        }
        // (the move has no link)
        Call::Fired(Fired::Move("826")) => {}
        // (ReceiveUnPossess's timer clear finds no handle)
        Call::Unpossessed { .. } => {}
        _ => unhandled(w, id, c),
    }
}

fn basics_press(w: &mut World, id: u32, b: Btn) {
    let p = basics_p(w, id);
    if callable(w, p) {
        pad(w, id).press(b, if b == Btn::Special { Some(Dir::Up) } else { None });
    }
}

fn basics_chain(w: &mut World, id: u32) {
    basics_press(w, id, Btn::Jump);
    delay(w, id, "1441", 0.5);
}

// ---- AerialCombat_HighUp

fn high_up(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            if let DemoState::AerialCombatHighUp(s) = &mut demo_mut(w, id).state {
                s.p = Some(f);
            }
            latent::set_timer(w, TutorialObj::Demo(id), "AerialCombat", 5.0, true);
            high_chain(w, id);
        }
        Call::Fired(Fired::Timer("AerialCombat")) => high_chain(w, id),
        Call::Fired(Fired::Delay("731")) => {
            high_press(w, id, Btn::Special);
            delay(w, id, "361", 0.5);
        }
        Call::Fired(Fired::Delay("361")) => {
            high_press(w, id, Btn::Jump);
            delay(w, id, "15", 0.37);
        }
        Call::Fired(Fired::Delay("15")) => {
            high_press(w, id, Btn::Special);
            delay(w, id, "188", 0.5);
        }
        Call::Fired(Fired::Delay("188")) => {}
        Call::Unpossessed { .. } => {}
        _ => unhandled(w, id, c),
    }
}

fn high_press(w: &mut World, id: u32, b: Btn) {
    let p = match &demo_mut(w, id).state {
        DemoState::AerialCombatHighUp(s) => s.p,
        _ => unreachable!(),
    };
    if callable(w, p) {
        pad(w, id).press(b, if b == Btn::Special { Some(Dir::Up) } else { None });
    }
}

fn high_chain(w: &mut World, id: u32) {
    high_press(w, id, Btn::Jump);
    delay(w, id, "731", 0.5);
}

// ---- EvadeToAttack

fn ev(w: &mut World, id: u32) -> &mut EvadeToAttack {
    match &mut demo_mut(w, id).state {
        DemoState::EvadeToAttack(s) => s,
        _ => unreachable!(),
    }
}

fn evade_to_attack(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            ev(w, id).possessed = Some(f);
            delay(w, id, "2577", 0.5);
        }
        Call::Fired(Fired::Delay("2577")) => {
            let f = ev(w, id).possessed;
            if callable(w, f)
                && let Some(f) = f
            {
                let at = place_of(w, f);
                ev(w, id).at = Some(at);
            }
            latent::set_timer(w, TutorialObj::Demo(id), "Attack Loop", 4.0, true);
            spawn_and_attack(w, id);
        }
        Call::Fired(Fired::Timer("Attack Loop")) => attack_loop(w, id),
        Call::Fired(Fired::Delay("903")) => {
            let p = get_pawn(w, id);
            ev(w, id).hit = p;
            if !active(w, p) {
                return;
            }
            pad(w, id).move_x(1.0);
            pad(w, id).press(Btn::Dodge, None);
            delay(w, id, "1164", 0.18);
        }
        Call::Fired(Fired::Delay("1164")) => {
            let hit = ev(w, id).hit;
            if !callable(w, hit) {
                return;
            }
            pad(w, id).press(Btn::Attack, Some(Dir::Fwd));
            pad(w, id).move_x(0.0);
            delay(w, id, "1299", 0.033);
        }
        Call::Fired(Fired::Delay("1299")) => {
            let hit = ev(w, id).hit;
            if callable(w, hit) {
                pad(w, id).release(Btn::Attack);
            }
        }
        Call::Fired(Fired::Delay("2522")) => {
            let cast = ev(w, id).cast;
            if callable(w, cast) {
                pad(w, id).release(Btn::Jump);
            }
            let q = get_pawn(w, id);
            // (K2_GetPawn None: MoveComponentTo on no component starts nothing, so no Completed)
            let at = ev(w, id).at;
            if let (true, Some(q), Some(at)) = (callable(w, q), q, at) {
                latent::move_to(w, TutorialObj::Demo(id), "1601", MoveTarget::Fighter(q), at.x, at.y, 0.6, true, true);
            }
        }
        Call::Fired(Fired::Move("1601")) => {
            let cast = ev(w, id).cast;
            if let (true, Some(cast)) = (callable(w, cast), cast) {
                set_rotation(w, cast, false);
            }
            spawn_and_attack(w, id);
        }
        Call::Unpossessed { .. } => {
            let last = ev(w, id).last;
            if actor::valid(w, last)
                && let Some(last) = last
            {
                actor::destroy(w, last, MeeseeksEnd::Plain);
            }
        }
        // EvadeToAttackCleanup (@2013): K2_DestroyActor on every Tutorial_Meeseeks (@15).
        Call::Delegate { handler: "demoComplete", call: DelegateCall::DemoComplete, .. } => {
            for a in actor::live(w) {
                actor::destroy(w, a, MeeseeksEnd::Plain);
            }
        }
        _ => unhandled(w, id, c),
    }
}

/// @563.
fn spawn_and_attack(w: &mut World, id: u32) {
    let last = actor::spawn(w, 0.0, 0.0);
    ev(w, id).last = last;
    delay(w, id, "903", 1.0);
}

/// Attack Loop (@2354).
fn attack_loop(w: &mut World, id: u32) {
    let p = get_pawn(w, id);
    let (true, Some(p)) = (callable(w, p), p) else { return };
    ev(w, id).cast = Some(p);
    set_rotation(w, p, true);
    pad(w, id).press(Btn::Jump, None);
    delay(w, id, "2522", 0.1);
}
