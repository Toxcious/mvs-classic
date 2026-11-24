//! Taz's Tasty: Buff_ApplyTasty, Buff_Tasty, Buff_TastyOnHit (on a fighter and on
//! a spawned actor), Buff_TastyBlock (offsets of each ubergraph unless named), AnimNotify_C015ListenForTastySpawn, and
//! the lick heal: Buff_c015_ApplybuffOnOverlap with Pawn_C015 BPI_LickHealOnOverlap @8178, PlayLickAllyMontage
//! @17202 and HealAllies. The chicken Buff_Tasty spawns at its cap and the heal pickups it drops are `chicken`.
//! What gives the Tasty: Taz's hits' AppliedBuffs rows (Buff_ApplyTasty, on a fighter or a spawned actor), and his
//! burped projectile (`burp`: Buff_TastyOnHit on the projectile, TastyStacksFromBurpedProjectile stacks, Pawn
//! @17782). The actor buffs' extensions are `ElemExt` variants (`Copy`).
#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-keeping negated tests

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::BuffClass;
use crate::combat::buffs::{
    BuffExt, BuffRef, apply_healing, buff, buff_mut, buff_of, end_buff, inst_view, remove_stack, spawn_buff, stack_buff,
};
use crate::combat::elem::{ElemExt, actor_inst_def, elem_buff, elem_buff_mut, elem_of, spawn_actor_buff};
use crate::combat::hit::timer_ticks;
use crate::events::{Event, NotifyRef, TazHealKind, UiStateOp, UiStateParams};
use crate::fighter::common::{latent_frames, peers, timer_frames, warn_once};
use crate::fighter::control::{emit, pawn_prop, trigger_ability, update_ability_cooldown};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, is_attacking};
use crate::math::ue::size;
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks, NotifyHandler};
use crate::registry::props::{self, num, truthy};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal, BuffSignal, BuffSignalArg};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

use super::pawn::update_eat_level;
use super::state::{taz_mut, taz_of};

const TASTY: Name = name!("Buff_Tasty_C");
const CHICKEN: Name = name!("Buff_Chicken_C");
const SPEED_BOOST: Name = name!("Buff_TastySpeedBoost_C");

/// Buff_Tasty_C's extension: the Instigator (`None`: none, and then neither knockback is bound).
#[derive(State, Debug, Default)]
pub struct TastyExt {
    pub inst: Option<FighterId>,
}

/// Buff_ApplyTasty_C's extension on a fighter: none is written (the buff ends in its BeginPlay).
#[derive(State, Debug, Default)]
pub struct ApplyTastyExt {}

/// Buff_TastyOnHit_C's extension on a fighter: the Instigator and the DoOnce still open.
#[derive(State, Debug, Default)]
pub struct TastyOnHitExt {
    pub inst: Option<FighterId>,
    pub open: bool,
}

/// Buff_TastyBlock_C's extension: the frames to the Delay(0.0)'s end (`due`, 0 once run) and the buff's duration.
#[derive(State, Debug, Default)]
pub struct TastyBlockExt {
    pub due: Real,
    pub duration: Real,
}

/// Buff_c015_ApplybuffOnOverlap_C's extension (the lick).
#[derive(State, Debug, Default)]
pub struct LickExt {
    /// The Instigator.
    pub inst: Option<FighterId>,
    /// Frames to the BeginPlay Delay's end, 0 once run.
    pub setup: Real,
    /// The overlap events on.
    pub on: bool,
    /// CanApply?.
    pub can_apply: bool,
    /// Team Index.
    pub team: i32,
    /// The ids of the fighters CheckForPlayers overlapped last frame.
    pub inside: Vec<i32>,
    /// Timer frames to AllowApplication, 0: none.
    pub allow: Real,
}

/// Buff_ApplyTasty_C's extension on a spawned actor: none is written (the buff ends in its BeginPlay).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct ApplyTastyActor {}

/// Buff_Tasty_C's extension on a spawned actor: the Instigator's id (-1: none).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct TastyActor {
    pub inst: i32,
}

/// Buff_TastyOnHit_C's extension on a spawned actor: the Instigator's id (-1: none) and the DoOnce still open.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct TastyOnHitActor {
    pub inst: i32,
    pub open: bool,
}

/// Comp_Actor_Destructible.GetAllies: the fighters of the owner's team but the owner, in player order.
// Deviation: the build's Allies drops a knocked-out ally until its respawn (OnPlayerKnockedOut -> RemoveItem @2338),
// is players only and in join order (`common::allies`); here a knocked-out ally (and a minion of the team) is counted
// and the order is the player order, as the TS port has it (the taz-taz trace moves at tick 348 with the build's list).
fn allies_of(w: &World, f: FighterId) -> Vec<FighterId> {
    let team = w.fighter(f).team.team;
    peers(w).iter().copied().filter(|&o| o != f && w.fighter(o).team.team == team).collect()
}

/// An actor buff's Instigator, kept as the fighter's id (`byId`).
fn by_id(w: &World, id: i32) -> Option<FighterId> {
    if id < 0 {
        return None;
    }
    peers(w).iter().copied().find(|&o| w.fighter(o).id == id)
}

/// A sphere of radius r at (x, y) against a fighter's root capsule (a touch is no overlap).
fn sphere_overlaps(w: &World, o: FighterId, x: Real, y: Real, r: Real) -> bool {
    let (half, rad, ay) = (capsule_half(w, o), cap_radius(w, o), actor_y(w, o));
    let seg = max(half - rad, 0.0);
    let cy = max(ay - seg, min(y, ay + seg));
    let (gx, gy) = (x - w.fighter(o).body.x, y - cy);
    (gx * gx + gy * gy).sqrt() < r + rad
}

// ---- Pawn_C015

/// BPI_UpdateTastyFighter(Fighter, Add) @8472 -> @8404 (authority) -> UpdateTastySpeedBoost(Fighter, Add): for the
/// pawn and each of its allies, a live Buff_TastySpeedBoost is told the target (UpdateTargetServer); Add true spawns
/// one where there is none. Buff_Tasty's EndPlay calls it with false, and no graph of the build calls it with true
/// (the class is dormant), so no speed boost exists and the call does nothing.
pub fn update_tasty_fighter(w: &mut World, t: FighterId, _fighter: FighterId, add: bool) {
    if add {
        warn_once(
            w,
            name!("Pawn_C015 UpdateTastySpeedBoost(true): Buff_TastySpeedBoost is not ported (chars/taz/tasty.rs)"),
        );
        return;
    }
    let mut list = vec![t];
    list.extend(allies_of(w, t));
    for o in list {
        if buff_of(w, o, SPEED_BOOST).is_some() {
            warn_once(w, name!("Buff_TastySpeedBoost_C UpdateTargetServer: not ported (chars/taz/tasty.rs)"));
        }
    }
}

/// HealAllies (the function, @0-1385): each ally within DistanceForHealing (500, Vector_Distance of the actor
/// locations) with a Comp_Actor_Damage: authority ApplyDamage(-|CurrentDamage >= HealPerChewyAlly ? HealPerChewyAlly
/// : CurrentDamage|) @763-1085 (ApplyDamage of 0 is no call: UGameplayStatics::ApplyDamage needs a damage other than
/// 0) and MulticastHealingVFX(ally) @1231 for every ally in reach, healed or not (PS_Heal on the ally's mesh, x1.25,
/// @16526: the TazHealFx event); then UpdateEatLevelServer(LocalEatGained, true, true) @1285 with the local never
/// written (0).
pub fn heal_allies(w: &mut World, f: FighterId) {
    let dist = pawn_prop(w, f, "DistanceForHealing").unwrap_or(500.0);
    let per = pawn_prop(w, f, "HealPerChewyAlly").unwrap_or(5.0);
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    for o in allies_of(w, f) {
        let d = size([0.0, w.fighter(o).body.x - fx, actor_y(w, o) - fy]);
        if !(d <= dist) {
            continue;
        }
        let dmg = w.fighter(o).stats.damage;
        let h = (if dmg >= per { per } else { dmg }).abs();
        if h != 0.0 {
            apply_healing(w, o, h);
        }
        emit(w, o, |fighter| Event::TazHealFx { fighter, kind: TazHealKind::Lick }); // @1231
    }
    update_eat_level(w, f, 0.0, true, true);
}

/// BPI_LickHealOnOverlap @8178: authority, Comp_Pawn_BranchManager.IsAttacking false -> PlayLickAllyMontage @8292,
/// which (@17202 -> @15) is BPI_HealAllies() alone (@13391 -> HealAllies): no montage is played.
fn lick_heal_on_overlap(w: &mut World, t: FighterId) {
    if is_attacking(w, t) {
        return;
    }
    heal_allies(w, t);
}

/// Comp_Pawn_DamageManager.KnockedBack of the Instigator of a Buff_Tasty (BeginPlay @124-332 binds it to
/// OnInstigatorKnockedBack -> @2880 RemoveStack(1)): every live Tasty whose Instigator is this pawn loses a stack.
/// Unverified: taken at the pawn's own KnockedBack hook; its order against the other handlers of the broadcast (the
/// owner's own bindings, Base_AnimBP's) is not read. A Tasty on a spawned actor is not reached.
fn knocked_back(w: &mut World, t: FighterId, _bkb: Real, _dx: Real, _dy: Real, _by: Option<ActorRef>) {
    for o in peers(w).to_vec() {
        let list: Vec<BuffRef> =
            w.fighter(o).buffs.buffs.iter().map(|b| BuffRef { fighter: o, serial: b.serial }).collect();
        for r in list {
            let hit = buff(w, r).is_some_and(|b| {
                b.cls == TASTY
                    && b.frames > 0.0
                    && matches!(b.ext, BuffExt::Tasty(TastyExt { inst: Some(i) }) if i == t)
            });
            if hit {
                remove_stack(w, o, r, 1);
            }
        }
    }
}

/// The hooks of this file in Taz's CharHooks.
pub fn hooks(h: &mut CharHooks) {
    h.knocked_back = Some(knocked_back);
}

// ---- Buff_Tasty

/// MulticastUI(Count) @881 (off a dedicated server, the owner's UI broker found): Count other than 0 ->
/// AddStateForPlayer {id Tasty, StateType 1, DisplayType 0, CurrentValue Count, MaxValue MaxStacks, Timer -1,
/// MI_State_Smell, Active, no pop text, AutoRemoveTime -1, no sub icon} @1193-1518; 0 -> RemoveStateForPlayer(Tasty)
/// @1670.
fn tasty_ui(w: &mut World, f: FighterId, max_stacks: Real, count: Real) {
    let op = if count == 0.0 {
        UiStateOp::Remove(name!("Tasty"))
    } else {
        UiStateOp::Add(UiStateParams {
            id: name!("Tasty"),
            state_type: 1,
            display_type: 0,
            current_value: count,
            max_value: max_stacks,
            timer: -1.0,
            icon_material: name!("MI_State_Smell"),
            active: true,
            pop_text: Name::NONE,
            auto_remove_time: -1.0,
            subicon: None,
        })
    };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

fn max_stacks(w: &World, b: BuffRef) -> Real {
    inst_view(w, b).num("MaxStacks").unwrap_or(1.0)
}

fn tasty_inst(w: &World, b: BuffRef) -> Option<FighterId> {
    match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::Tasty(e)) => e.inst,
        _ => None,
    }
}

// Buff_Tasty (MaxStacks 18, CanStack, StackDecayBuffer 4 / StackDecayRate .5: the parent's stack timer, spawned
// with a duration of 0: no life timer; BlockerBuffs Buff_Chicken / Buff_TastyBlock, IsDebuff,
// DestroyBuffWhenOwnerDefeated).
// - ReceiveBeginPlay @10 (authority): MulticastUI(Min(stacks, MaxStacks), true) @100; the Instigator's DamageManager
//   valid -> KnockedBack -> OnInstigatorKnockedBack (`knocked_back` above) @309; then the owner's ->
//   OnOwnerKnockedBack @558 (@2804: AddStack(1) while CurrentStackAmount < MaxStacks). No cap test here: a spawn at
//   18 stacks is no chicken until the next stack call.
// - AddStack @1727 / RemoveStack @2174 overrides: the parent's call, then (authority) MulticastUI(CurrentStackAmount)
//   @1780 and, with CurrentStackAmount >= MaxStacks @1804 and the owner's Comp_State_Manager valid,
//   SpawnBuffActor(Buff_Chicken_C, owner, Instigator, 0, ChickenDuration, true) @2081 (the chicken's BreakBuffs
//   destroy this buff inside the call). The parent's stack timer (TimedRemoveStack -> RemoveStack(1)) reaches the
//   override as well (`combat::buffs` tick_buffs calls `stacks` with added false).
//   Deviation: a RemoveStack that takes the count to 0 or below destroys the buff inside the parent's call
//   (`remove_stack`, which then does not call `stacks`); the build's override goes on on the pending-kill actor with
//   MulticastUI(CurrentStackAmount), an AddStateForPlayer of a count below 0 after EndPlay's removal (a cleanse of
//   fewer than 10 stacks). Not made: the multicast of a destroyed actor is not read.
// - ReceiveEndPlay @2198: MulticastUI(0) @2217; Trail and FullTastyMesh destroyed (presentation: not drawn);
//   authority: the Instigator cast to BPI_C015 -> BPI_UpdateTastyFighter(owner, false) @2752.
// - BPI On Cleanse @0: RemoveStack(10), BuffCleansed = true, DestroyOnCleanse = false (BuffCleansed is the cleanse's
//   pop text, not sent).
// - AddTracePointServer @627 / Check for Fighters: no caller in the build (the speed boost path, dormant).

fn tasty_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Tasty(TastyExt { inst: from });
    }
    let m = max_stacks(w, b);
    let s = buff(w, b).map_or(0.0, |x| x.stacks);
    tasty_ui(w, f, m, min(s, m));
}

fn tasty_stacks(w: &mut World, f: FighterId, b: BuffRef, _added: bool) -> bool {
    let m = max_stacks(w, b);
    let s = buff(w, b).map_or(0.0, |x| x.stacks);
    tasty_ui(w, f, m, s);
    if s >= m {
        let dur = inst_view(w, b).num("ChickenDuration").unwrap_or(0.0);
        let inst = tasty_inst(w, b);
        spawn_buff(w, f, CHICKEN, dur, Some(0), inst, true, None);
    }
    false
}

fn tasty_on(w: &mut World, f: FighterId, b: BuffRef, sig: BuffSignal, _a: &BuffSignalArg) -> bool {
    if sig != BuffSignal::KnockedBack || tasty_inst(w, b).is_none() {
        return false;
    }
    if buff(w, b).map_or(0.0, |x| x.stacks) < max_stacks(w, b) {
        stack_buff(w, f, b, 1.0);
    }
    false
}

fn tasty_cleanse(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    remove_stack(w, f, b, 10);
    false
}

fn tasty_end(w: &mut World, f: FighterId, b: BuffRef) {
    tasty_ui(w, f, 1.0, 0.0);
    if let Some(inst) = tasty_inst(w, b)
        && taz_of(w, inst).is_some()
    {
        update_tasty_fighter(w, inst, f, false);
    }
}

// On a spawned actor (Taz's hit on a projectile or an item: Buff_ApplyTasty below). The owner's UI broker is none
// (GetUIBrokerForFighter fails for an actor: no UI), the parent's stack timer is the actor path's
// (`combat::elem`). The AddStack override's cap test runs behind the parent's AddStack (another hit's
// SpawnBuffActor: the same-class branch of `spawn_actor_buff`), at the cap too.
// Deviation: the two knockback bindings are not made for an actor owner (its Comp_Pawn_DamageManager's KnockedBack
//   is not broadcast to buffs by the port). The chicken at the cap goes to the actor's state manager (refused there:
//   no actor-side entry).

fn tasty_actor_begin(w: &mut World, a: ActorRef, i: u32, from: Option<FighterId>) -> bool {
    let inst = from.map_or(-1, |o| w.fighter(o).id);
    if let Some(b) = elem_buff_mut(w, a, i) {
        b.ext = ElemExt::Tasty(TastyActor { inst });
    }
    false
}

fn tasty_actor_stacks(w: &mut World, a: ActorRef, i: u32, added: bool) -> bool {
    let Some((serial, stacks, ext)) = elem_buff(w, a, i).map(|b| (b.serial, b.stacks, b.ext)) else { return false };
    let (m, dur) = {
        let d = actor_inst_def(w, a, i);
        let d = d.as_deref();
        (d.and_then(|d| num(d, "MaxStacks")).unwrap_or(1.0), d.and_then(|d| num(d, "ChickenDuration")).unwrap_or(0.0))
    };
    if added && crate::real(stacks) >= m {
        let inst = match ext {
            ElemExt::Tasty(e) => e.inst,
            _ => -1,
        };
        let by = by_id(w, inst);
        spawn_actor_buff(w, a, CHICKEN, 0, dur, true, by);
    }
    let alive = crate::actors::world_actor::actor_elem(w, a)
        .and_then(Option::as_ref)
        .is_some_and(|st| st.buffs.iter().any(|b| b.serial == serial));
    !alive
}

// ---- Buff_ApplyTasty
// ReceiveBeginPlay @834 -> @10 (authority; the owner's Comp_State_Manager valid @144): CheckHasBuff(Buff_Tasty) @229;
// with no Tasty, or CurrentStackAmount - 1 > 0 @362-505: SpawnBuffActor(Buff_Tasty_C, owner, Instigator,
// HasBuff ? CurrentStackAmount - 1 : CurrentStackAmount, 0.0, true) @727; then K2_DestroyActor @815 in every case
// (one stack on a Tasty that is there adds nothing).

fn apply_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    let has = buff_of(w, f, TASTY).is_some();
    let s = buff(w, b).map_or(0.0, |x| x.stacks);
    if !has || s - 1.0 > 0.0 {
        let n = if has { s - 1.0 } else { s };
        spawn_buff(w, f, TASTY, 0.0, Some(to_int32(n)), from, true, None);
    }
    end_buff(w, f, b);
}

/// On a spawned actor: the same on its Comp_State_Manager.
fn apply_actor_begin(w: &mut World, a: ActorRef, i: u32, from: Option<FighterId>) -> bool {
    let has = elem_of(w, a, BuffClass::BuffTasty).is_some();
    let s = elem_buff(w, a, i).map_or(0, |b| b.stacks);
    if !has || s - 1 > 0 {
        spawn_actor_buff(w, a, TASTY, if has { s - 1 } else { s }, 0.0, true, from);
    }
    true
}

// ---- Buff_TastyOnHit
// ReceiveBeginPlay @355 (authority): the owner's Comp_Pawn_HitBoxHandler valid -> its HitEnemyPlayer ->
// OnHitEnemyPlayer @576-599. OnHitEnemyPlayer @641 -> a DoOnce @340 (open at first) @59: closed, then the hit
// player's Comp_State_Manager valid @150 -> SpawnBuffActor(Buff_Tasty_C, HitPlayer, Instigator, CurrentStackAmount,
// 0.0, true) @231 and K2_DestroyActor @15. With no state manager the buff stays, closed (a fighter always has one).
// On a spawned actor (`burp`'s burped projectile): the handler's HitEnemyPlayer is the actor's HitEnemyPlayer signal.
// On a fighter: its own handler's HitEnemyPlayer, the HitEnemy signal (no graph of the build spawns it on one).

fn on_hit_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::TastyOnHit(TastyOnHitExt { inst: from, open: true });
    }
}

fn on_hit_on(w: &mut World, _f: FighterId, b: BuffRef, sig: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(victim) = a.victim.filter(|_| sig == BuffSignal::HitEnemy) else { return false };
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::TastyOnHit(e) = &mut x.ext else { return false };
    if !e.open {
        return false;
    }
    e.open = false;
    let (inst, stacks) = (e.inst, x.stacks);
    spawn_buff(w, victim, TASTY, 0.0, Some(to_int32(stacks)), inst, true, None);
    true
}

fn on_hit_actor_begin(w: &mut World, a: ActorRef, i: u32, from: Option<FighterId>) -> bool {
    let inst = from.map_or(-1, |o| w.fighter(o).id);
    if let Some(b) = elem_buff_mut(w, a, i) {
        b.ext = ElemExt::TastyOnHit(TastyOnHitActor { inst, open: true });
    }
    false
}

fn on_hit_actor_on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, arg: Option<&ActorBuffArg>) -> bool {
    let Some(arg) = arg.filter(|_| s == ActorBuffSignal::HitEnemyPlayer) else { return false };
    let Some(b) = elem_buff_mut(w, a, i) else { return false };
    let ElemExt::TastyOnHit(e) = &mut b.ext else { return false };
    if !e.open {
        return false;
    }
    e.open = false;
    let (inst, stacks) = (e.inst, b.stacks);
    let by = by_id(w, inst);
    spawn_buff(w, arg.by, TASTY, 0.0, Some(stacks), by, true, None);
    true
}

// ---- Buff_TastyBlock
// Spawned by Buff_Chicken's EndPlay (`chicken`) with BlockDuration (8) and the owner as Instigator; BreakBuffs
// Buff_Tasty, and Buff_Tasty's BlockerBuffs name it: no Tasty while it lasts.
// ReceiveBeginPlay @319 -> @284 (authority) -> Delay(0.0) @15 -> @70: the owner BPI_IsDead -> K2_DestroyActor @229;
// else MulticastUI(BuffDuration) @260 -> @673 (off a dedicated server, the broker found): State Params.Timer = the
// duration @836, AddStateForPlayer(State Params) @965 (its defaults: id TastyBlock, StateType NewEnumerator2 = byte 0,
// DisplayType NewEnumerator3 = byte 3 (UI_StateType.json / UI_StateDisplayType.json Names), CurrentValue / MaxValue
// -1, MI_State_Smell, Active, AutoRemoveTime -1, sub icon MI_State_Block). ReceiveEndPlay @334 (off a dedicated
// server, the broker found): RemoveStateForPlayer(State Params.id) @609.
// Unverified: Delay(0.0) is taken to end in the buff's first tick (`combat::buffs` tick_buffs, as Buff_Cleanse's).

/// UI_StateType by its enumerator's full name (NewEnumerator<n> is not the byte).
fn state_type(s: &str) -> i32 {
    match s {
        "UI_StateType::NewEnumerator2" => 0,
        "UI_StateType::NewEnumerator0" => 1,
        "UI_StateType::NewEnumerator1" => 2,
        _ => 0,
    }
}

/// UI_StateDisplayType: NewEnumerator<n> is byte n (its Names).
fn display_type(s: Option<&str>) -> i32 {
    let s = s.unwrap_or("");
    let Some(i) = s.rfind("NewEnumerator") else { return 0 };
    let digits = &s[i + "NewEnumerator".len()..];
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    digits.parse().unwrap_or(0)
}

fn block_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, duration: Option<Real>) {
    let duration = duration.or_else(|| inst_view(w, b).num("BuffDuration")).unwrap_or(0.0);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::TastyBlock(TastyBlockExt { due: latent_frames(0.0), duration });
    }
}

fn block_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::TastyBlock(e) = &mut x.ext else { return false };
    if e.due <= 0.0 {
        return false;
    }
    e.due -= 1.0;
    if e.due > 0.0 {
        return false;
    }
    let duration = e.duration;
    if w.fighter(f).life.dead != 0.0 {
        return true;
    }
    let op = {
        let sp = inst_view(w, b).obj("State Params");
        let txt = |k: &str| sp.and_then(|p| props::text(p, k));
        let n = |k: &str, d: Real| sp.and_then(|p| num(p, k)).unwrap_or(d);
        UiStateOp::Add(UiStateParams {
            id: txt("id").map_or(name!("TastyBlock"), |s| props::name(w, s)),
            state_type: txt("StateType").map_or(0, state_type),
            display_type: display_type(txt("DisplayType")),
            current_value: n("CurrentValue", -1.0),
            max_value: n("MaxValue", -1.0),
            timer: duration,
            icon_material: txt("IconMaterial").map_or(Name::NONE, |s| props::name(w, s)),
            active: sp.and_then(|p| p.get("Active")).filter(|v| !v.is_null()).is_none_or(truthy),
            pop_text: txt("PopText").map_or(Name::NONE, |s| props::name(w, s)),
            auto_remove_time: n("AutoRemoveTime", -1.0),
            subicon: txt("Subicon").map(|s| props::name(w, s)),
        })
    };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
    false
}

fn block_end(w: &mut World, f: FighterId, b: BuffRef) {
    let id = inst_view(w, b)
        .obj("State Params")
        .and_then(|p| props::text(p, "id"))
        .map_or(name!("TastyBlock"), |s| props::name(w, s));
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(id));
}

// ---- Buff_c015_ApplybuffOnOverlap (the lick heal)
// Taz's own passive (Comp_Pawn_PassiveBuffManager: BuffSelf, 0 stacks, no duration; Instigator: Taz). "CanApply?"
// true and TriggerAbilityIndex 2 in its class defaults.
// - ReceiveBeginPlay @2212: Delay(1.0) @2222 -> @15: the owner's Comp_Actor_Destructible valid -> GetAllies @196; with
//   allies: CanApply? = true @348, the owner a BP_Fighter_Character -> CheckForPlayers (sphere, radius 200) attached
//   to its root @500, Team Index = the owner's TeamIndex @752; with none: CanApply? = false @802 and
//   CheckForPlayers.SetGenerateOverlapEvents(false) @813 for good; an owner that is no fighter: K2_DestroyActor @847.
// - CheckForPlayers' begin overlap @2277: the Instigator not attacking @2348, CanApply? @2432, the other actor's
//   CurrentDamage > 0, its TeamIndex = Team Index and not the owner @2544-2794 -> the Instigator's
//   BPI_LickHealOnOverlap @2903, the owner's Comp_Pawn_AbilityManager.TriggerAbilityActual(2) @3121, CanApply? = false
//   @3175, VisualsOff @3186, then @862: K2_SetTimerDelegate(AllowApplication, 16 s) @885.
// - AllowApplication @3769: CanApply? = true, CheckIfAllyIsOverlappingAtStart @3780 (@3528: with CanApply?, each ally
//   of GetAllies that CheckForPlayers overlaps while the Instigator is not attacking @1206-1412: BPI_LickHealOnOverlap
//   @1521, TriggerAbilityActual(2) @1739, CanApply? = false, the 16 s timer again @1804 -> @862, and the loop goes on:
//   no CurrentDamage or owner test there, no VisualsOff), then UpdateAbilityCooldownServer(2, false, 0, 1) @3975.
// - VisualsOn / VisualsOff: SpawnEmitterAttached(None, ...) @3343 / the Visuals destroyed: nothing is drawn.
// Unverified: the overlap events are taken from the Delay's end on, at the owner's root capsule each frame in the
//   buff's tick (the sphere is not attached before; its overlaps at the buff actor's own place, with the class default
//   CanApply? true and Team Index 0, ahead of the Delay are not read: native question). An ally's root capsule (Pawn)
//   alone begins the overlap.

fn ability(w: &World, b: BuffRef) -> i32 {
    inst_view(w, b).num("TriggerAbilityIndex").map_or(2, to_int32)
}

fn lick_ext(w: &mut World, b: BuffRef) -> Option<&mut LickExt> {
    match buff_mut(w, b).map(|x| &mut x.ext) {
        Some(BuffExt::Lick(e)) => Some(e),
        _ => None,
    }
}

/// `in_timer`: called from the timer pass (AllowApplication @862), where a timer set is counted from this frame
/// (`timer_frames`); from the overlap in the tick groups otherwise.
fn lick(w: &mut World, f: FighterId, b: BuffRef, in_timer: bool) {
    let inst = lick_ext(w, b).and_then(|e| e.inst).unwrap_or(f);
    if taz_of(w, inst).is_some() {
        lick_heal_on_overlap(w, inst);
    }
    let slot = ability(w, b);
    trigger_ability(w, f, slot);
    let allow = if in_timer { timer_ticks(Some(16.0)) } else { timer_frames(Some(16.0)) };
    if let Some(e) = lick_ext(w, b) {
        e.can_apply = false;
        e.allow = allow;
    }
}

fn lick_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    let can_apply = inst_view(w, b).get("CanApply?").filter(|v| !v.is_null()).is_none_or(truthy);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Lick(LickExt {
            inst: from,
            setup: latent_frames(1.0),
            on: false,
            can_apply,
            team: 0,
            inside: Vec::new(),
            allow: 0.0,
        });
    }
}

fn lick_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(setup) = lick_ext(w, b).map(|e| e.setup) else { return false };
    if setup > 0.0 {
        let left = setup - 1.0;
        let none = allies_of(w, f).is_empty();
        let team = w.fighter(f).team.team;
        let Some(e) = lick_ext(w, b) else { return false };
        e.setup = left;
        if left == 0.0 {
            if none {
                e.can_apply = false;
                return false;
            }
            e.can_apply = true;
            e.on = true;
            e.team = team;
        }
    }
    if !lick_ext(w, b).is_some_and(|e| e.on) {
        return false;
    }
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let now: Vec<FighterId> =
        peers(w).iter().copied().filter(|&o| o != f && sphere_overlaps(w, o, fx, fy, 200.0)).collect();
    let inside = lick_ext(w, b).map(|e| std::mem::take(&mut e.inside)).unwrap_or_default();
    let inst = lick_ext(w, b).and_then(|e| e.inst).unwrap_or(f);
    for &o in &now {
        if inside.contains(&w.fighter(o).id) {
            continue;
        }
        let Some((can_apply, team)) = lick_ext(w, b).map(|e| (e.can_apply, e.team)) else { break };
        if is_attacking(w, inst) || !can_apply || !(w.fighter(o).stats.damage > 0.0) || w.fighter(o).team.team != team {
            continue;
        }
        lick(w, f, b, false);
    }
    let ids: Vec<i32> = now.iter().map(|&o| w.fighter(o).id).collect();
    if let Some(e) = lick_ext(w, b) {
        e.inside = ids;
    }
    false
}

fn lick_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = lick_ext(w, b) else { return false };
    if e.allow <= 0.0 {
        return false;
    }
    e.allow -= 1.0;
    if e.allow > 0.0 {
        return false;
    }
    e.can_apply = true;
    let inst = e.inst.unwrap_or(f);
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    for o in allies_of(w, f) {
        if sphere_overlaps(w, o, fx, fy, 200.0) && !is_attacking(w, inst) {
            lick(w, f, b, true);
        }
    }
    // UpdateAbilityCooldownServer(2, false, 0, 1): the slot's cooldown taken down by its whole length.
    let slot = ability(w, b);
    update_ability_cooldown(w, f, slot, false, 0.0, 1.0);
    false
}

// ---- AnimNotify_C015ListenForTastySpawn
// Received_NotifyBegin / Received_NotifyEnd @121: BPI_SetListenForTastyBuffGiver(true / false) on a BPI_C015 owner
// (Pawn_C015 @8307: authority, SpawningMeleeBuffActor = On @8337). The variable has no reader in the build (the melee
// Tasty path, Buff_TastyMelee / TastyMeleePickup, is dormant). On another fighter (a Disguise's copy of the montage)
// the cast fails: nothing.

fn listen_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = taz_mut(w, f) {
        s.spawning_melee_buff_actor = true;
    }
}

fn listen_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = taz_mut(w, f) {
        s.spawning_melee_buff_actor = false;
    }
}

/// This file's registry entries: the Tasty buffs (in registration order) and AnimNotify_C015ListenForTastySpawn_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut tasty = BuffEntry::new(
        BuffClass::BuffTasty,
        BuffHooks {
            begin: Some(tasty_begin),
            stacks: Some(tasty_stacks),
            on: Some(tasty_on),
            cleanse: Some(tasty_cleanse),
            end: Some(tasty_end),
            ..BuffHooks::default()
        },
    );
    tasty.actor = Some(ActorBuffHooks {
        begin: Some(tasty_actor_begin),
        stacks: Some(tasty_actor_stacks),
        ..ActorBuffHooks::default()
    });
    let mut apply =
        BuffEntry::new(BuffClass::BuffApplyTasty, BuffHooks { begin: Some(apply_begin), ..BuffHooks::default() });
    apply.actor = Some(ActorBuffHooks { begin: Some(apply_actor_begin), ..ActorBuffHooks::default() });
    let mut on_hit = BuffEntry::new(
        BuffClass::BuffTastyOnHit,
        BuffHooks { begin: Some(on_hit_begin), on: Some(on_hit_on), ..BuffHooks::default() },
    );
    on_hit.actor = Some(ActorBuffHooks {
        begin: Some(on_hit_actor_begin),
        on: Some(on_hit_actor_on),
        ..ActorBuffHooks::default()
    });
    let block = BuffEntry::new(
        BuffClass::BuffTastyBlock,
        BuffHooks { begin: Some(block_begin), tick: Some(block_tick), end: Some(block_end), ..BuffHooks::default() },
    );
    let lick = BuffEntry::new(
        BuffClass::BuffC015ApplybuffOnOverlap,
        BuffHooks { begin: Some(lick_begin), tick: Some(lick_tick), timer: Some(lick_timer), ..BuffHooks::default() },
    );
    for (cls, e) in [
        ("Buff_Tasty_C", tasty),
        ("Buff_ApplyTasty_C", apply),
        ("Buff_TastyOnHit_C", on_hit),
        ("Buff_TastyBlock_C", block),
        ("Buff_c015_ApplybuffOnOverlap_C", lick),
    ] {
        r.buffs.enter(cls, e)?;
    }
    r.notifies.enter(
        "AnimNotify_C015ListenForTastySpawn_C",
        NotifyHandler { begin: Some(listen_begin), end: Some(listen_end), ..NotifyHandler::default() },
    )?;
    Ok(())
}
