//! The advanced tutorials' fighters classes (`fighters.rs`, `fighters/{demos,minigames}.rs`): the
//! demo controllers of DodgeMeter, Cooldowns, Armor, DodgeJump and AttackDecay step 1 (their own state, the payloads
//! of `DemoState`), the minigames TutorialMinigame_DodgeBasics, _Dodge_Refill and _ArmorBreak (`MinigameState`) and
//! TutorialActor_AttackDecay's own ReceiveBeginPlay.
//!
//! Each class's own SpawnPunchingBag(SpawnTag, PlayerIndex 1, Pawn_WonderWomanV3) is the host's `spawn_idle_bag`
//! (FacingLeft, SpawnInvulnerabilityTime 0, PlayerDataReadyServer(1), the team of player data 1, CurrentDamage 0,
//! Idle_AIController).
//! Not ported: PS_WonderWoman_TutorialSpawn (Armor_Break @531-1223, AttackDecay_Variety @1175 / @1348, the minigame
//! ArmorBreak @1343) and PS_Invulnerability on both meshes (AttackDecay_Variety @2241, @2413), as the beginner
//! AttackDecay_Basics.
//! What a closure would capture is a field of the class's state; a `possessed` field holds the pawn ReceivePossess's
//! Delay captured (`first_possess`).

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::ai::bt::BotController;
use crate::ai::pad::Btn as BotBtn;
use crate::combat::buffs::spawn_buff;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_is_a, lookup_owner};
use crate::input::{Btn, Dir};
use crate::name;
use crate::world::World;
use crate::world::flow::{DelegateCall, DelegateList, PawnList};
use crate::world::pawns;

use super::super::classes::{self, Call, Fired, demo_mut, demo_of, minigame_mut, minigame_of};
use super::super::demo::{DemoPad, DemoState, Place, active, callable, get_pawn, place_of, teleport};
use super::super::minigame::{self, MinigameState};
use super::super::pawn;
use super::super::run;
use super::super::{TutorialObj, latent};
use crate::fighter::match_side::TutorialEvent;

// ---- the demo controllers' state (the record writes them under these names)

/// TutorialController_Dodge_Basics.
#[derive(State, Debug, Default)]
pub struct DodgeBasicsDemo {
    pub fighter: Option<FighterId>,
    pub at: Option<Place>,
    pub bag: Option<FighterId>,
    /// ReceivePossess's pawn (the Delay's capture).
    pub possessed: Option<FighterId>,
}

/// TutorialController_Dodge_DodgeMeterDeplete.
#[derive(State, Debug, Default)]
pub struct DodgeMeterDeplete {
    pub fighter: Option<FighterId>,
    pub possessed: Option<FighterId>,
}

/// TutorialController_Dodge_DodgeMeterReplenish.
#[derive(State, Debug, Default)]
pub struct DodgeMeterReplenish {
    pub possessed: Option<FighterId>,
}

/// TutorialController_Cooldowns_Basics.
#[derive(State, Debug, Default)]
pub struct CooldownsBasics {
    pub fighter: Option<FighterId>,
}

/// TutorialController_Armor_Basics.
#[derive(State, Debug, Default)]
pub struct ArmorBasics {
    pub fighter: Option<FighterId>,
    pub bag: Option<FighterId>,
    /// The `w` (the World, set by ReceivePossess and ReceiveUnPossess): the record writes it null until then.
    pub w_set: bool,
    pub possessed: Option<FighterId>,
}

/// TutorialController_Armor_Break.
#[derive(State, Debug, Default)]
pub struct ArmorBreakDemo {
    pub fighter: Option<FighterId>,
    pub at: Option<Place>,
    pub bag: Option<FighterId>,
    pub bag_at: Option<Place>,
    pub complete: bool,
    pub possessed: Option<FighterId>,
}

/// TutorialController_DodgeJump (FirstLoop true).
#[derive(State, Debug)]
pub struct DodgeJump {
    pub at: Option<Place>,
    pub first_loop: bool,
    pub cast: Option<FighterId>,
    pub possessed: Option<FighterId>,
}

impl Default for DodgeJump {
    fn default() -> Self {
        DodgeJump { at: None, first_loop: true, cast: None, possessed: None }
    }
}

/// TutorialController_AttackDecay_Variety.
#[derive(State, Debug, Default)]
pub struct AttackDecayVariety {
    pub fighter: Option<FighterId>,
    pub at: Option<Place>,
    pub bag: Option<FighterId>,
    pub bag_at: Option<Place>,
    pub complete: bool,
}

// ---- the minigames' state

/// TutorialMinigame_DodgeBasics (EnemyMinigame's fields): SpawnedEnemies, the newest at index 0, and
/// DodgingMinigameComplete.
#[derive(State, Debug, Default)]
pub struct DodgeBasicsMinigame {
    pub enemies: Vec<FighterId>,
    pub game_over: bool,
}

/// TutorialMinigame_Dodge_Refill: no fields.
#[derive(State, Debug, Default)]
pub struct DodgeRefill {}

/// TutorialMinigame_ArmorBreak (EnemyMinigame's fields): SpawnedEnemies and ArmorMinigameComplete.
#[derive(State, Debug, Default)]
pub struct ArmorBreakMinigame {
    pub enemies: Vec<FighterId>,
    pub game_over: bool,
}

// ---- the family's classes (`classes.rs` asks every family; a class entered in two is an error)

/// The demo controller of a DemoAIController class of this family (its `DemoState`), `None` for a class it lacks.
pub fn make_demo(cls: &str) -> Option<(&'static str, DemoState)> {
    use DemoState as D;
    Some(match cls {
        "TutorialController_Dodge_Basics" => {
            ("TutorialController_Dodge_Basics", D::DodgeBasics(DodgeBasicsDemo::default()))
        }
        "TutorialController_Dodge_DodgeMeterDeplete" => {
            ("TutorialController_Dodge_DodgeMeterDeplete", D::DodgeMeterDeplete(DodgeMeterDeplete::default()))
        }
        "TutorialController_Dodge_DodgeMeterReplenish" => {
            ("TutorialController_Dodge_DodgeMeterReplenish", D::DodgeMeterReplenish(DodgeMeterReplenish::default()))
        }
        "TutorialController_Cooldowns_Basics" => {
            ("TutorialController_Cooldowns_Basics", D::CooldownsBasics(CooldownsBasics::default()))
        }
        "TutorialController_Armor_Basics" => {
            ("TutorialController_Armor_Basics", D::ArmorBasics(ArmorBasics::default()))
        }
        "TutorialController_Armor_Break" => {
            ("TutorialController_Armor_Break", D::ArmorBreak(ArmorBreakDemo::default()))
        }
        "TutorialController_DodgeJump" => ("TutorialController_DodgeJump", D::DodgeJump(DodgeJump::default())),
        "TutorialController_AttackDecay_Variety" => {
            ("TutorialController_AttackDecay_Variety", D::AttackDecayVariety(AttackDecayVariety::default()))
        }
        _ => return None,
    })
}

/// The minigame actor of a CustomTutorialActors class of this family, `None` for a class it lacks.
pub fn make_minigame(cls: &str) -> Option<(&'static str, MinigameState)> {
    use MinigameState as M;
    Some(match cls {
        "TutorialMinigame_DodgeBasics" => {
            ("TutorialMinigame_DodgeBasics", M::DodgeBasics(DodgeBasicsMinigame::default()))
        }
        "TutorialMinigame_Dodge_Refill" => ("TutorialMinigame_Dodge_Refill", M::DodgeRefill(DodgeRefill::default())),
        "TutorialMinigame_ArmorBreak" => ("TutorialMinigame_ArmorBreak", M::ArmorBreak(ArmorBreakMinigame::default())),
        _ => return None,
    })
}

/// A TutorialActor child class's own ReceiveBeginPlay, after the parent's: nothing for a class this family lacks.
/// TutorialActor_AttackDecay: the parent's, then BPI_SetAttackDecayEnabled(true) (@110).
pub fn actor_begin_play(w: &mut World, cls: &str) {
    if cls == "TutorialActor_AttackDecay" {
        w.flow.same_attack_penalty = true;
    }
}

/// The characters a class of this family is drawn with (`TutorialClasses.drawnWith`): none (the family has no
/// entries).
pub fn drawn_with(cls: &str) -> &'static [&'static str] {
    let _ = cls;
    &[]
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

fn pad(w: &mut World, id: u32) -> &mut DemoPad {
    &mut demo_mut(w, id).pad
}

fn delay(w: &mut World, id: u32, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Demo(id), node, s);
}

fn set_timer(w: &mut World, id: u32, name: &'static str, s: Real) {
    latent::set_timer(w, TutorialObj::Demo(id), name, s, true);
}

fn clear_timer(w: &mut World, id: u32, name: &'static str) {
    latent::clear_timer(w, TutorialObj::Demo(id), name);
}

/// ReceivePossess's pawn for the Delay at `node` it starts: the first while that Delay is pending (a second call of a
/// pending Delay starts nothing, so its link runs with the first call's pawn).
fn first_possess(w: &World, id: u32, node: &'static str, f: FighterId, cur: Option<FighterId>) -> Option<FighterId> {
    if latent::delay_pending(w, TutorialObj::Demo(id), node) { cur } else { Some(f) }
}

/// A call the class has no handler for: a node of another class's, which no entry of this one makes.
fn unhandled(w: &World, id: u32, c: Call<'_>) -> ! {
    panic!("tutorial: {} has no handler for {c:?}", demo_of(w, id).cls);
}

/// GetAllActorsOfClass(cls) and K2_DestroyActor on each: the projectiles of the class or a child of it.
fn destroy_projectiles_of(w: &mut World, cls: crate::names::Name) {
    let ids: Vec<_> = w.actors.projectiles.iter().map(|(id, _)| id).collect();
    for pid in ids {
        let Some(p) = w.actors.projectiles.get(pid) else { continue };
        let (dead, pcls) = (p.dead, p.cls);
        let own = ActorRef::Fighter(lookup_owner(w, ActorRef::Projectile(pid)));
        if !dead && actor_is_a(w, own, pcls, cls) == Some(true) {
            crate::world::spawning::destroy_projectile(w, pid, false);
        }
    }
}

/// The same for the items of a class (Item_Sandwich_C: no child class).
fn destroy_items_of(w: &mut World, cls: crate::names::Name) {
    let ids: Vec<_> = w.actors.items.iter().map(|(id, _)| id).collect();
    for iid in ids {
        let Some(it) = w.actors.items.get(iid) else { continue };
        if !it.dead && it.cls == cls {
            crate::world::items::destroy_item(w, iid);
        }
    }
}

/// The bag's Buff_ArmorPack_C: SpawnBuffActor(Buff_ArmorPack_C, bag, bag, 1, 3.0, false, false) (Armor_Basics @892,
/// Armor_Break @1574).
fn armor_pack(w: &mut World, bag: Option<FighterId>) {
    if let (true, Some(b)) = (callable(w, bag), bag) {
        spawn_buff(w, b, name!("Buff_ArmorPack_C"), 3.0, Some(1), Some(b), false, Some(b));
    }
}

// ---- the demo controllers

/// A call on one of this family's demo controllers (`TutorialRun::demos`, by id).
pub fn demo(w: &mut World, id: u32, c: Call<'_>) {
    use DemoState as D;
    match &demo_of(w, id).state {
        D::DodgeBasics(_) => dodge_basics(w, id, c),
        D::DodgeMeterDeplete(_) => dodge_meter_deplete(w, id, c),
        D::DodgeMeterReplenish(_) => dodge_meter_replenish(w, id, c),
        D::CooldownsBasics(_) => cooldowns_basics(w, id, c),
        D::ArmorBasics(_) => armor_basics(w, id, c),
        D::ArmorBreak(_) => armor_break(w, id, c),
        D::DodgeJump(_) => dodge_jump(w, id, c),
        D::AttackDecayVariety(_) => attack_decay_variety(w, id, c),
        _ => unreachable!("not a fighters demo controller"),
    }
}

/// TutorialController_Dodge_Basics. ReceivePossess: Delay 0.1 (@754 -> @15): DemoFighter and its location, a bag at
/// PunchingBagClose, the bag's Comp_Pawn_ChargeAttackManager.ChargeEnd bound to OnAttackCharged (@278), DemoFighter's
/// EvadeInvulnerabilityRechargeTime 0.5 and BaseEvadeInvulnerability 0.26 (@341, @386), a looping 5.0 s timer to Demo
/// and Demo() now. Demo (@866): the bag's BPI_NormalAttackPressed(1 Down), never released (it charges to its end).
/// OnAttackCharged (@1024 -> @981): DemoFighter.EvadeButtonPressed(5), then MoveComponentTo(the stored location,
/// 2.0 s, ease out, ease in) with no link (@661). ReceiveUnPossess (@809): the bag destroyed, the timer cleared.
fn dodge_basics(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Demo(id);
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, DodgeBasics).possessed;
            let p = first_possess(w, id, "@754", f, cur);
            dst!(w, id, DodgeBasics).possessed = p;
            delay(w, id, "@754", 0.1);
        }
        Call::Fired(Fired::Delay("@754")) => {
            let f = dst!(w, id, DodgeBasics).possessed;
            dst!(w, id, DodgeBasics).fighter = f;
            if let (true, Some(f)) = (callable(w, f), f) {
                let at = place_of(w, f);
                dst!(w, id, DodgeBasics).at = Some(at);
            }
            let bag = run::spawn_idle_bag(w, "PunchingBagClose", 0.0, Some(0.0));
            dst!(w, id, DodgeBasics).bag = bag;
            if let Some(b) = bag {
                classes::bind(w, DelegateList::Pawn(b, PawnList::ChargeEnd), me, "charged");
            }
            if let (true, Some(f)) = (callable(w, f), f) {
                pawn::set_evade_recharge_time(w, f, 0.5);
                pawn::set_base_evade_invulnerability(w, f, 0.26);
            }
            set_timer(w, id, "Demo", 5.0);
            dodge_basics_demo(w, id);
        }
        Call::Fired(Fired::Timer("Demo")) => dodge_basics_demo(w, id),
        Call::Delegate { handler: "charged", .. } => {
            let s = dst!(w, id, DodgeBasics);
            let (f, at) = (s.fighter, s.at);
            let (true, Some(f)) = (callable(w, f), f) else { return };
            pad(w, id).press(Btn::Dodge, None);
            if let Some(at) = at {
                latent::move_to(w, me, "@661", latent::MoveTarget::Fighter(f), at.x, at.y, 2.0, true, true);
            }
        }
        Call::Fired(Fired::Move("@661")) => {}
        Call::Unpossessed { .. } => {
            let bag = dst!(w, id, DodgeBasics).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                run::despawn(w, b);
            }
            clear_timer(w, id, "Demo");
        }
        _ => unhandled(w, id, c),
    }
}

fn dodge_basics_demo(w: &mut World, id: u32) {
    let bag = dst!(w, id, DodgeBasics).bag;
    if let (true, Some(b)) = (callable(w, bag), bag) {
        pawn::bot_press(w, b, BotBtn::Attack, Some(Dir::Down), false);
    }
}

/// TutorialController_Dodge_DodgeMeterDeplete. ReceivePossess: Delay 0.1 (@340 -> @15): DemoFighter, a looping 2.0 s
/// timer to Demo and one call. Demo (@227): DemoFighter's BPI_EvadeButtonPressed. ReceiveUnPossess clears the timer.
fn dodge_meter_deplete(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, DodgeMeterDeplete).possessed;
            let p = first_possess(w, id, "@340", f, cur);
            dst!(w, id, DodgeMeterDeplete).possessed = p;
            delay(w, id, "@340", 0.1);
        }
        Call::Fired(Fired::Delay("@340")) => {
            let f = dst!(w, id, DodgeMeterDeplete).possessed;
            dst!(w, id, DodgeMeterDeplete).fighter = f;
            set_timer(w, id, "Demo", 2.0);
            deplete_demo(w, id);
        }
        Call::Fired(Fired::Timer("Demo")) => deplete_demo(w, id),
        Call::Unpossessed { .. } => clear_timer(w, id, "Demo"),
        _ => unhandled(w, id, c),
    }
}

fn deplete_demo(w: &mut World, id: u32) {
    let f = dst!(w, id, DodgeMeterDeplete).fighter;
    if callable(w, f) {
        pad(w, id).press(Btn::Dodge, None);
    }
}

/// TutorialController_Dodge_DodgeMeterReplenish. ReceivePossess: Delay 0.1 (@330 -> @15): EvadeCount = MaxEvadeCount
/// (@109), EvadeInvulnerabilityRechargeTime = SafeDivide(it, 2) (@180-244), UpdateEvadeValues (@293). No timer, no
/// ReceiveUnPossess.
fn dodge_meter_replenish(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, DodgeMeterReplenish).possessed;
            let p = first_possess(w, id, "@330", f, cur);
            dst!(w, id, DodgeMeterReplenish).possessed = p;
            delay(w, id, "@330", 0.1);
        }
        Call::Fired(Fired::Delay("@330")) => {
            let f = dst!(w, id, DodgeMeterReplenish).possessed;
            let (true, Some(f)) = (callable(w, f), f) else { return };
            pawn::set_evade_count(w, f, pawn::max_evade_count(w, f));
            let half = crate::fighter::moves::evade_recharge(w, f) / 2.0;
            pawn::set_evade_recharge_time(w, f, half);
            crate::fighter::control::update_evade_values(w, f);
        }
        Call::Unpossessed { .. } => {}
        _ => unhandled(w, id, c),
    }
}

/// TutorialController_Cooldowns_Basics. ReceivePossess (@410): Possessed Pawn, its Comp_Pawn_AbilityManager
/// AbilityCooldownUpdated bound to OnCooldownComplete (@527), OnCooldownComplete(0, false) now. OnCooldownComplete
/// (@731 -> @616): BPI_SpecialAttackPressed(4 Neutral) on every cooldown update, its arguments unread (the start of a
/// cooldown as well as its end). ReceiveUnPossess (@611 -> @15): K2_DestroyActor on every TetherProjectile_C (Wonder
/// Woman's lasso is one). Nothing unbinds.
fn cooldowns_basics(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            dst!(w, id, CooldownsBasics).fighter = Some(f);
            classes::bind(
                w,
                DelegateList::Pawn(f, PawnList::AbilityCooldownUpdated),
                TutorialObj::Demo(id),
                "cooldown",
            );
            cooldown_press(w, id);
        }
        Call::Delegate { handler: "cooldown", call: DelegateCall::AbilityCooldownUpdated { .. }, .. } => {
            cooldown_press(w, id);
        }
        Call::Unpossessed { .. } => destroy_projectiles_of(w, name!("TetherProjectile_C")),
        _ => unhandled(w, id, c),
    }
}

fn cooldown_press(w: &mut World, id: u32) {
    let f = dst!(w, id, CooldownsBasics).fighter;
    if callable(w, f) {
        pad(w, id).press(Btn::Special, Some(Dir::Neutral));
    }
}

/// TutorialController_Armor_Basics. ReceivePossess: Delay 0.1 (@1176 -> @410): DemoFighter, a bag at PunchingBagCenter,
/// the bag's HitByEnemy bound to DestroyAllSandwhich (@596), a looping 5.0 s timer to ArmorDemo (IgniteDemoTimer) and
/// one call. ArmorDemo (@1662): Delay 0.2 (-> @892): the bag's armour pack, DemoFighter's BPI_SpecialAttackPressed(1
/// Down); Delay 0.7 (@1116 -> @777): BPI_SpecialAttackPressed(1 Down) again. DestroyAllSandwhich: Delay 0.01 (@1717 ->
/// @1171 -> @15): K2_DestroyActor on every Item_Sandwich_C. ReceiveUnPossess (@1318): the bag destroyed, the timer
/// cleared, DestroyAllSandwhich, the bind removed (@1254).
fn armor_basics(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Demo(id);
    match c {
        Call::Possessed(f) => {
            dst!(w, id, ArmorBasics).w_set = true;
            let cur = dst!(w, id, ArmorBasics).possessed;
            let p = first_possess(w, id, "@1176", f, cur);
            dst!(w, id, ArmorBasics).possessed = p;
            delay(w, id, "@1176", 0.1);
        }
        Call::Fired(Fired::Delay("@1176")) => {
            let f = dst!(w, id, ArmorBasics).possessed;
            dst!(w, id, ArmorBasics).fighter = f;
            let bag = run::spawn_idle_bag(w, "PunchingBagCenter", 0.0, Some(0.0));
            dst!(w, id, ArmorBasics).bag = bag;
            if let Some(b) = bag {
                classes::bind(w, DelegateList::Pawn(b, PawnList::HitByEnemy), me, "sandwiches");
            }
            set_timer(w, id, "IgniteDemoTimer", 5.0);
            armor_basics_demo(w, id);
        }
        Call::Fired(Fired::Timer("IgniteDemoTimer")) => armor_basics_demo(w, id),
        Call::Fired(Fired::Delay("@1662")) => {
            let s = dst!(w, id, ArmorBasics);
            let (bag, f) = (s.bag, s.fighter);
            armor_pack(w, bag);
            if callable(w, f) {
                pad(w, id).press(Btn::Special, Some(Dir::Down));
            }
            delay(w, id, "@1116", 0.7);
        }
        Call::Fired(Fired::Delay("@1116")) => {
            let f = dst!(w, id, ArmorBasics).fighter;
            if callable(w, f) {
                pad(w, id).press(Btn::Special, Some(Dir::Down));
            }
        }
        Call::Delegate { handler: "sandwiches", .. } => sandwiches(w, id),
        Call::Fired(Fired::Delay("@1717")) => destroy_items_of(w, name!("Item_Sandwich_C")),
        Call::Unpossessed { .. } => {
            dst!(w, id, ArmorBasics).w_set = true;
            let bag = dst!(w, id, ArmorBasics).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                run::despawn(w, b);
            }
            clear_timer(w, id, "IgniteDemoTimer");
            sandwiches(w, id);
            if let Some(b) = bag {
                classes::unbind(w, DelegateList::Pawn(b, PawnList::HitByEnemy), me, Some("sandwiches"));
            }
        }
        _ => unhandled(w, id, c),
    }
}

fn armor_basics_demo(w: &mut World, id: u32) {
    delay(w, id, "@1662", 0.2);
}

/// DestroyAllSandwhich: Delay 0.01 (@1717), once `w` is set.
fn sandwiches(w: &mut World, id: u32) {
    if dst!(w, id, ArmorBasics).w_set {
        delay(w, id, "@1717", 0.01);
    }
}

/// TutorialController_Armor_Break. ReceivePossess: Delay 0.1 (@1451 -> @41): DemoFighter and its location, a bag at
/// PunchingBagClose and its location, a looping 6.0 s timer to ArmorDemo, (@15) DemoComplete = false and one call.
/// ArmorDemo (@1574): the bag's armour pack; Delay 0.8 (@1680 -> @1278): DemoFighter's BPI_NormalAttackPressed(1 Down);
/// Delay 4.0 (@1396 -> @466): unless DemoComplete the bag's CurrentDamage 0 (@600), both teleported back (@784, @1090:
/// rotation (0, 0, 0)). ReceiveUnPossess (@1506): the bag destroyed, the timer cleared, DemoComplete = true.
fn armor_break(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, ArmorBreak).possessed;
            let p = first_possess(w, id, "@1451", f, cur);
            dst!(w, id, ArmorBreak).possessed = p;
            delay(w, id, "@1451", 0.1);
        }
        Call::Fired(Fired::Delay("@1451")) => {
            let f = dst!(w, id, ArmorBreak).possessed;
            dst!(w, id, ArmorBreak).fighter = f;
            if let (true, Some(f)) = (callable(w, f), f) {
                let at = place_of(w, f);
                dst!(w, id, ArmorBreak).at = Some(at);
            }
            let bag = run::spawn_idle_bag(w, "PunchingBagClose", 0.0, Some(0.0));
            dst!(w, id, ArmorBreak).bag = bag;
            if let Some(b) = bag {
                let p = place_of(w, b);
                dst!(w, id, ArmorBreak).bag_at = Some(p);
            }
            set_timer(w, id, "ArmorDemoTimer", 6.0);
            dst!(w, id, ArmorBreak).complete = false;
            armor_break_demo(w, id);
        }
        Call::Fired(Fired::Timer("ArmorDemoTimer")) => armor_break_demo(w, id),
        Call::Fired(Fired::Delay("@1680")) => {
            let f = dst!(w, id, ArmorBreak).fighter;
            if callable(w, f) {
                pad(w, id).press(Btn::Attack, Some(Dir::Down));
            }
            delay(w, id, "@1396", 4.0);
        }
        Call::Fired(Fired::Delay("@1396")) => {
            let s = dst!(w, id, ArmorBreak);
            if s.complete {
                return;
            }
            let (f, bag, at, bag_at) = (s.fighter, s.bag, s.at, s.bag_at);
            if let (true, Some(b)) = (callable(w, bag), bag) {
                pawn::set_current_damage(w, b, 0.0);
            }
            teleport(w, bag, bag_at);
            teleport(w, f, at);
        }
        Call::Unpossessed { .. } => {
            let bag = dst!(w, id, ArmorBreak).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                run::despawn(w, b);
            }
            clear_timer(w, id, "ArmorDemoTimer");
            dst!(w, id, ArmorBreak).complete = true;
        }
        _ => unhandled(w, id, c),
    }
}

fn armor_break_demo(w: &mut World, id: u32) {
    let bag = dst!(w, id, ArmorBreak).bag;
    armor_pack(w, bag);
    delay(w, id, "@1680", 0.8);
}

/// TutorialController_DodgeJump (FirstLoop true). ReceivePossess (@1014 -> @15): Delay 0.2 (-> @70): the possessed
/// pawn's location, a looping 3.0 s timer to Attack Loop and one call. Attack Loop (@828): K2_GetPawn valid: Delay
/// FirstLoop ? 1.2 : 2.0 (@934 -> @556): FirstLoop = false; K2_GetPawn active: MoveX(1.0), EvadeButtonPressed(5); Delay
/// 0.1 (@773 -> @238): JumpPressed on that cast; Delay 0.1 (@274 -> @329): MoveX(0.0); Delay 0.7 (@370 -> @425):
/// K2_GetPawn teleported back. ReceiveUnPossess (@1019 -> @993) clears the timer.
fn dodge_jump(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, DodgeJump).possessed;
            let p = first_possess(w, id, "@15", f, cur);
            dst!(w, id, DodgeJump).possessed = p;
            delay(w, id, "@15", 0.2);
        }
        Call::Fired(Fired::Delay("@15")) => {
            let f = dst!(w, id, DodgeJump).possessed;
            if let (true, Some(f)) = (callable(w, f), f) {
                let at = place_of(w, f);
                dst!(w, id, DodgeJump).at = Some(at);
            }
            set_timer(w, id, "Attack Loop", 3.0);
            dodge_jump_loop(w, id);
        }
        Call::Fired(Fired::Timer("Attack Loop")) => dodge_jump_loop(w, id),
        Call::Fired(Fired::Delay("@934")) => {
            dst!(w, id, DodgeJump).first_loop = false;
            let p = get_pawn(w, id);
            if !active(w, p) {
                return;
            }
            dst!(w, id, DodgeJump).cast = p;
            let pd = pad(w, id);
            pd.move_x(1.0);
            pd.press(Btn::Dodge, None);
            delay(w, id, "@773", 0.1);
        }
        Call::Fired(Fired::Delay("@773")) => {
            let cast = dst!(w, id, DodgeJump).cast;
            if callable(w, cast) {
                pad(w, id).press(Btn::Jump, None);
            }
            delay(w, id, "@274", 0.1);
        }
        Call::Fired(Fired::Delay("@274")) => {
            let cast = dst!(w, id, DodgeJump).cast;
            if callable(w, cast) {
                pad(w, id).move_x(0.0);
            }
            delay(w, id, "@370", 0.7);
        }
        Call::Fired(Fired::Delay("@370")) => {
            let p = get_pawn(w, id);
            let at = dst!(w, id, DodgeJump).at;
            teleport(w, p, at);
        }
        Call::Unpossessed { .. } => clear_timer(w, id, "Attack Loop"),
        _ => unhandled(w, id, c),
    }
}

fn dodge_jump_loop(w: &mut World, id: u32) {
    let p = get_pawn(w, id);
    if !callable(w, p) {
        return;
    }
    let first = dst!(w, id, DodgeJump).first_loop;
    delay(w, id, "@934", if first { 1.2 } else { 2.0 });
}

/// TutorialController_AttackDecay_Variety. ReceivePossess (@2532): DemoFighter and its location; Delay 0.1 (@2703 ->
/// @15): a bag at PunchingBagClose moved by (0, 50, 0) (K2_SetActorLocation, no sweep, @184), the stored bag location
/// its place + another (0, 50, 0) (@346); Delay 1.5 (@373 -> @428): a looping 7.0 s timer to AttackDecayDemo and one
/// call. AttackDecayDemo (@2758): BPI_NormalAttackPressed(2 Forward); Delay 0.4 (@2876 -> @546): unless DemoComplete
/// BPI_NormalAttackPressed(1 Down) and BPI_NormalAttackReleased; Delay 0.35 (@819 -> @874): unless DemoComplete
/// BPI_NormalAttackPressed(1 Down); Delay 3.5 (@1031 -> @1086): unless DemoComplete, Delay 0.0 (@1417 -> @1472): unless
/// DemoComplete both teleported back, the bag's CurrentDamage 0, AddInvisibility on both; Delay 0.15 (@2031 -> @2086):
/// unless DemoComplete RemoveInvisibility on both. No decay inserts. ReceiveUnPossess (@2931): with the bag valid, the
/// bag destroyed, the timer cleared and DemoComplete = true.
fn attack_decay_variety(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let at = place_of(w, f);
            let s = dst!(w, id, AttackDecayVariety);
            s.fighter = Some(f);
            s.at = Some(at);
            delay(w, id, "@2703", 0.1);
        }
        Call::Fired(Fired::Delay("@2703")) => {
            let bag = run::spawn_idle_bag(w, "PunchingBagClose", 0.0, Some(0.0));
            dst!(w, id, AttackDecayVariety).bag = bag;
            if let Some(bag) = bag {
                let (x, y) = (w.fighter(bag).body.x, crate::fighter::actor_y(w, bag));
                crate::fighter::moves::recall_move(w, bag, x - 50.0, y);
                crate::combat::phases::root_moved(w, bag);
                let (bx, by) = (w.fighter(bag).body.x, crate::fighter::actor_y(w, bag));
                dst!(w, id, AttackDecayVariety).bag_at = Some(Place { x: bx - 50.0, y: by });
            }
            delay(w, id, "@373", 1.5);
        }
        Call::Fired(Fired::Delay("@373")) => {
            set_timer(w, id, "AttackDecayDemoTimer", 7.0);
            variety_demo(w, id);
        }
        Call::Fired(Fired::Timer("AttackDecayDemoTimer")) => variety_demo(w, id),
        Call::Fired(Fired::Delay("@2876")) => {
            let s = dst!(w, id, AttackDecayVariety);
            if s.complete {
                return;
            }
            let f = s.fighter;
            if callable(w, f) {
                let pd = pad(w, id);
                pd.press(Btn::Attack, Some(Dir::Down));
                pd.release(Btn::Attack);
            }
            delay(w, id, "@819", 0.35);
        }
        Call::Fired(Fired::Delay("@819")) => {
            let s = dst!(w, id, AttackDecayVariety);
            if s.complete {
                return;
            }
            let f = s.fighter;
            if callable(w, f) {
                pad(w, id).press(Btn::Attack, Some(Dir::Down));
            }
            delay(w, id, "@1031", 3.5);
        }
        Call::Fired(Fired::Delay("@1031")) => {
            if dst!(w, id, AttackDecayVariety).complete {
                return;
            }
            delay(w, id, "@1417", 0.0);
        }
        Call::Fired(Fired::Delay("@1417")) => {
            let s = dst!(w, id, AttackDecayVariety);
            if s.complete {
                return;
            }
            let (f, bag, at, bag_at) = (s.fighter, s.bag, s.at, s.bag_at);
            teleport(w, f, at);
            teleport(w, bag, bag_at);
            if let (true, Some(b)) = (callable(w, bag), bag) {
                pawn::set_current_damage(w, b, 0.0);
            }
            if let (true, Some(f)) = (callable(w, f), f) {
                crate::fighter::anim::add_invisibility(w, f);
            }
            if let (true, Some(b)) = (callable(w, bag), bag) {
                crate::fighter::anim::add_invisibility(w, b);
            }
            delay(w, id, "@2031", 0.15);
        }
        Call::Fired(Fired::Delay("@2031")) => {
            let s = dst!(w, id, AttackDecayVariety);
            if s.complete {
                return;
            }
            let (f, bag) = (s.fighter, s.bag);
            if let (true, Some(f)) = (callable(w, f), f) {
                crate::fighter::anim::remove_invisibility(w, f);
            }
            if let (true, Some(b)) = (callable(w, bag), bag) {
                crate::fighter::anim::remove_invisibility(w, b);
            }
        }
        Call::Unpossessed { .. } => {
            let bag = dst!(w, id, AttackDecayVariety).bag;
            let (true, Some(b)) = (callable(w, bag), bag) else { return };
            run::despawn(w, b);
            clear_timer(w, id, "AttackDecayDemoTimer");
            dst!(w, id, AttackDecayVariety).complete = true;
        }
        _ => unhandled(w, id, c),
    }
}

fn variety_demo(w: &mut World, id: u32) {
    let f = dst!(w, id, AttackDecayVariety).fighter;
    if callable(w, f) {
        pad(w, id).press(Btn::Attack, Some(Dir::Fwd));
    }
    delay(w, id, "@2876", 0.4);
}

// ---- the minigames (`TutorialMinigame_Parent`, `minigame.rs`)

/// A call on one of this family's minigame actors (`TutorialActor::minigames`, by id).
pub fn minigame(w: &mut World, id: u32, c: Call<'_>) {
    let kind = match &minigame_of(w, id).expect("a live minigame").state {
        MinigameState::DodgeBasics(_) => 0,
        MinigameState::DodgeRefill(_) => 1,
        _ => 2,
    };
    match kind {
        0 => dodge_basics_minigame(w, id, c),
        1 => dodge_refill(w, id, c),
        _ => armor_break_minigame(w, id, c),
    }
}

fn unhandled_minigame(w: &World, id: u32, c: Call<'_>) -> ! {
    panic!("tutorial: {} has no handler for {c:?}", minigame_of(w, id).map_or("a destroyed minigame", |m| m.cls));
}

fn mdelay(w: &mut World, id: u32, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Minigame(id), node, s);
}

/// The managed pawn (`host.managed`): TutorialManagedPawn.
fn managed(w: &World) -> Option<FighterId> {
    w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).and_then(|a| a.managed)
}

/// EnemyMinigame's SpawnedEnemies and its complete flag, whichever class holds them.
fn enemy_state(w: &mut World, id: u32) -> (&mut Vec<FighterId>, &mut bool) {
    match &mut minigame_mut(w, id).expect("a live minigame").state {
        MinigameState::DodgeBasics(s) => (&mut s.enemies, &mut s.game_over),
        MinigameState::ArmorBreak(s) => (&mut s.enemies, &mut s.game_over),
        _ => unreachable!("not an enemy minigame"),
    }
}

/// EnemyMinigame `spawn`: the Wonder Woman at MakeTransform((0, -400, 0), yaw 180) with FacingLeft written before
/// FinishSpawningActor, SpawnInvulnerabilityTime 0, PlayerDataReadyServer(1), the team of player data 1 and an
/// Idle_AIController spawned at the origin that possesses her; the newest enemy at index 0. `bind` adds the class's own
/// binds on the new pawn in its place in the order.
/// Unverified: the spawn's Z 0 is the actor place as passed (the class's SpawnCollisionHandlingMethod is not read);
/// the pawn's movement takes it from there.
fn spawn_enemy_pawn(w: &mut World, id: u32, bind: impl FnOnce(&mut World, FighterId)) {
    let team = run::data_team(w, 1);
    let f = run::spawn_fighter(w, name!("wonderwoman"), 400.0, 0.0, true, 1, team);
    enemy_state(w, id).0.insert(0, f);
    pawn::set_spawn_invulnerability_time(w, f, 0.0);
    bind(w, f);
    run::possess_bot(w, f, BotController::Idle);
}

/// `first`: SpawnedEnemies[0] while it is callable.
fn first_enemy(w: &mut World, id: u32) -> Option<FighterId> {
    let f = enemy_state(w, id).0.first().copied();
    if callable(w, f) { f } else { None }
}

/// The teardown of each valid enemy (index 0 kept or not): its PlayerState destroyed, the controller's SetLifeSpan(0),
/// its BrainComponent destroyed, UnPossess, K2_DestroyActor (the TutorialActor's own teardown, despawn_pawn).
fn teardown(w: &mut World, id: u32, keep_first: bool) {
    let list = enemy_state(w, id).0.clone();
    for (i, f) in list.into_iter().enumerate() {
        if keep_first && i == 0 {
            continue;
        }
        if callable(w, Some(f)) {
            pawns::despawn_pawn(w, f, true);
        }
    }
}

/// The step delegates of an EnemyMinigame: SetDodgingMinigameComplete / SetArmorMinigameComplete (TutorialStepComplete)
/// and CleanUpSpawnedTutorialEnemies (TutorialStepCleanup): every enemy torn down, then K2_DestroyActor on itself.
fn enemy_step(w: &mut World, id: u32, handler: &str) {
    match handler {
        "onStepComplete" => *enemy_state(w, id).1 = true,
        "onStepCleanup" => {
            teardown(w, id, false);
            minigame::destroy(w, id);
        }
        _ => unreachable!("an EnemyMinigame handler"),
    }
}

/// TutorialMinigame_DodgeBasics. ReceiveBeginPlay (@4265): GetPlayerPawn(0)'s EvadeInvulnerabilityRechargeTime = 0.5
/// (@4376) and its EvadedAttack bound to On Evaded Attack (@4444); the step delegates (@2673, @2737); then (@2853)
/// unless DodgingMinigameComplete the spawn: the enemy's AIAttackLoopTimer cleared (@3215), its OnExitedArena bound
/// to MinigameSpawnEnemy (@3313); Delay 1.0 (@4155 -> @1228): DownAttackToDodge() and a looping 4.0 s timer of it
/// (AIAttackLoopTimer); Delay 1.0 (@4210 -> @222): every enemy but [0] torn down.
/// DownAttackToDodge (@4857): [0] IsMovingOnGround: BPI_NormalAttackPressed(1 Down) (@4764), Delay 2.0 (@4802 ->
/// @130), [0]'s BPI_NormalAttackReleased: a 2 s charged down attack.
/// On Evaded Attack: the TutorialActor's BPI_HandleTutorialEvent(UniqueCharacterMechanic5) called directly (@90).
fn dodge_basics_minigame(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Minigame(id);
    match c {
        Call::BeginPlay => {
            minigame::bind_steps(w, id, true, true);
            let m = managed(w);
            if let (true, Some(m)) = (callable(w, m), m) {
                pawn::set_evade_recharge_time(w, m, 0.5);
                classes::bind(w, DelegateList::Pawn(m, PawnList::EvadedAttack), me, "evaded");
            }
            dodge_spawn_enemy(w, id);
        }
        Call::Delegate { handler: "onStepComplete" | "onStepCleanup", .. } => {
            let Call::Delegate { handler, .. } = c else { unreachable!() };
            enemy_step(w, id, handler);
        }
        Call::Delegate { handler: "respawn", .. } => dodge_spawn_enemy(w, id),
        Call::Delegate { handler: "evaded", .. } => minigame::handle_event(w, TutorialEvent::UniqueCharacterMechanic5),
        Call::Fired(Fired::Delay("@4155")) => {
            down_attack_to_dodge(w, id);
            latent::set_timer(w, me, "AIAttackLoopTimer", 4.0, true);
        }
        Call::Fired(Fired::Timer("AIAttackLoopTimer")) => down_attack_to_dodge(w, id),
        Call::Fired(Fired::Delay("@4210")) => teardown(w, id, true),
        Call::Fired(Fired::Delay("@4802")) => {
            if let Some(g) = first_enemy(w, id) {
                pawn::bot_release(w, g, BotBtn::Attack);
            }
        }
        Call::Destroyed => {}
        _ => unhandled_minigame(w, id, c),
    }
}

/// `spawnEnemy` of DodgeBasics.
fn dodge_spawn_enemy(w: &mut World, id: u32) {
    if *enemy_state(w, id).1 {
        return;
    }
    let me = TutorialObj::Minigame(id);
    spawn_enemy_pawn(w, id, |w, f| {
        latent::clear_timer(w, me, "AIAttackLoopTimer");
        classes::bind(w, DelegateList::Pawn(f, PawnList::ExitedArena), me, "respawn");
    });
    mdelay(w, id, "@4155", 1.0);
    mdelay(w, id, "@4210", 1.0);
}

fn down_attack_to_dodge(w: &mut World, id: u32) {
    let Some(f) = first_enemy(w, id) else { return };
    if crate::world::flow::movement_mode_byte(w, f) != 1 {
        return;
    }
    pawn::bot_press(w, f, BotBtn::Attack, Some(Dir::Down), false);
    mdelay(w, id, "@4802", 2.0);
}

/// TutorialMinigame_Dodge_Refill. ReceiveBeginPlay (@127): GetPlayerPawn(0) as DemoFighter, its EvadeCount =
/// MaxEvadeCount (@51, the meter spent) and UpdateEvadeValues (@10). The task's DodgeMeterFull comes from the meter's
/// own recharge (BaseFighter ReplenishDodgeMeter @311).
fn dodge_refill(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::BeginPlay => {
            let f = managed(w);
            let (true, Some(f)) = (callable(w, f), f) else { return };
            pawn::set_evade_count(w, f, pawn::max_evade_count(w, f));
            crate::fighter::control::update_evade_values(w, f);
        }
        Call::Destroyed => {}
        _ => unhandled_minigame(w, id, c),
    }
}

/// TutorialMinigame_ArmorBreak. ReceiveBeginPlay (@4831): the step delegates (@2915, @2979), then MinigameSpawnEnemy's
/// body (@3095, no complete test): the enemy's OnExitedArena bound to MinigameSpawnEnemy (@3614) and its
/// Comp_Pawn_DamageManager HitByEnemy to ArmorBroken (@3929); Delay 0.2 (@15 -> @70): [0] gets
/// SpawnBuffActor(Buff_ArmorPack_C, [0], [0], 1, 9999999.0, true, false) (@125); Delay 1.0 (@4776 -> @232): every
/// enemy but [0] torn down.
/// ArmorBroken (@5103): [0]'s HitByEnemy unbound (@5232); Delay 0.75 (@5048 -> @1238): PS_WonderWoman_TutorialSpawn at
/// [0], [0] K2_DestroyActor (@1467); Delay 1.0 (@1503 -> @1558): MinigameSpawnEnemy unless ArmorMinigameComplete.
/// Any hit runs it, not only one that breaks the armour: every hit replaces her with a fresh armoured one 1.75 s later.
fn armor_break_minigame(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Minigame(id);
    match c {
        Call::BeginPlay => {
            minigame::bind_steps(w, id, true, true);
            armor_spawn_enemy(w, id);
        }
        Call::Delegate { handler: "onStepComplete" | "onStepCleanup", .. } => {
            let Call::Delegate { handler, .. } = c else { unreachable!() };
            enemy_step(w, id, handler);
        }
        Call::Delegate { handler: "respawn", .. } => armor_spawn_enemy(w, id),
        Call::Delegate { handler: "armorBroken", .. } => {
            let f = enemy_state(w, id).0.first().copied();
            if let Some(f) = f {
                classes::unbind(w, DelegateList::Pawn(f, PawnList::HitByEnemy), me, Some("armorBroken"));
            }
            mdelay(w, id, "@5048", 0.75);
        }
        Call::Fired(Fired::Delay("@15")) => {
            if let Some(f) = first_enemy(w, id) {
                spawn_buff(w, f, name!("Buff_ArmorPack_C"), 9999999.0, Some(1), Some(f), true, Some(f));
            }
        }
        Call::Fired(Fired::Delay("@4776")) => teardown(w, id, true),
        Call::Fired(Fired::Delay("@5048")) => {
            if let Some(g) = first_enemy(w, id) {
                run::despawn(w, g);
            }
            mdelay(w, id, "@1503", 1.0);
        }
        Call::Fired(Fired::Delay("@1503")) => {
            if !*enemy_state(w, id).1 {
                armor_spawn_enemy(w, id);
            }
        }
        Call::Destroyed => {}
        _ => unhandled_minigame(w, id, c),
    }
}

/// `spawnEnemy` of ArmorBreak.
fn armor_spawn_enemy(w: &mut World, id: u32) {
    let me = TutorialObj::Minigame(id);
    spawn_enemy_pawn(w, id, |w, f| {
        classes::bind(w, DelegateList::Pawn(f, PawnList::ExitedArena), me, "respawn");
        classes::bind(w, DelegateList::Pawn(f, PawnList::HitByEnemy), me, "armorBroken");
    });
    mdelay(w, id, "@15", 0.2);
    mdelay(w, id, "@4776", 1.0);
}
