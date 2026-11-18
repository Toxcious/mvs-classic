//! AbilityRefundOnDebuffHit, AbilityRefundOnHit, BackToBack, JumpCount, CollateralDamage and its tracker.
//! Read: Buff_Perk_AbilityRefundOnDebuffHit, Buff_Perk_AbilityRefundOnHit, Buff_BackToBack, Buff_Perk_JumpCount,
//! Buff_CollateralDamage, Buff_CollateralDamageTracker; Comp_State_Manager CheckHasDebuff.
//! The data-only stat perks have no graph: their class defaults go through the folds and the damage passes.

use super::conds::has_debuff;
use super::hooks::{NO_ACTOR, NO_HOOKS};
use super::{PerkExt, def, enter, ext, ext_mut, flag, kind, num, set_ext, stacks};
use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::actor_owner;
use crate::combat::buffs::{buff_actor_light_hit, end_buff, set_buff_values, spawn_buff, update_states};
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::fighter::FighterId;
use crate::fighter::common::{buff_def, latent_frames, lookup_owner, peers};
use crate::fighter::control::update_ability_cooldown;
use crate::fighter::{actor_y, in_hitstun};
use crate::math::ue;
use crate::name;
use crate::registry::entries::{ActorBuffHooks, BuffHooks};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal, BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// Comp_Actor_Destructible.GetAllies: the other fighters of the owner's team, one waiting for its respawn left out
/// (the list the component keeps; its order the sim's, unverified). Players only: a minion is among the peers but
/// never in the list. Walked in place: its readers change no fighter's team, life or the peers.
fn allies(w: &World, f: FighterId) -> impl Iterator<Item = FighterId> + '_ {
    let team = w.fighter(f).team.team;
    peers(w).iter().copied().filter(move |&o| {
        let x = w.fighter(o);
        o != f && x.team.team == team && x.life.dead <= 0.0 && !x.life.is_minion
    })
}

fn refund_slots(w: &mut World, f: FighterId, secs: Real) {
    let n = w.fighter(f).abilities.abilities.len();
    for i in 0..n {
        let slot = i32::try_from(i).expect("an ability slot");
        update_ability_cooldown(w, f, slot, false, secs, 0.0);
    }
}

// ---------------------------------------------------------------- Buff_Perk_AbilityRefundOnDebuffHit
// BeginPlay @1604 (authority): the owner's Comp_Pawn_HitBoxHandler valid -> binds HitEnemyPlayer to OnEnemyHit
// (the HitEnemy signal); EndPlay @1890 unbinds. OnEnemyHit @2185: the hit a BP_FighterCharacter with a valid
// Comp_State_Manager whose CheckHasDebuff @2401 is true, AND the buff owner's Comp_Pawn_AbilityManager valid @2546;
// AbilityRefundSeconds = SelectFloat(Stacked 0.5, Unstacked 0.25, CurrentStackAmount >= 2) @2594-2683; then every
// slot of the owner's Abilities @1553 -> @967-1404, then GetAllies @1409-1480 and every slot of each ally with an
// AbilityManager @15-934: UpdateAbilityCooldownServer(slot, false, seconds, 0.0). No distance, slot, dedup or
// cooldown of its own.
// The hit's own on-hit buffs are on the victim by then (the hit's buffs ahead of HitEnemyPlayer @22770).
// Actor side (GiveBuffToSpawnedActors): the copy binds the actor's handler, and the callback's AND reads the actor's
// own Comp_Pawn_AbilityManager, which no SpawnedActorParent child of the data has (only the pawns carry one):
// nothing.
fn refund_on_debuff_hit(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(victim) = a.victim else { return false };
    if s != BuffSignal::HitEnemy || !has_debuff(w, victim) {
        return false;
    }
    let d = def(w, b);
    let k = if stacks(w, b) >= 2.0 { "StackedAbilityRefundSeconds" } else { "UnstackedAbilityRefundSeconds" };
    let secs = num(d, k);
    refund_slots(w, f, secs);
    // (the list as the walk began, in a list of the World's pool: the refund writes the fighters)
    let mut os = crate::world::pawns::take_fighters(w);
    os.extend(allies(w, f));
    for &o in &os {
        refund_slots(w, o, secs);
    }
    crate::world::pawns::put_fighters(w, os);
    false
}

// ---------------------------------------------------------------- Buff_Perk_AbilityRefundOnHit
// AbilityRefundPercent 0.5, GiveBuffToSpawnedActors, no stacking. ReceiveBeginPlay @516 -> @10 (authority): the
// owner cast to SpawnedActorParent @64 -> its Comp_Pawn_HitBoxHandler's HitEnemyPlayer bound to OnEnemyHit
// @194-217; EndPlay @531 -> @263 unbinds the same way. On a fighter the cast fails: the fighter's own copy binds
// nothing and does nothing. The actor's copy (from GrantBuffsFromInstigator, a SpawnedActorParent's BeginPlay) is
// bound for its life.
// OnEnemyHit(HitPlayer, Actor) @555: HitEnemyPlayer's second argument is the handler's GetOwner() (@22770), the
// actor; Actor.GetOwner() cast to BaseFighter @597-662 (the actor's owner), its BPI_GetCurrentAttack @676
// AbilitySlot (the attack that runs now, not the one that spawned the actor) -> Comp_Pawn_AbilityManager
// UpdateAbilityCooldownServer(slot, false, 0.0, AbilityRefundPercent) @772: half the slot's full cooldown off. No
// target, slot or cooldown test of its own.
// Actor.GetOwner() is the actor's owner: a reflect sets the projectile's Owner and Instigator to the reflector
// (Comp_ReflectProjectile ReflectProjectile @1385, @2105-2277).
// Unverified: a thrown item's Owner is taken as its last thrower (ItemActorParent PickUp's SetOwner is not read).
fn refund_on_hit_actor(w: &mut World, a: ActorRef, _i: u32, s: ActorBuffSignal, _by: Option<&ActorBuffArg>) -> bool {
    if s != ActorBuffSignal::HitEnemyPlayer {
        return false;
    }
    let Some(owner) = actor_owner(w, a) else { return false };
    let own = lookup_owner(w, a);
    let pct = num(buff_def(w, ActorRef::Fighter(own), name!("Buff_Perk_AbilityRefundOnHit_C")), "AbilityRefundPercent");
    let slot = w.fighter(owner).abilities.attack_slot;
    update_ability_cooldown(w, owner, slot, false, 0.0, pct);
    false
}

// ---------------------------------------------------------------- Buff_BackToBack
// No graph but TakenDamageModCondition (@0-743): BuffOwner's Comp_Actor_Destructible valid, then true at the first
// of its GetAllies with Vector_Distance(ally location, owner location) <= MaxEffectDistance (650) @384-627, else
// false. Its mods are the data's: TakenDamageMods DamageMod 1 Multiply, per stack -0.06 Add (0.94 / 0.88).
// UE locations: (depth, -x, y); every fighter is on the plane, the depth difference 0.
fn back_to_back(
    w: &mut World,
    f: FighterId,
    b: BuffRef,
    _att: Option<ActorRef>,
    _boxed: bool,
    _h: &HitPacket,
    _by: Option<FighterId>,
) -> bool {
    let max = num(def(w, b), "MaxEffectDistance");
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    allies(w, f).any(|o| ue::size2d([w.fighter(o).body.x - x, actor_y(w, o) - y]) <= max)
}

// ---------------------------------------------------------------- Buff_Perk_JumpCount
// BeginPlay @10 (authority): the owner's HitBoxHandler valid -> binds HitEnemyPlayer to OnHitEnemyPlayer.
// OnHitEnemyPlayer @1297: TripleJumpOn -> nothing; else the owner's CharacterMovementComponent valid and not
// IsMovingOnGround -> MulticastUpdateState(true).
// MulticastUpdateState @767: a value other than TripleJumpOn -> TripleJumpOn = it; JumpMods.JumpCountMod =
// SelectInt(1, 0, TripleJumpOn && !DoesImplementInterface(owner, BPI_C017)) @1010; the owner's Comp_State_Manager
// UpdatePlayerStatus(null) @1231 (the jump fold's MaxJumpNumber + JumpCountMod).
// ReceiveTick @1273 -> @411 (authority): MoreThan1Stack -> nothing; else TripleJumpOn && IsMovingOnGround ->
// MulticastUpdateState(false).
// AddStack @308: the parent's AddStack first, then (authority) MoreThan1Stack already -> nothing; else
// MoreThan1Stack = true and MulticastUpdateState(true), whatever the count became: a team mate's copy (the ally
// pass's second spawn of the class) leaves the extra jump on for the buff's life.
// BPI_C017: Iron Giant's pawn (the character key irongiant).
// Unverified: the tick is the buff's tick in the owner's tick group, as the port's other buff ReceiveTicks; the
// build's actor tick order against the pawn's CMC is not read.
fn triple_jump(w: &mut World, f: FighterId, b: BuffRef, on: bool) {
    let Some(PerkExt::JumpCount { on: was, .. }) = ext_mut(w, b) else { return };
    if *was == on {
        return;
    }
    *was = on;
    let mut mods = def(w, b).obj("JumpMods").cloned().unwrap_or_default();
    let giant = w.fighter(f).ch == name!("irongiant");
    mods.insert("JumpCountMod".into(), Value::Num(if on && !giant { 1.0 } else { 0.0 }));
    let mut v = Props::new();
    v.insert("JumpMods".into(), Value::Obj(mods));
    set_buff_values(w, b, &v);
    update_states(w, f);
}

fn jump_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    set_ext(w, b, PerkExt::JumpCount { on: false, more: false });
}

fn jump_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    let on = matches!(ext(w, b), Some(PerkExt::JumpCount { on: true, .. }));
    if s != BuffSignal::HitEnemy || on || w.fighter(f).body.on_floor {
        return false;
    }
    triple_jump(w, f, b, true);
    false
}

fn jump_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if matches!(ext(w, b), Some(PerkExt::JumpCount { on: true, more: false })) && w.fighter(f).body.on_floor {
        triple_jump(w, f, b, false);
    }
    false
}

fn jump_stacks(w: &mut World, f: FighterId, b: BuffRef, added: bool) -> bool {
    if !added {
        return false;
    }
    let Some(PerkExt::JumpCount { more, .. }) = ext_mut(w, b) else { return false };
    if *more {
        return false;
    }
    *more = true;
    triple_jump(w, f, b, true);
    false
}

// ---------------------------------------------------------------- Buff_CollateralDamage
// BeginPlay @10 (authority): the owner's HitBoxHandler valid -> binds HitEnemyPlayer to OnHit. OnHit @308: the hit
// actor's Comp_State_Manager valid -> its SpawnBuffActor(Buff_CollateralDamageTracker, the hit actor,
// GetInstigator(), CurrentStackAmount, 2.0, true, false) @473. No End unbind, no cooldown.
// GetInstigator: the Instigator the perk manager gave (`add_perks`: the owner, or the team mate whose perk it is).
// Actor side (GiveBuffToSpawnedActors): the copy on a spawned actor binds that actor's handler the same way
// (HitEnemyPlayer). GrantBuffsFromInstigator @819 passes the actor as the Instigator, which SpawnBuffActor
// @440-842 turns into its owner (the Instigator argument when it is a pawn, else its owner when that is one): the
// fighter that spawned the actor.
const TRACKER: crate::names::Name = name!("Buff_CollateralDamageTracker_C");

/// A buff's stack count as SpawnBuffActor's int argument.
fn stack_count(s: Real) -> i32 {
    #[allow(clippy::cast_possible_truncation, reason = "a stack count is an integer")]
    let n = s as i32;
    n
}

fn collateral_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    set_ext(w, b, PerkExt::Collateral { by: Some(from.unwrap_or(f)) });
}

fn collateral_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(victim) = a.victim else { return false };
    if s != BuffSignal::HitEnemy {
        return false;
    }
    let by = match ext(w, b) {
        Some(PerkExt::Collateral { by }) => by.unwrap_or(f),
        _ => f,
    };
    let n = stack_count(stacks(w, b));
    spawn_buff(w, victim, TRACKER, 2.0, Some(n), Some(by), true, None);
    false
}

fn collateral_actor(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, a_arg: Option<&ActorBuffArg>) -> bool {
    let by = a_arg.map(|x| ActorRef::Fighter(x.by));
    let Some(ActorRef::Fighter(v)) = by else { return false };
    if s != ActorBuffSignal::HitEnemyPlayer {
        return false;
    }
    let Some(n) = super::conds::actor_buff(w, a, i).map(|e| e.stacks) else { return false };
    let owner = actor_owner(w, a);
    spawn_buff(w, v, TRACKER, 2.0, Some(n), owner, true, None);
    false
}

// ---------------------------------------------------------------- Buff_CollateralDamageTracker
// IsDebuff, BuffDuration 2, CanRefresh, CanStack, MaxStacks 5, DamagePerStack 1, DestroyAfterHit, BufferTime 1.5,
// BounceDamage: Damage 0 and every other field 0 / false.
// BeginPlay @555 (authority): BounceDamage.Damage += CurrentStackAmount x DamagePerStack @595-696 (once: a later
// AddStack or refresh does not recompute it); the owner a BP_FighterCharacter with a Comp_Pawn_HitstunHandler whose
// CheckHitstunActive is true @976 -> binds the handler's HitStunEnded to OnHitStunEnded and the pawn's OnIsBouncing
// to OnBounce @1086-1173; else K2_DestroyActor @1215.
// OnHitStunEnded @1235: K2_DestroyActor.
// OnBounce @15: BufferEnabled -> nothing; a type other than 1..6 -> nothing @30-300; else BufferEnabled = true, the
// buff's own Comp_Pawn_HitBoxHandler.pfgDoDamage(null, the owner, BounceDamage, null, [], [], false, false) @332
// (a packet of Damage alone takes the light path, credited to the Instigator); DestroyAfterHit -> K2_DestroyActor
// @469; else Delay(BufferTime) @484 -> BufferEnabled = false @543.
// by: the actor's Instigator (SpawnBuffActor's), the hit's credit.
fn tracker_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    let d = def(w, b);
    let bounce = d.obj("BounceDamage");
    let base = bounce.and_then(|o| o.get("Damage")).and_then(Value::as_f64).unwrap_or(0.0);
    let dmg = (stacks(w, b) * num(d, "DamagePerStack")) + crate::math::narrow(base);
    set_ext(w, b, PerkExt::Tracker { dmg, buffer: false, left: 0.0, by: from });
    if !in_hitstun(w, f) {
        end_buff(w, f, b);
    }
}

fn tracker_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s == BuffSignal::HitStunEnded {
        return true;
    }
    if s != BuffSignal::IsBouncing {
        return false;
    }
    let Some(PerkExt::Tracker { dmg, buffer, by, .. }) = ext(w, b).copied() else { return false };
    let t = a.bounce.unwrap_or(0.0);
    if buffer || !(1.0..=6.0).contains(&t) {
        return false;
    }
    if let Some(PerkExt::Tracker { buffer, .. }) = ext_mut(w, b) {
        *buffer = true;
    }
    buff_actor_light_hit(w, f, b, f, by, dmg, false, false);
    let d = def(w, b);
    if flag(d, "DestroyAfterHit") {
        return true;
    }
    let left = latent_frames(num(d, "BufferTime"));
    if let Some(PerkExt::Tracker { left: l, .. }) = ext_mut(w, b) {
        *l = left;
    }
    false
}

fn tracker_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    if let Some(PerkExt::Tracker { buffer, left, .. }) = ext_mut(w, b)
        && *left > 0.0
    {
        *left -= 1.0;
        if *left <= 0.0 {
            *buffer = false;
        }
    }
    false
}

pub(super) fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    enter(
        r,
        &[
            kind(
                C::BuffPerkAbilityRefundOnDebuffHit,
                BuffHooks { on: Some(refund_on_debuff_hit), ..NO_HOOKS },
                Some(NO_ACTOR),
            ),
            kind(
                C::BuffPerkAbilityRefundOnHit,
                NO_HOOKS,
                Some(ActorBuffHooks { on: Some(refund_on_hit_actor), ..NO_ACTOR }),
            ),
            kind(C::BuffBackToBack, BuffHooks { taken_cond: Some(back_to_back), ..NO_HOOKS }, None),
            kind(
                C::BuffPerkJumpCount,
                BuffHooks {
                    begin: Some(jump_begin),
                    on: Some(jump_on),
                    tick: Some(jump_tick),
                    stacks: Some(jump_stacks),
                    ..NO_HOOKS
                },
                None,
            ),
            kind(
                C::BuffCollateralDamage,
                BuffHooks { begin: Some(collateral_begin), on: Some(collateral_on), ..NO_HOOKS },
                Some(ActorBuffHooks { on: Some(collateral_actor), ..NO_ACTOR }),
            ),
            kind(
                C::BuffCollateralDamageTracker,
                BuffHooks { begin: Some(tracker_begin), on: Some(tracker_on), tick: Some(tracker_tick), ..NO_HOOKS },
                None,
            ),
        ],
    )
}
