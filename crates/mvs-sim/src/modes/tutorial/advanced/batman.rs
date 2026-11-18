//! The Batman tutorial's classes (TutorialActor_Batman_Fundementals, the port's own tile): the minigames
//! TutorialMinigame_Batman_Batarang, _Batarang_Retrieve and _Batarang_Destroy (`MinigameState`) and the demo controllers
//! TutorialController_Batman_Batarang, _Batarang_Retrieve and _Batarang_Destroy (`DemoState`). No recorded trace has a
//! Batman
//! class, so nothing here is on a trace: the decoded Blueprints
//! (the offsets below) are the source, the harness tests (`mvs-sim-trace/tests/tutorial_batman_paths.rs`)
//! the check.
//! The actor's own graph is FinishTutorial alone, which calls the parent's: nothing of its own runs here.
//! Each class's own SpawnPunchingBag(SpawnTag, 1, class) is `pawn::spawn_punching_bag` (FacingLeft,
//! SpawnInvulnerabilityTime 0, PlayerDataReadyServer(1), the team of player data 1, CurrentDamage 0, an
//! Idle_AIController).

use crate::Real;
use crate::State;
use crate::actors::{ActorId, ActorRef, ItemId};
use crate::ai::pad::Btn as BotBtn;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::actor_is_a;
use crate::fighter::match_side::{Foreign, SpawnRequest, TutorialEvent};
use crate::fighter::moves;
use crate::input::{Btn, Dir};
use crate::name;
use crate::names::Name;
use crate::world::World;
use crate::world::flow::{ActorList, DelegateCall, DelegateList, PawnList};
use mvs_data::value::{Props, Value};

use super::super::classes::{self, Call, Fired, RunList, demo_mut, demo_of, minigame_mut, minigame_of};
use super::super::demo::{DemoPad, DemoState, Place, callable, place_of, teleport};
use super::super::minigame::{self, MinigameState};
use super::super::pawn::{self, BagSpawn};
use super::super::run;
use super::super::{TutorialObj, latent};

const BATARANG: Name = name!("Item_BatarangV2_C");
const JACK: Name = name!("HarleyJackInTheBox_C");

// ---- the demo controllers' state

/// TutorialController_Batman_Batarang.
#[derive(State, Debug, Default)]
pub struct BatarangDemo {
    /// DemoFighter.
    pub fighter: Option<FighterId>,
    /// ReceivePossess's pawn (the Delay's capture).
    pub possessed: Option<FighterId>,
}

/// TutorialController_Batman_Batarang_Retrieve.
#[derive(State, Debug, Default)]
pub struct RetrieveDemo {
    pub fighter: Option<FighterId>,
    /// Initial Location.
    pub at: Option<Place>,
    pub possessed: Option<FighterId>,
    /// On Projectile Spawned's SpawnedActor (the event's parameter, overwritten by each call).
    pub spawned: Option<ActorRef>,
}

/// TutorialController_Batman_Batarang_Destroy (the demo pawn is Harley).
#[derive(State, Debug, Default)]
pub struct DestroyDemo {
    pub fighter: Option<FighterId>,
    pub at: Option<Place>,
    /// The controller's own BatmanV2 bag.
    pub bag: Option<FighterId>,
    pub possessed: Option<FighterId>,
}

// ---- the minigames' state

/// TutorialMinigame_Batman_Batarang.
#[derive(State, Debug, Default)]
pub struct BatarangMinigame {
    /// The tutorial fighter (TutorialActors[0].TutorialManagedPawn at BeginPlay).
    pub fighter: Option<FighterId>,
    /// Initial Position.
    pub at: Option<Place>,
    /// CurrentTargetStage.
    pub stage: i32,
    /// Targets and HitTargets.
    pub targets: Vec<ActorId>,
    pub hit_targets: Vec<ActorId>,
    /// The stage 3 Wonder Woman bag.
    pub bag: Option<FighterId>,
    /// Play VO With Delay's parameters (VOIndex, FighterToSpeak), overwritten by each call.
    pub vo_index: i32,
    pub vo_fighter: Option<FighterId>,
}

/// TutorialMinigame_Batman_Batarang_Retrieve: no fields of its own.
#[derive(State, Debug, Default)]
pub struct RetrieveMinigame {}

/// TutorialMinigame_Batman_Batarang_Destroy.
#[derive(State, Debug, Default)]
pub struct DestroyMinigame {
    /// Its BatmanV2 bag.
    pub bag: Option<FighterId>,
    /// On Batarang Damaged's DoOnce (@2098).
    pub vo_done: bool,
}

// ---- the family's classes (`classes.rs` asks every family; a class entered in two is an error)

/// The demo controller of a DemoAIController class of this family (its `DemoState`), `None` for a class it lacks.
pub fn make_demo(cls: &str) -> Option<(&'static str, DemoState)> {
    use DemoState as D;
    Some(match cls {
        "TutorialController_Batman_Batarang" => {
            ("TutorialController_Batman_Batarang", D::BatmanBatarang(BatarangDemo::default()))
        }
        "TutorialController_Batman_Batarang_Retrieve" => {
            ("TutorialController_Batman_Batarang_Retrieve", D::BatmanRetrieve(RetrieveDemo::default()))
        }
        "TutorialController_Batman_Batarang_Destroy" => {
            ("TutorialController_Batman_Batarang_Destroy", D::BatmanDestroy(DestroyDemo::default()))
        }
        _ => return None,
    })
}

/// The minigame actor of a CustomTutorialActors class of this family, `None` for a class it lacks.
pub fn make_minigame(cls: &str) -> Option<(&'static str, MinigameState)> {
    use MinigameState as M;
    Some(match cls {
        "TutorialMinigame_Batman_Batarang" => {
            ("TutorialMinigame_Batman_Batarang", M::BatmanBatarang(BatarangMinigame::default()))
        }
        "TutorialMinigame_Batman_Batarang_Retrieve" => {
            ("TutorialMinigame_Batman_Batarang_Retrieve", M::BatmanRetrieve(RetrieveMinigame::default()))
        }
        "TutorialMinigame_Batman_Batarang_Destroy" => {
            ("TutorialMinigame_Batman_Batarang_Destroy", M::BatmanDestroy(DestroyMinigame::default()))
        }
        _ => return None,
    })
}

/// A TutorialActor child class's own ReceiveBeginPlay: TutorialActor_Batman_Fundementals has none (its one graph is
/// FinishTutorial, which calls the parent's).
pub fn actor_begin_play(w: &mut World, cls: &str) {
    let _ = (w, cls);
}

/// The characters a class of this family is drawn with: the Batarang minigame's targets are Harley's
/// HarleyJackInTheBox and its stage 3 bag a Wonder Woman.
pub fn drawn_with(cls: &str) -> &'static [&'static str] {
    match cls {
        "TutorialMinigame_Batman_Batarang" => &["harley", "wonderwoman"],
        _ => &[],
    }
}

// ---- what the classes share

/// The state of the demo class of `id`, by its variant.
macro_rules! dst {
    ($w:expr, $id:expr, $v:ident) => {
        match &mut demo_mut($w, $id).state {
            DemoState::$v(s) => s,
            _ => unreachable!(),
        }
    };
}

/// The state of the minigame class of `id`, by its variant.
macro_rules! mst {
    ($w:expr, $id:expr, $v:ident) => {
        match &mut minigame_mut($w, $id).expect("a live minigame").state {
            MinigameState::$v(s) => s,
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

fn mdelay(w: &mut World, id: u32, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Minigame(id), node, s);
}

/// ReceivePossess's pawn for the Delay at `node` it starts: the first while that Delay is pending.
fn first_possess(w: &World, id: u32, node: &'static str, f: FighterId, cur: Option<FighterId>) -> Option<FighterId> {
    if latent::delay_pending(w, TutorialObj::Demo(id), node) { cur } else { Some(f) }
}

fn unhandled(w: &World, id: u32, c: Call<'_>) -> ! {
    panic!("tutorial: {} has no handler for {c:?}", demo_of(w, id).cls);
}

fn unhandled_minigame(w: &World, id: u32, c: Call<'_>) -> ! {
    panic!("tutorial: {} has no handler for {c:?}", minigame_of(w, id).map_or("a destroyed minigame", |m| m.cls));
}

/// The managed pawn: TutorialActors[0].TutorialManagedPawn.
fn managed(w: &World) -> Option<FighterId> {
    w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).and_then(|a| a.managed)
}

/// The actor's PunchingBag.
fn actor_bag(w: &World) -> Option<FighterId> {
    w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).and_then(|a| a.bag)
}

/// A live item that casts to Item_BatarangV2 (the class or a child of it).
fn is_batarang(w: &World, a: ActorRef) -> Option<ItemId> {
    let ActorRef::Item(it) = a else { return None };
    let i = w.actors.items.get(it).filter(|i| !i.dead)?;
    (actor_is_a(w, a, i.cls, BATARANG) == Some(true)).then_some(it)
}

/// GetAllActorsOfClass(Item_BatarangV2) and K2_DestroyActor on each.
fn destroy_batarangs(w: &mut World) {
    let ids: Vec<_> = w.actors.items.iter().map(|(id, _)| id).collect();
    for it in ids {
        if is_batarang(w, ActorRef::Item(it)).is_some() {
            crate::world::items::destroy_item(w, it);
        }
    }
}

/// `Destroy Batarang` (the Batarang minigame @4631): SpawnEmitterAtLocation(PS_CreatureCrystal_Destroy, its
/// location), then K2_DestroyActor.
fn destroy_batarang(w: &mut World, it: ItemId) {
    let Some(i) = w.actors.items.get(it).filter(|i| !i.dead) else { return };
    let (x, y, owner) = (i.x, i.y, i.owner);
    let owner_id = owner.map_or(-1, |o| w.fighter(o).id);
    w.events.push(Event::ProjectileDeath {
        cls: BATARANG,
        fighter: owner_id,
        vfx: Some(name!("PS_CreatureCrystal_Destroy")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    crate::world::items::destroy_item(w, it);
}

/// The graphs' own SpawnPunchingBag(PunchingBag, 1, Pawn_BatmanV2_C).
fn batman_bag(w: &mut World) -> Option<FighterId> {
    let team = run::data_team(w, 1);
    let b = BagSpawn {
        tag: "PunchingBag",
        character: name!("batman"),
        slot: 1,
        team,
        damage: 0.0,
        spawn_invulnerability_time: Some(0.0),
    };
    pawn::spawn_punching_bag(w, b)
}

/// Comp_Actor_VO.VOLocal(index) on a callable pawn.
fn vo_local(w: &mut World, f: Option<FighterId>, index: i32) {
    if let (true, Some(f)) = (callable(w, f), f) {
        crate::fighter::match_side::vo_local(w, f, usize::try_from(index).expect("a VO index"));
    }
}

// ---- the demo controllers

/// A call on one of this family's demo controllers.
pub fn demo(w: &mut World, id: u32, c: Call<'_>) {
    use DemoState as D;
    match &demo_of(w, id).state {
        D::BatmanBatarang(_) => batarang_demo(w, id, c),
        D::BatmanRetrieve(_) => retrieve_demo(w, id, c),
        D::BatmanDestroy(_) => destroy_demo(w, id, c),
        _ => unreachable!("not a Batman demo controller"),
    }
}

/// TutorialController_Batman_Batarang. ReceivePossess (@652): Delay 0.1 (-> @85): DemoFighter, a looping 3.0 s timer
/// to Throw Batarang (ThrowBatarangTimer), Delay 0.5 (@15 -> @70): Throw Batarang(). Throw Batarang (@752):
/// NormalAttackPressed(4 Neutral, false), NormalAttackReleased (@791). ReceiveUnPossess (@707): every Item_BatarangV2
/// destroyed; no timer clear.
fn batarang_demo(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, BatmanBatarang).possessed;
            let p = first_possess(w, id, "@652", f, cur);
            dst!(w, id, BatmanBatarang).possessed = p;
            delay(w, id, "@652", 0.1);
        }
        Call::Fired(Fired::Delay("@652")) => {
            let f = dst!(w, id, BatmanBatarang).possessed;
            dst!(w, id, BatmanBatarang).fighter = f;
            latent::set_timer(w, TutorialObj::Demo(id), "ThrowBatarang", 3.0, true);
            delay(w, id, "@15", 0.5);
        }
        Call::Fired(Fired::Delay("@15") | Fired::Timer("ThrowBatarang")) => {
            let f = dst!(w, id, BatmanBatarang).fighter;
            if callable(w, f) {
                let pd = pad(w, id);
                pd.press(Btn::Attack, Some(Dir::Neutral));
                pd.release(Btn::Attack);
            }
        }
        Call::Unpossessed { .. } => destroy_batarangs(w),
        _ => unhandled(w, id, c),
    }
}

/// TutorialController_Batman_Batarang_Retrieve. ReceivePossess (@1488): Delay 0.1 (-> @15): DemoFighter and its
/// location, a looping 8.0 s timer to Throw Batarang (@209); Delay 0.5 (@289 -> @344): Throw Batarang(), then
/// DemoFighter's Comp_Spawner.AfterSpawnedActor bound to On Projectile Spawned (@358-432).
/// On Projectile Spawned (@1719): Delay 0.4 (@1664 -> @836): SetMoveAxisY(-1.0); Delay 0.4 (@957 -> @474):
/// SetMoveAxisY(0.0); Delay 2.0 (@595 -> @650): SetMoveAxisX(1.0), the spawned actor's OnDestroyed bound to On Batarang
/// Destroyed (@771). On Batarang Destroyed (@1724): SetMoveAxisX(0.0); Delay 2.0 (@1845 -> @1012): DemoFighter
/// teleported back (@1062). ReceiveUnPossess (@1543): every batarang destroyed; no timer clear.
/// Unverified: the first throw's batarang spawns from the press in a later frame (the pad's press is the pawn's next
/// frame), so the bind ahead of it catches it (L4).
fn retrieve_demo(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Demo(id);
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, BatmanRetrieve).possessed;
            let p = first_possess(w, id, "@1488", f, cur);
            dst!(w, id, BatmanRetrieve).possessed = p;
            delay(w, id, "@1488", 0.1);
        }
        Call::Fired(Fired::Delay("@1488")) => {
            let f = dst!(w, id, BatmanRetrieve).possessed;
            dst!(w, id, BatmanRetrieve).fighter = f;
            if let (true, Some(f)) = (callable(w, f), f) {
                let at = place_of(w, f);
                dst!(w, id, BatmanRetrieve).at = Some(at);
            }
            latent::set_timer(w, me, "ThrowBatarang", 8.0, true);
            delay(w, id, "@289", 0.5);
        }
        Call::Fired(Fired::Delay("@289")) => {
            retrieve_throw(w, id);
            let f = dst!(w, id, BatmanRetrieve).fighter;
            if let (true, Some(f)) = (callable(w, f), f) {
                classes::bind(w, DelegateList::Pawn(f, PawnList::AfterSpawnedActor), me, "projectileSpawned");
            }
        }
        Call::Fired(Fired::Timer("ThrowBatarang")) => retrieve_throw(w, id),
        Call::Delegate { handler: "projectileSpawned", call: DelegateCall::AfterSpawnedActor { actor }, .. } => {
            dst!(w, id, BatmanRetrieve).spawned = Some(*actor);
            delay(w, id, "@1664", 0.4);
        }
        Call::Fired(Fired::Delay("@1664")) => {
            let f = dst!(w, id, BatmanRetrieve).fighter;
            if let (true, Some(f)) = (callable(w, f), f) {
                moves::set_move_axis_y(w, f, -1.0);
            }
            delay(w, id, "@957", 0.4);
        }
        Call::Fired(Fired::Delay("@957")) => {
            let f = dst!(w, id, BatmanRetrieve).fighter;
            if let (true, Some(f)) = (callable(w, f), f) {
                moves::set_move_axis_y(w, f, 0.0);
            }
            delay(w, id, "@595", 2.0);
        }
        Call::Fired(Fired::Delay("@595")) => {
            let s = dst!(w, id, BatmanRetrieve);
            let (f, spawned) = (s.fighter, s.spawned);
            if callable(w, f) {
                pad(w, id).set_move_axis_x(1.0);
            }
            if let Some(a) = spawned
                && !crate::combat::spawned_hits::dead(w, a)
            {
                classes::bind(w, DelegateList::Actor(a, ActorList::Destroyed), me, "batarangDestroyed");
            }
        }
        Call::Delegate { handler: "batarangDestroyed", .. } => {
            let f = dst!(w, id, BatmanRetrieve).fighter;
            if callable(w, f) {
                pad(w, id).set_move_axis_x(0.0);
            }
            delay(w, id, "@1845", 2.0);
        }
        Call::Fired(Fired::Delay("@1845")) => {
            let s = dst!(w, id, BatmanRetrieve);
            let (f, at) = (s.fighter, s.at);
            teleport(w, f, at);
        }
        Call::Unpossessed { .. } => destroy_batarangs(w),
        _ => unhandled(w, id, c),
    }
}

fn retrieve_throw(w: &mut World, id: u32) {
    let f = dst!(w, id, BatmanRetrieve).fighter;
    if callable(w, f) {
        let pd = pad(w, id);
        pd.press(Btn::Attack, Some(Dir::Neutral));
        pd.release(Btn::Attack);
    }
}

/// TutorialController_Batman_Batarang_Destroy. ReceivePossess (@2187): Delay 0.1 (-> @15): DemoFighter (Harley) and
/// her location, SpawnPunchingBag(PunchingBag, 1, Pawn_BatmanV2_C) (@186) with DisableCD (@255), DemoFighter's
/// OnTakeAnyDamage bound to On Harley Hit (@369), a looping 6.0 s timer to Throw Batarang (@456); Delay 0.5 (@536 ->
/// @591): Throw Batarang(). Throw Batarang (@2247): the bag's NormalAttackPressed(4, false) and NormalAttackReleased
/// (@2286); Delay 0.7 (@2322 -> @1037): the bag's SetMoveAxisY(-1.0); Delay 0.6 (@1158 -> @1213): SetMoveAxisY(0.0).
/// On Harley Hit (@2377): her SetMoveAxisY(-1.0) and SetMoveAxisX(1.0); Delay 1.1 (@2619 -> @1331): her
/// BPI_NormalAttackPressed(2 Forward) (no release), SetMoveAxisY(0.0), SetMoveAxisX(0.0); Delay 2.0 (@1691 -> @1746):
/// teleported back, CurrentDamage 0 (@1856), RemoveBuff(Buff_Weakened) (@2040). ReceiveUnPossess (@2242 -> @606): the
/// bag and every batarang destroyed; no timer clear.
fn destroy_demo(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Demo(id);
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, BatmanDestroy).possessed;
            let p = first_possess(w, id, "@2187", f, cur);
            dst!(w, id, BatmanDestroy).possessed = p;
            delay(w, id, "@2187", 0.1);
        }
        Call::Fired(Fired::Delay("@2187")) => {
            let f = dst!(w, id, BatmanDestroy).possessed;
            dst!(w, id, BatmanDestroy).fighter = f;
            if let (true, Some(f)) = (callable(w, f), f) {
                let at = place_of(w, f);
                dst!(w, id, BatmanDestroy).at = Some(at);
            }
            let bag = batman_bag(w);
            dst!(w, id, BatmanDestroy).bag = bag;
            if let Some(b) = bag {
                pawn::set_disable_cd(w, b, true);
            }
            if let (true, Some(f)) = (callable(w, f), f) {
                classes::bind(w, DelegateList::Pawn(f, PawnList::TakeAnyDamage), me, "harleyHit");
            }
            latent::set_timer(w, me, "ThrowBatarang", 6.0, true);
            delay(w, id, "@536", 0.5);
        }
        Call::Fired(Fired::Delay("@536") | Fired::Timer("ThrowBatarang")) => {
            let bag = dst!(w, id, BatmanDestroy).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                pawn::bot_press(w, b, BotBtn::Attack, Some(Dir::Neutral), false);
                pawn::bot_release(w, b, BotBtn::Attack);
            }
            delay(w, id, "@2322", 0.7);
        }
        Call::Fired(Fired::Delay("@2322")) => {
            let bag = dst!(w, id, BatmanDestroy).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                moves::set_move_axis_y(w, b, -1.0);
            }
            delay(w, id, "@1158", 0.6);
        }
        Call::Fired(Fired::Delay("@1158")) => {
            let bag = dst!(w, id, BatmanDestroy).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                moves::set_move_axis_y(w, b, 0.0);
            }
        }
        Call::Delegate { handler: "harleyHit", .. } => {
            let f = dst!(w, id, BatmanDestroy).fighter;
            if let (true, Some(f)) = (callable(w, f), f) {
                moves::set_move_axis_y(w, f, -1.0);
                pad(w, id).set_move_axis_x(1.0);
            }
            delay(w, id, "@2619", 1.1);
        }
        Call::Fired(Fired::Delay("@2619")) => {
            let f = dst!(w, id, BatmanDestroy).fighter;
            if let (true, Some(f)) = (callable(w, f), f) {
                pad(w, id).press(Btn::Attack, Some(Dir::Fwd));
                moves::set_move_axis_y(w, f, 0.0);
                pad(w, id).set_move_axis_x(0.0);
            }
            delay(w, id, "@1691", 2.0);
        }
        Call::Fired(Fired::Delay("@1691")) => {
            let s = dst!(w, id, BatmanDestroy);
            let (f, at) = (s.fighter, s.at);
            teleport(w, f, at);
            if let (true, Some(f)) = (callable(w, f), f) {
                pawn::set_current_damage(w, f, 0.0);
                crate::combat::buffs::remove_buff(w, f, Some(name!("Buff_Weakened_C")), None);
            }
        }
        Call::Unpossessed { .. } => {
            let bag = dst!(w, id, BatmanDestroy).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                run::despawn(w, b);
            }
            destroy_batarangs(w);
        }
        _ => unhandled(w, id, c),
    }
}

// ---- the minigames

/// A call on one of this family's minigame actors.
pub fn minigame(w: &mut World, id: u32, c: Call<'_>) {
    let kind = match &minigame_of(w, id).expect("a live minigame").state {
        MinigameState::BatmanBatarang(_) => 0,
        MinigameState::BatmanRetrieve(_) => 1,
        _ => 2,
    };
    match kind {
        0 => batarang_minigame(w, id, c),
        1 => retrieve_minigame(w, id, c),
        _ => destroy_minigame(w, id, c),
    }
}

/// The TargetPractice<n> points (custom content: the build's levels have no actor with these tags, so Spawn Target
/// finds none there). Each stage's points as (x, height above the stage's ground top): stage 0 one jack on the
/// floor straight ahead of the Player start, stage 1 one on the floor and one in the air, stage 2 two in the air and
/// on the floor further on, stage 3 one on the floor behind the PunchingBag start (the Wonder Woman bag stands
/// between). Six in all: task 0.0's six hits.
// Deviation: the port's own TargetPractice0 .. 3 points; the build reaches no target (spec 14 section 6).
const TARGET_PRACTICE: [&[(Real, Real)]; 4] =
    [&[(-250.0, 0.0)], &[(150.0, 0.0), (-150.0, 300.0)], &[(450.0, 450.0), (750.0, 0.0)], &[(900.0, 0.0)]];

/// TutorialMinigame_Batman_Batarang. ReceiveBeginPlay (@3331 -> @2059): the fighter and its location, Disable Moveset
/// (@5267-5928: BPI_DisableMovement, BPI_DisableJump, BPI_DisableInputType(3 Evade), (0 NormalAttack), (1
/// SpecialAttack), BPI_EnableInputTypeDirection(0, 4 Neutral)), its Comp_Spawner.AfterSpawnedActor bound to On
/// Projectile Spawned (@2201), TutorialStepCleanup bound to CleanUpSpawnedActor (@1872), Spawn Target (@1937), Play
/// VO With Delay(12, fighter, 0.7) (@1951).
/// On Projectile Spawned (@3901): for an Item_BatarangV2 (any other actor fails the cast @3966 and ends the call):
/// its OnActorHit bound to On Batarang Hit (-> Destroy Batarang @5109), then the fighter teleported back to Initial
/// Position (@4040-4090).
/// On Target Hit (@4151): a batarang causer is destroyed (Destroy Batarang); then a HarleyJackInTheBox not yet in
/// HitTargets: BPI_ProcessTutorialEvent(72) (@4523) and added. Any other causer: nothing.
/// On Target Destroyed (@4826): out of Targets; with none left every batarang destroyed (@2485), CurrentTargetStage
/// plus 1 (@2831) and by the new stage: 1 Spawn Target; 2 Play VO With Delay(341, fighter, 1.3) (@3256) and Spawn Target;
/// 3 the Wonder Woman bag (@3076), its SpecialAttackPressed(3 Back, ForceSpecialDirection) (@3212), its
/// DamageDisabled (@83) and Spawn Target; 4 the bag destroyed (@3294).
/// CleanUpSpawnedActor (@3724 -> @3709): the minigame destroyed; targets, batarangs and the bag stay.
/// Not ported: DestroyChildComponent(Comp_Actor_VO) on the bag (the Idle bag speaks no line the port plays).
fn batarang_minigame(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Minigame(id);
    match c {
        Call::BeginPlay => {
            let f = managed(w);
            mst!(w, id, BatmanBatarang).fighter = f;
            let (true, Some(f)) = (callable(w, f), f) else { return };
            let at = place_of(w, f);
            mst!(w, id, BatmanBatarang).at = Some(at);
            moves::disable_movement_input(w, f);
            moves::disable_jump(w, f);
            moves::disable_input_type(w, f, 3);
            moves::disable_input_type(w, f, 0);
            moves::disable_input_type(w, f, 1);
            moves::enable_input_type_direction(w, f, 0, 4);
            classes::bind(w, DelegateList::Pawn(f, PawnList::AfterSpawnedActor), me, "projectileSpawned");
            minigame::bind_steps(w, id, false, true);
            spawn_target(w, id);
            play_vo_with_delay(w, id, 12, Some(f), 0.7);
            // (the port's own overlap test of the batarangs and the targets, every frame)
            mdelay(w, id, "targetCheck", 0.0);
        }
        Call::Fired(Fired::Delay("@5208")) => {
            let s = mst!(w, id, BatmanBatarang);
            let (f, n) = (s.vo_fighter, s.vo_index);
            vo_local(w, f, n);
        }
        Call::Fired(Fired::Delay("targetCheck")) => {
            target_check(w, id);
            mdelay(w, id, "targetCheck", 0.0);
        }
        Call::Delegate { handler: "projectileSpawned", call: DelegateCall::AfterSpawnedActor { actor }, .. } => {
            let Some(it) = is_batarang(w, *actor) else { return };
            classes::bind(w, DelegateList::Actor(ActorRef::Item(it), ActorList::ActorHit), me, "batarangHit");
            let s = mst!(w, id, BatmanBatarang);
            let (f, at) = (s.fighter, s.at);
            teleport(w, f, at);
        }
        Call::Delegate { handler: "batarangHit", src: Some(ActorRef::Item(it)), .. } => destroy_batarang(w, it),
        Call::Delegate { handler: "targetHit", src, call: DelegateCall::TakeAnyDamage { causer, .. } } => {
            let Some(it) = causer.and_then(|a| is_batarang(w, a)) else { return };
            destroy_batarang(w, it);
            let Some(ActorRef::Actor(t)) = src else { return };
            if w.actors.actors.get(t).is_none_or(|a| a.cls != JACK) {
                return;
            }
            if mst!(w, id, BatmanBatarang).hit_targets.contains(&t) {
                return;
            }
            minigame::process_event(w, TutorialEvent::UniqueCharacterMechanic0);
            if let Some(m) = minigame_mut(w, id)
                && let MinigameState::BatmanBatarang(s) = &mut m.state
            {
                s.hit_targets.push(t);
            }
        }
        Call::Delegate { handler: "targetDestroyed", src: Some(ActorRef::Actor(t)), .. } => target_destroyed(w, id, t),
        Call::Delegate { handler: "onStepCleanup", .. } => minigame::destroy(w, id),
        Call::Destroyed => {}
        _ => unhandled_minigame(w, id, c),
    }
}

/// Play VO With Delay(VOIndex, FighterToSpeak, Delay): the parameters written, then Delay (@5208, a second call while
/// it is pending starts none: its end reads the newest parameters) to FighterToSpeak.Comp_Actor_VO.VOLocal(VOIndex).
fn play_vo_with_delay(w: &mut World, id: u32, index: i32, f: Option<FighterId>, s: Real) {
    let st = mst!(w, id, BatmanBatarang);
    st.vo_index = index;
    st.vo_fighter = f;
    mdelay(w, id, "@5208", s);
}

/// Spawn Target (@3729): every actor tagged "TargetPractice" + CurrentTargetStage: a HarleyJackInTheBox at it (@153),
/// added to Targets, UpdateMaxLifetimeWithAuthority(0.0) (@834), OnTakeAnyDamage bound to On Target Hit (@875) and
/// OnDestroyed to On Target Destroyed (@939).
/// Deviation: the points are the port's own (`TARGET_PRACTICE`); the build's SpawnActor has no owner, the port's jack
/// is the tutorial fighter's (a world actor needs one), so its team is his, not -1, and it faces his way.
/// Not ported: its Comp_Actor_KeepOnCamera (@704-798), and on stage 2 the Comp_RunAlongEdgeTrace of the second and
/// later targets (@1178-1420: the port's edge runner moves projectiles only): they stand still.
fn spawn_target(w: &mut World, id: u32) {
    let me = TutorialObj::Minigame(id);
    let s = mst!(w, id, BatmanBatarang);
    let (stage, f) = (s.stage, s.fighter);
    let (true, Some(f)) = (callable(w, f), f) else { return };
    let Some(points) = usize::try_from(stage).ok().and_then(|i| TARGET_PRACTICE.get(i)) else { return };
    let ground = crate::math::narrow(w.stage.ground.top);
    for &(x, dy) in *points {
        // (the spawn place is the owner's place plus the transform's Y along his facing and Z above his actor
        // place - 100: `spawn_place`)
        let facing = crate::fighter::facing(w, f);
        let ty = (x - w.fighter(f).body.x) * facing;
        let tz = ground + dy - crate::fighter::actor_y(w, f) + 100.0;
        let mut tr = Props::new();
        tr.insert("Translation".into(), xyz(0.0, ty, tz));
        let mut p = Props::new();
        p.insert("Transforms".into(), Value::Obj(tr));
        p.insert("Socket".into(), Value::Str("None".into()));
        let r = SpawnRequest {
            cls: JACK,
            n: crate::events::NotifyRef { montage: Name::NONE, index: 0 },
            attack_id: w.fighter(f).decay.attack_count,
            foreign: Foreign::None,
            copy: None,
            charge: None,
            aim: None,
            props: Some(std::sync::Arc::new(p)),
            instigator: None,
            origin: None,
        };
        let Some(t) = crate::world::spawning::spawn_world_actor(w, f, &r) else { continue };
        mst!(w, id, BatmanBatarang).targets.push(t);
        crate::chars::harley::actors::set_endless(w, t);
        classes::bind(w, DelegateList::Actor(ActorRef::Actor(t), ActorList::TakeAnyDamage), me, "targetHit");
        classes::bind(w, DelegateList::Actor(ActorRef::Actor(t), ActorList::Destroyed), me, "targetDestroyed");
    }
}

fn xyz(x: Real, y: Real, z: Real) -> Value {
    let o = [("X", x), ("Y", y), ("Z", z)].into_iter().map(|(k, v)| (k.to_owned(), Value::Num(f64::from(v))));
    Value::Obj(o.collect())
}

/// The port's own hit of a target: a batarang's ThrowHitBox (its hit box able to deal) beginning to overlap a live
/// target's HurtBox: the target's ReceiveAnyDamage (Explode), then its OnTakeAnyDamage with the batarang as the
/// DamageCauser (AActor::TakeDamage's order).
/// Deviation: the build's hit is the batarang's pfgDoDamage on the jack (a projectile victim: the clank or
/// DestroyProjectiles branch, spec 14 N3.6), which the port's item hits do not run against a world actor's HurtBox;
/// this test stands for it, once a frame from the minigame's own latent loop, with no clank back on the batarang.
fn target_check(w: &mut World, id: u32) {
    let mut k = 0;
    while let Some(t) = target_of(w, id, k) {
        k += 1;
        let mut n = 0;
        while let Some(it) = w.actors.items.nth(n) {
            n += 1;
            if is_batarang(w, ActorRef::Item(it)).is_none() || w.actors.actors.get(t).is_none_or(|a| a.dead) {
                continue;
            }
            let i = w.actors.items.get(it).expect("a live item");
            if !crate::actors::item::item_box_on(w, it) || i.holder >= 0 {
                continue;
            }
            let (x, y, dmg) = (i.x, i.y, i.hit.damage.unwrap_or(0.0));
            let r = crate::actors::item::item_hit_radius(w, it);
            let s = crate::combat::boxes::WorldSphere { x, y, r, z: Some(0.0), ..Default::default() };
            if r <= 0.0 || !crate::chars::harley::actors::hurt_box_meets(w, t, &s) {
                continue;
            }
            crate::chars::harley::actors::receive_any_damage(w, t);
            crate::world::flow::broadcast_take_any_damage(w, ActorRef::Actor(t), dmg, Some(ActorRef::Item(it)));
        }
    }
}

/// Targets[k] of the Batarang minigame `id` while it lives (a hit's step end destroys it in the loop).
fn target_of(w: &World, id: u32, k: usize) -> Option<ActorId> {
    match &minigame_of(w, id)?.state {
        MinigameState::BatmanBatarang(s) => s.targets.get(k).copied(),
        _ => None,
    }
}

/// On Target Destroyed (@4826).
fn target_destroyed(w: &mut World, id: u32, t: ActorId) {
    let s = mst!(w, id, BatmanBatarang);
    s.targets.retain(|&x| x != t);
    if !s.targets.is_empty() {
        return;
    }
    // (@2733: Destroy Batarang on each, its emitter included)
    let mut n = 0;
    while let Some(it) = w.actors.items.nth(n) {
        n += 1;
        if is_batarang(w, ActorRef::Item(it)).is_some() {
            destroy_batarang(w, it);
        }
    }
    let s = mst!(w, id, BatmanBatarang);
    s.stage += 1;
    let (stage, f) = (s.stage, s.fighter);
    match stage {
        1 => spawn_target(w, id),
        2 => {
            play_vo_with_delay(w, id, 341, f, 1.3);
            spawn_target(w, id);
        }
        3 => {
            let bag = run::spawn_idle_bag(w, "PunchingBag", 0.0, Some(0.0));
            mst!(w, id, BatmanBatarang).bag = bag;
            if let Some(b) = bag {
                pawn::bot_press_forced(w, b, BotBtn::Special, Dir::Back);
                w.fighter_mut(b).invuln.damage_disabled = true;
            }
            spawn_target(w, id);
        }
        4 => {
            let bag = mst!(w, id, BatmanBatarang).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                run::despawn(w, b);
            }
        }
        _ => {}
    }
}

/// TutorialMinigame_Batman_Batarang_Retrieve. ReceiveBeginPlay (@796): TutorialStepCleanup bound to
/// CleanupTutorialStep (@283), the game mode's PlayerRespawned to Set Up Bindings (@656), Set Up Bindings(0) (@720).
/// Set Up Bindings (@2200): the fighter's Comp_Spawner.AfterSpawnedActor bound to On Projectile Spawned (@2246),
/// Disable Moveset (@1847-2158: BPI_DisableInputType(0), (1), BPI_EnableInputTypeDirection(0, 4)), the actor's
/// PunchingBag's OnTakeAnyDamage bound to On Punching Bag Hit (@2401).
/// On Punching Bag Hit (@1003): a DamageCauser of class Item_BatarangV2_C itself: ProcessTutorialEvent(72) (@1176).
/// On Projectile Spawned (@1216): a batarang's PickupArea begin overlap bound to On Batarang Retrieved (@1291).
/// On Batarang Retrieved (@1514): the fighter overlapping it while it lies Idle: ProcessTutorialEvent(73) (@1474).
/// CleanupTutorialStep (@2443 -> @15): ClearMulticastDelegate on PlayerRespawned (@111); the minigame stays.
/// Not ported: the Comp_Actor_Destructible OnPlayerRespawned -> UpdateAllyListServer binds the clear also drops (the
/// port's ally list is no binding of that delegate).
fn retrieve_minigame(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Minigame(id);
    match c {
        Call::BeginPlay => {
            minigame::bind_steps(w, id, false, true);
            classes::bind_run(w, RunList::PlayerRespawned, me, "setUpBindings");
            set_up_bindings(w, id);
        }
        Call::Delegate { handler: "setUpBindings", .. } => set_up_bindings(w, id),
        Call::Delegate { handler: "bagHit", call: DelegateCall::TakeAnyDamage { causer, .. }, .. } => {
            let Some(ActorRef::Item(it)) = *causer else { return };
            if w.actors.items.get(it).is_some_and(|i| i.cls == BATARANG) {
                minigame::process_event(w, TutorialEvent::UniqueCharacterMechanic0);
            }
        }
        Call::Delegate { handler: "projectileSpawned", call: DelegateCall::AfterSpawnedActor { actor }, .. } => {
            if let Some(it) = is_batarang(w, *actor) {
                let at = DelegateList::Actor(ActorRef::Item(it), ActorList::PickupOverlap);
                classes::bind(w, at, me, "retrieved");
            }
        }
        Call::Delegate { handler: "retrieved", call: DelegateCall::PickupOverlap { other, idle }, .. } => {
            let f = managed(w);
            if *idle && f.is_some_and(|f| *other == ActorRef::Fighter(f)) {
                minigame::process_event(w, TutorialEvent::UniqueCharacterMechanic1);
            }
        }
        Call::Delegate { handler: "onStepCleanup", .. } => {
            if let Some(t) = w.tutorial.as_mut() {
                t.player_respawned.clear();
            }
        }
        Call::Destroyed => {}
        _ => unhandled_minigame(w, id, c),
    }
}

/// Set Up Bindings (@2200).
fn set_up_bindings(w: &mut World, id: u32) {
    let me = TutorialObj::Minigame(id);
    let f = managed(w);
    if let (true, Some(f)) = (callable(w, f), f) {
        classes::bind(w, DelegateList::Pawn(f, PawnList::AfterSpawnedActor), me, "projectileSpawned");
        moves::disable_input_type(w, f, 0);
        moves::disable_input_type(w, f, 1);
        moves::enable_input_type_direction(w, f, 0, 4);
    }
    let bag = actor_bag(w);
    if let (true, Some(b)) = (callable(w, bag), bag) {
        classes::bind(w, DelegateList::Pawn(b, PawnList::TakeAnyDamage), me, "bagHit");
    }
}

/// TutorialMinigame_Batman_Batarang_Destroy (the player is Harley). ReceiveBeginPlay (@2136): the fighter's
/// CurrentDamage 0 (@1541), TutorialStepCleanup bound to CleanupTutorialStep (@941), SpawnPunchingBag(PunchingBag,
/// 1, Pawn_BatmanV2_C) (@1006), the bag's DisableCD (@1075) and DamageDisabled (@1189), its AfterSpawnedActor bound to
/// On Projectile Spawned (@1244), a looping 5.0 s timer to Throw Batarang (@1353) and Throw Batarang() (@1406).
/// Throw Batarang (@2435): the bag's BPI_NormalAttackPressed(4 Neutral) (@2514, held: no release); Delay 1.0 (@2553 ->
/// @15): its SetMoveAxisY(-1.0) (@94); Delay 0.3 (@136 -> @191): SetMoveAxisY(0.0) (@266).
/// On Projectile Spawned (@2151): a batarang's OnTakeAnyDamage bound to On Batarang Damaged (@2226).
/// On Batarang Damaged (@2608): a DamageCauser of the fighter's class: ProcessTutorialEvent(72) (@2391), then once
/// (@2098) the bag's VOLocal(140).
/// CleanupTutorialStep (@2742): the bag and every batarang destroyed (@2778-1945); the minigame and its timer stay
/// (its calls on the destroyed bag are skipped).
fn destroy_minigame(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Minigame(id);
    match c {
        Call::BeginPlay => {
            let f = managed(w);
            if let (true, Some(f)) = (callable(w, f), f) {
                pawn::set_current_damage(w, f, 0.0);
            }
            minigame::bind_steps(w, id, false, true);
            let bag = batman_bag(w);
            mst!(w, id, BatmanDestroy).bag = bag;
            if let Some(b) = bag {
                pawn::set_disable_cd(w, b, true);
                w.fighter_mut(b).invuln.damage_disabled = true;
                classes::bind(w, DelegateList::Pawn(b, PawnList::AfterSpawnedActor), me, "projectileSpawned");
            }
            latent::set_timer(w, me, "Throw Batarang", 5.0, true);
            destroy_throw(w, id);
        }
        Call::Fired(Fired::Timer("Throw Batarang")) => destroy_throw(w, id),
        Call::Fired(Fired::Delay("@2553")) => {
            let bag = mst!(w, id, BatmanDestroy).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                moves::set_move_axis_y(w, b, -1.0);
            }
            mdelay(w, id, "@136", 0.3);
        }
        Call::Fired(Fired::Delay("@136")) => {
            let bag = mst!(w, id, BatmanDestroy).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                moves::set_move_axis_y(w, b, 0.0);
            }
        }
        Call::Delegate { handler: "projectileSpawned", call: DelegateCall::AfterSpawnedActor { actor }, .. } => {
            if let Some(it) = is_batarang(w, *actor) {
                let at = DelegateList::Actor(ActorRef::Item(it), ActorList::TakeAnyDamage);
                classes::bind(w, at, me, "batarangDamaged");
            }
        }
        Call::Delegate { handler: "batarangDamaged", call: DelegateCall::TakeAnyDamage { causer, .. }, .. } => {
            let Some(ActorRef::Fighter(g)) = *causer else { return };
            let Some(f) = managed(w) else { return };
            if w.fighter(g).ch != w.fighter(f).ch {
                return;
            }
            minigame::process_event(w, TutorialEvent::UniqueCharacterMechanic0);
            let s = mst!(w, id, BatmanDestroy);
            if s.vo_done {
                return;
            }
            s.vo_done = true;
            let bag = s.bag;
            vo_local(w, bag, 140);
        }
        Call::Delegate { handler: "onStepCleanup", .. } => {
            let bag = mst!(w, id, BatmanDestroy).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                run::despawn(w, b);
            }
            destroy_batarangs(w);
        }
        Call::Destroyed => {}
        _ => unhandled_minigame(w, id, c),
    }
}

/// Throw Batarang (@2435).
fn destroy_throw(w: &mut World, id: u32) {
    let bag = mst!(w, id, BatmanDestroy).bag;
    if let (true, Some(b)) = (callable(w, bag), bag) {
        pawn::bot_press(w, b, BotBtn::Attack, Some(Dir::Neutral), false);
    }
    mdelay(w, id, "@2553", 1.0);
}
