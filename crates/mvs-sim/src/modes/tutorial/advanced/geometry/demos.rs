//! The geometry family's demo controllers: TutorialController_WallSlide,
//! _FastFall_Intro, _FastFall_Jump, _FastFall_Platform, _KBISide and _KBIUp.
//! Simplification: the demo pawn's BPI_JumpPressedLocal goes through the demo pad as JumpPressed (`DemoPad` has no
//! local press); the two differ by CancelInvuln alone, and a demo pawn has no invulnerability to cancel.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::ai::pad::Btn as BotBtn;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_is_a, lookup_owner, warn_text};
use crate::input::{Btn, Dir};
use crate::name;
use crate::names::Name;
use crate::world::World;
use crate::world::flow::{DelegateList, PawnList};

use super::super::super::classes::{self, Call, Fired, demo_mut};
use super::super::super::demo::{DemoPad, DemoState, Place, callable, place_of};
use super::super::super::latent;
use super::super::super::pawn::{BagSpawn, bot_down, bot_move_x, bot_press, sound_at, spawn_punching_bag, tagged};
use super::super::super::{TutorialObj, run};
use super::solids::{self, Held, SolidClass, spec_of};
use super::{CAST_SOUND, emitter_at};

const WONDER_WOMAN: Name = name!("wonderwoman");
const REINDOG: Name = name!("creature");

/// TutorialController_WallSlide: ReceivePossess Delay 0.1 (@70): its SpawnPunchingBag(PunchingBag, 2,
/// Pawn_WonderWomanV3) (FacingLeft, PlayerDataReadyServer(2), the team of player data 2, CurrentDamage 0, an
/// Idle_AIController), a TutorialWall at (0, -1000, 1480), the bag's MoveX(1.0); Delay 0.35, its JumpPressed;
/// Delay 0.35, a looping 3.0 s timer to WallJump (the bag's JumpPressed). ReceiveUnPossess (@678): the bag and the
/// wall destroyed; the timer is not cleared.
#[derive(State, Debug, Default)]
pub struct WallSlide {
    pub bag: Option<FighterId>,
    pub wall: Option<Held>,
}

/// TutorialController_FastFall_Intro: ReceivePossess (@453) DemoFighter, Delay 0.5 (@70): a looping 3.0 s timer to
/// Fastfall Loop and one call now. Fastfall Loop (@282): BPI_JumpPressedLocal; Delay 0.4 (@161): BPI_DownPressed, then
/// a Delay 0.3 with nothing after it. ReceiveUnPossess (@602) clears the timer. DownReleased is never called.
/// TutorialController_FastFall_Jump: the same with a 4.5 s timer (@15); the loop (@395): BPI_JumpPressedLocal; Delay
/// 0.4 (@219): BPI_DownPressed; Delay 0.2 (@390 -> @106): BPI_JumpPressedLocal. ReceiveUnPossess (@715) clears it.
#[derive(State, Debug, Default)]
pub struct FastFall {
    pub fighter: Option<FighterId>,
    pub jump_again: bool,
}

/// TutorialController_FastFall_Platform: ReceivePossess Delay 0.5 (@15): a TutorialPlatform at the transform of the
/// first actor tagged Platform, PS_MeeseeksSpawn at it (no Team set), the cast sound at it; a Pawn_WonderWomanV3 at the
/// first PunchingBagCenter (FacingLeft, PlayerDataReadyServer(2), the team of player data 1, CurrentDamage 0, an
/// Idle_AIController); Delay 1.0 (@1937): a looping 3.0 s timer to Fastfall Loop and one call now. Fastfall Loop
/// (@2778): the bag's BPI_JumpPressedLocal; Delay 0.4 (@2065): its BPI_DownPressed; Delay 0.3 (@2236 -> @2028): its
/// DownReleased. ReceiveUnPossess (@2701): the platform and the bag destroyed, then every TetherProjectile_C; the
/// timer is not cleared.
#[derive(State, Debug, Default)]
pub struct FastFallPlatform {
    pub platform: Option<Held>,
    pub bag: Option<FighterId>,
}

/// The KBI demos' shared part: DemoComplete (class default true: the possession's own second Possess runs
/// ReceiveUnPossess once ahead of the demo, which flips it to false), DemoFighter and its place, PunchingBagsKilled,
/// the bag and its handlers.
/// Not ported: Comp_Pawn_DamageManager DI_ShowVisualOverride and DI_VisualLifetimeOverride on the bag (the port draws no
/// DI visual to force).
#[derive(State, Debug)]
pub struct KbiDemo {
    pub complete: bool,
    pub fighter: Option<FighterId>,
    pub at: Option<Place>,
    pub killed: i32,
    pub bag: Option<FighterId>,
}

impl Default for KbiDemo {
    fn default() -> Self {
        KbiDemo { complete: true, fighter: None, at: None, killed: 0, bag: None }
    }
}

/// TutorialController_KBISide (InitialPunchingBagDamage 100, DI Visual Lifetime Override 6): ReceivePossess (@3031)
/// DemoFighter, Delay 0.1 (@15): InitialDemoFighterLocation, SpawnDummy. SpawnDummy (@107): DemoComplete: nothing;
/// else a Pawn_Reindog dummy (damage 100), Delay 1.3 (@1713): AttackDummy (NormalAttackPressed(1 Down)).
/// OnDummyHit (@3822): the bag's BPI_MoveX(killed even ? 1 : -1). OnDummyDeath (@3269): PunchingBagsKilled + 1, Delay
/// (lifetime - 3.5 = 2.5 s) (@1728), then Delay 1.5 (@2068): SpawnDummy. ReceiveUnPossess (@3180): DemoComplete
/// flipped, the bag destroyed, the camera foci and the tethers destroyed.
/// Not ported: the AmbientSound camera focus with a Comp_Actor_KeepOnCamera at the dummy's death place (@3338), held
/// for the 2.5 s (the port's camera has no actor focus to add).
#[derive(State, Debug, Default)]
pub struct KbiSideDemo {
    pub k: KbiDemo,
}

/// TutorialController_KBIUp (InitialPunchingBagDamage 115): ReceivePossess (@192) DemoFighter, Delay 0.1 (@30):
/// InitialDemoFighterLocation, SpawnDummy, then SpawnWall (@2679: a TutorialWallShort at the transform of the first
/// KBICeilRight, K2_SetActorRotation((0, 0, 90)), the cast sound at it). SpawnDummy (@2158): DemoComplete: nothing;
/// else a Pawn_WonderWomanV3 dummy (damage 115), Delay 1.5 (@126): AttackDummy (NormalAttackPressed(0 Up)).
/// OnDummyHit (@2173): the bag's BPI_MoveX(killed even ? 0 : 1). OnPunchingBagDeath (@2555): PunchingBagsKilled + 1,
/// Delay 0.2 (@141): the bag destroyed, SpawnDummy. ReceiveUnPossess (@2037): DemoComplete flipped, the bag and the
/// wall destroyed.
#[derive(State, Debug, Default)]
pub struct KbiUpDemo {
    pub k: KbiDemo,
    pub wall: Option<Held>,
}

// ---- the classes' tables

/// The state of the class `$v` of demo `$id`.
macro_rules! st {
    ($w:expr, $id:expr, $v:ident) => {
        match &mut demo_mut($w, $id).state {
            DemoState::$v(s) => s,
            _ => unreachable!(),
        }
    };
}

fn pad(w: &mut World, id: u32) -> &mut DemoPad {
    &mut demo_mut(w, id).pad
}

fn delay(w: &mut World, id: u32, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Demo(id), node, s);
}

fn set_timer(w: &mut World, id: u32, name: &'static str, s: Real) {
    latent::set_timer(w, TutorialObj::Demo(id), name, s, true);
}

/// A call the class has no handler for: a node of another class's, which no entry of this one makes.
fn unhandled(w: &World, id: u32, c: Call<'_>) {
    panic!("tutorial: {} has no handler for {c:?}", classes::demo_of(w, id).cls);
}

/// GetAllActorsOfClass(TetherProjectile_C) destroyed at a demo's end. Not ported: no pawn of these demos throws one.
fn tethers(w: &World, cls: &str) {
    let tether = w.actors.projectiles.iter().any(|(pid, p)| {
        let own = ActorRef::Fighter(lookup_owner(w, ActorRef::Projectile(pid)));
        !p.dead && actor_is_a(w, own, p.cls, name!("TetherProjectile_C")) == Some(true)
    });
    if tether {
        warn_text(w, &format!("{cls}: a TetherProjectile_C left at the demo's end is not destroyed"));
    }
}

/// A call on one of this family's demo controllers.
pub fn call(w: &mut World, id: u32, c: Call<'_>) {
    use DemoState as D;
    match &classes::demo_of(w, id).state {
        D::WallSlide(_) => wall_slide(w, id, c),
        D::FastFall(_) => fast_fall(w, id, c),
        D::FastFallPlatform(_) => fast_fall_platform(w, id, c),
        D::KbiSide(_) => kbi(w, id, c, false),
        D::KbiUp(_) => kbi(w, id, c, true),
        _ => unreachable!("a demo of another family"),
    }
}

// ---- WallSlide

fn wall_slide(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(_) => delay(w, id, "@15", 0.1),
        Call::Fired(Fired::Delay("@15")) => {
            let team = run::data_team(w, 2);
            let bag = spawn_punching_bag(
                w,
                BagSpawn {
                    tag: "PunchingBag",
                    character: WONDER_WOMAN,
                    slot: 2,
                    team,
                    damage: 0.0,
                    spawn_invulnerability_time: Some(0.0),
                },
            );
            st!(w, id, WallSlide).bag = bag;
            let wall = Held::spawn(w, SolidClass::Wall, spec_of(id, 0), &[0.0, -1000.0, 1480.0], &[]);
            st!(w, id, WallSlide).wall = wall;
            if let (true, Some(bag)) = (callable(w, bag), bag) {
                bot_move_x(w, bag, 1.0);
            }
            delay(w, id, "@413", 0.35);
        }
        Call::Fired(Fired::Delay("@413")) => {
            wall_jump(w, id);
            delay(w, id, "@504", 0.35);
        }
        Call::Fired(Fired::Delay("@504")) => set_timer(w, id, "WallJump", 3.0),
        Call::Fired(Fired::Timer("WallJump")) => wall_jump(w, id),
        Call::Unpossessed { .. } => {
            let bag = st!(w, id, WallSlide).bag;
            if let (true, Some(bag)) = (callable(w, bag), bag) {
                run::despawn(w, bag);
            }
            let mut wall = st!(w, id, WallSlide).wall.take();
            solids::destroy(w, &mut wall);
            st!(w, id, WallSlide).wall = wall;
        }
        _ => unhandled(w, id, c),
    }
}

fn wall_jump(w: &mut World, id: u32) {
    let bag = st!(w, id, WallSlide).bag;
    if let (true, Some(bag)) = (callable(w, bag), bag) {
        bot_press(w, bag, BotBtn::Jump, None, false);
    }
}

// ---- FastFall

fn fast_fall(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            st!(w, id, FastFall).fighter = Some(f);
            delay(w, id, "possess", 0.5);
        }
        Call::Fired(Fired::Delay("possess")) => {
            let again = st!(w, id, FastFall).jump_again;
            set_timer(w, id, "Fastfall Loop", if again { 4.5 } else { 3.0 });
            fall_loop(w, id);
        }
        Call::Fired(Fired::Timer("Fastfall Loop")) => fall_loop(w, id),
        Call::Fired(Fired::Delay("@161")) => {
            let s = st!(w, id, FastFall);
            let (f, again) = (s.fighter, s.jump_again);
            if callable(w, f) {
                pad(w, id).down_pressed();
            }
            if !again {
                delay(w, id, "@15", 0.3);
                return;
            }
            delay(w, id, "@335", 0.2);
        }
        Call::Fired(Fired::Delay("@15")) => {}
        Call::Fired(Fired::Delay("@335")) => {
            let f = st!(w, id, FastFall).fighter;
            if callable(w, f) {
                pad(w, id).press(Btn::Jump, None);
            }
        }
        Call::Unpossessed { .. } => latent::clear_timer(w, TutorialObj::Demo(id), "Fastfall Loop"),
        _ => unhandled(w, id, c),
    }
}

fn fall_loop(w: &mut World, id: u32) {
    let f = st!(w, id, FastFall).fighter;
    if callable(w, f) {
        pad(w, id).press(Btn::Jump, None);
    }
    delay(w, id, "@161", 0.4);
}

// ---- FastFallPlatform

fn fast_fall_platform(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(_) => delay(w, id, "@15", 0.5),
        Call::Fired(Fired::Delay("@15")) => {
            let t = tagged(w, "Platform", 0);
            let p = t.and_then(|t| Held::spawn(w, SolidClass::Platform, spec_of(id, 0), &t.loc, &t.rot));
            if let Some(p) = &p {
                let (x, y) = p.at(w);
                emitter_at(w, name!("TutorialController_FastFall_Platform_C"), x, y);
                sound_at(w, CAST_SOUND, x, y);
            }
            st!(w, id, FastFallPlatform).platform = p;
            let team = run::data_team(w, 1);
            let bag = spawn_punching_bag(
                w,
                BagSpawn {
                    tag: "PunchingBagCenter",
                    character: WONDER_WOMAN,
                    slot: 2,
                    team,
                    damage: 0.0,
                    spawn_invulnerability_time: Some(0.0),
                },
            );
            st!(w, id, FastFallPlatform).bag = bag;
            delay(w, id, "@1937", 1.0);
        }
        Call::Fired(Fired::Delay("@1937")) => {
            set_timer(w, id, "Fastfall Loop", 3.0);
            platform_loop(w, id);
        }
        Call::Fired(Fired::Timer("Fastfall Loop")) => platform_loop(w, id),
        Call::Fired(Fired::Delay("@2894")) => {
            let bag = st!(w, id, FastFallPlatform).bag;
            if let (true, Some(bag)) = (callable(w, bag), bag) {
                bot_down(w, bag, true);
            }
            delay(w, id, "@2181", 0.3);
        }
        Call::Fired(Fired::Delay("@2181")) => {
            let bag = st!(w, id, FastFallPlatform).bag;
            if let (true, Some(bag)) = (callable(w, bag), bag) {
                bot_down(w, bag, false);
            }
        }
        Call::Unpossessed { .. } => {
            let mut p = st!(w, id, FastFallPlatform).platform.take();
            solids::destroy(w, &mut p);
            st!(w, id, FastFallPlatform).platform = p;
            let bag = st!(w, id, FastFallPlatform).bag;
            if let (true, Some(bag)) = (callable(w, bag), bag) {
                run::despawn(w, bag);
            }
            tethers(w, "TutorialController_FastFall_Platform");
        }
        _ => unhandled(w, id, c),
    }
}

fn platform_loop(w: &mut World, id: u32) {
    let bag = st!(w, id, FastFallPlatform).bag;
    if let (true, Some(bag)) = (callable(w, bag), bag) {
        bot_press(w, bag, BotBtn::Jump, None, true);
    }
    delay(w, id, "@2894", 0.4);
}

// ---- the KBI demos

/// The shared fields of the KBI demo `id`.
fn k(w: &mut World, id: u32) -> &mut KbiDemo {
    match &mut demo_mut(w, id).state {
        DemoState::KbiSide(s) => &mut s.k,
        DemoState::KbiUp(s) => &mut s.k,
        _ => unreachable!(),
    }
}

fn kbi(w: &mut World, id: u32, c: Call<'_>, up: bool) {
    match c {
        Call::Possessed(f) => {
            k(w, id).fighter = Some(f);
            delay(w, id, "possess", 0.1);
        }
        Call::Fired(Fired::Delay("possess")) => {
            let f = k(w, id).fighter;
            if callable(w, f) {
                let at = place_of(w, f.expect("callable"));
                k(w, id).at = Some(at);
            }
            if up {
                dummy(w, id, true);
                let t = tagged(w, "KBICeilRight", 0);
                let wall =
                    t.and_then(|t| Held::spawn(w, SolidClass::WallShort, spec_of(id, 0), &t.loc, &[0.0, 0.0, 90.0]));
                if let Some(h) = &wall {
                    let (x, y) = h.at(w);
                    sound_at(w, CAST_SOUND, x, y);
                }
                match &mut demo_mut(w, id).state {
                    DemoState::KbiUp(s) => s.wall = wall,
                    _ => unreachable!(),
                }
            } else {
                dummy(w, id, false);
            }
        }
        // AttackDummy: DemoFighter's BPI_NormalAttackPressed(dir)
        Call::Fired(Fired::Delay("@1658")) if !up => attack_dummy(w, id, Dir::Down),
        Call::Fired(Fired::Delay("@1867")) if up => attack_dummy(w, id, Dir::Up),
        // OnDummyHit: the Attackee's BPI_MoveX(PunchingBagsKilled even ? even : odd)
        Call::Delegate { handler: "dummyHit", src: Some(ActorRef::Fighter(f)), .. } => {
            let (even, odd) = if up { (0.0, 1.0) } else { (1.0, -1.0) };
            if callable(w, Some(f)) {
                let killed = k(w, id).killed;
                bot_move_x(w, f, if killed % 2 == 0 { even } else { odd });
            }
        }
        Call::Delegate { handler: "dummyDeath", .. } => {
            k(w, id).killed += 1;
            if up {
                delay(w, id, "@2624", 0.2);
            } else {
                delay(w, id, "@3758", 6.0 - 3.5);
            }
        }
        Call::Fired(Fired::Delay("@3758")) if !up => delay(w, id, "@2013", 1.5),
        Call::Fired(Fired::Delay("@2013")) if !up => dummy(w, id, false),
        Call::Fired(Fired::Delay("@2624")) if up => {
            let bag = k(w, id).bag;
            if let (true, Some(bag)) = (callable(w, bag), bag) {
                run::despawn(w, bag);
            }
            dummy(w, id, true);
        }
        Call::Unpossessed { .. } => {
            let s = k(w, id);
            s.complete = !s.complete;
            let bag = s.bag;
            if let (true, Some(bag)) = (callable(w, bag), bag) {
                run::despawn(w, bag);
            }
            if up {
                let mut wall = match &mut demo_mut(w, id).state {
                    DemoState::KbiUp(s) => s.wall.take(),
                    _ => unreachable!(),
                };
                solids::destroy(w, &mut wall);
                match &mut demo_mut(w, id).state {
                    DemoState::KbiUp(s) => s.wall = wall,
                    _ => unreachable!(),
                }
            } else {
                tethers(w, "TutorialController_KBISide");
            }
        }
        _ => unhandled(w, id, c),
    }
}

/// SpawnDummy / the demo's `dummy`: DemoComplete: nothing; else a pawn of the class at the first actor tagged
/// PunchingBag, FacingLeft, PlayerDataReadyServer(1), Team 1, CurrentDamage, an Idle_AIController,
/// KnockedBackIncludeSelf to OnDummyHit, OnExitedArena to the death handler, then the dummy's attack after its Delay.
fn dummy(w: &mut World, id: u32, up: bool) {
    if k(w, id).complete {
        return;
    }
    let (character, damage) = if up { (WONDER_WOMAN, 115.0) } else { (REINDOG, 100.0) };
    let bag = spawn_punching_bag(
        w,
        BagSpawn { tag: "PunchingBag", character, slot: 1, team: 1, damage, spawn_invulnerability_time: Some(0.0) },
    );
    k(w, id).bag = bag;
    if let Some(bag) = bag {
        let me = TutorialObj::Demo(id);
        classes::bind(w, DelegateList::Pawn(bag, PawnList::KnockedBackIncludeSelf), me, "dummyHit");
        classes::bind(w, DelegateList::Pawn(bag, PawnList::ExitedArena), me, "dummyDeath");
    }
    if up {
        delay(w, id, "@1867", 1.5);
    } else {
        delay(w, id, "@1658", 1.3);
    }
}

fn attack_dummy(w: &mut World, id: u32, dir: Dir) {
    let f = k(w, id).fighter;
    if callable(w, f) {
        pad(w, id).press(Btn::Attack, Some(dir));
    }
}
