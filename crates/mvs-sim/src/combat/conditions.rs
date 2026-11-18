//! Conditions (the shared ones and the characters' `conditions(...)`): a plain one is a `fn` in the registry; a
//! condition class (`condClasses`) keeps state per fighter, a [`CondState`], and its behaviour is a `match` on
//! [`CondClass`] here.

use crate::State;
use crate::World;
use crate::actors::ActorRef;
use crate::fighter::{self, FighterId, control};
use crate::names::Name;
use crate::registry::entries::{CondFn, Condition};
use crate::registry::signals::CondSignal;
use crate::registry::{Registry, RegistryError};

/// A condition class (`COND_CLASSES`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CondClass {
    /// `HitEnemyCondition_C`.
    HitEnemy,
    /// `HitNonPlayerCondition_C`.
    HitNonPlayer,
    /// `FinnAimDashCondition_C`.
    FinnAimDash,
}

/// A condition class's state (`make()`'s object), a variant per state shape.
#[derive(State, Debug)]
pub enum CondState {
    /// `HitCond` of `HitEnemyCondition_C` and `HitNonPlayerCondition_C`.
    Hit { has_hit: bool, bound: bool },
    /// `FinnAimDashCondition_C`'s, with Finn's port.
    FinnAimDash,
}

/// `WallJump`, the trace channel.
const TRACE_WALL_JUMP: i32 = 2;

/// BaseFighter BPI_IsOnWall (@13038): IsWallSliding -> true; else BoxTraceSingle from the actor location + (0, 0,
/// CapsuleComponent.CapsuleHalfHeight x 0.45) (the unscaled property) to that + the forward vector x (the scaled
/// radius + 15), half size (0, 0, 50) (UE X, Y, Z: depth 0, hx 0, hy 50), WallJump, ignore self @427; its return.
// Unverified: a crouched capsule's half height is taken as the class's (capHalfU): no reader is on the ground.
pub fn is_on_wall(w: &mut World, f: FighterId) -> bool {
    let me = w.fighter(f);
    if me.wall.wall_sliding {
        return true;
    }
    let (x, facing, half) = (me.body.x, me.body.facing_dir, me.capsule.cap_half_u);
    let y = fighter::actor_y(w, f) + (half * 0.45);
    let x1 = x + facing * (fighter::cap_radius(w, f) + 15.0);
    let own = Some(ActorRef::Fighter(f));
    crate::fighter::traces::box_trace_single(w, x, y, x1, y, 0.0, 50.0, 0.0, TRACE_WALL_JUMP, &[], own).is_some()
}

/// BPI_CheckIfHasBuff's HasBuff (Comp_State_Manager CheckHasBuff): a live buff of the class or a child of it
/// (Buff_CooldownIncrease_C and Buff_Educated_C020_C are children of Buff_Educated_C).
fn has_buff_named(w: &World, f: FighterId, cls: Name) -> bool {
    crate::combat::buffs::has_buff(w, f, cls)
}

fn equipped(w: &World, f: FighterId, tag: Name) -> bool {
    fighter::has_equipped(w, f, tag)
}

fn charge_pct(w: &World, f: FighterId) -> crate::Real {
    w.fighter(f).charge.charge_pct
}

/// The plain conditions, in registration order.
const PLAIN: &[(&str, CondFn)] = &[
    // OnWall Check (OnWall): a BPI_BaseFighter -> BPI_IsOnWall; else false. Stripe's
    // ChainSaw (OnWall_C on its ConditionSectionJump to End, 45.405-144.57).
    ("OnWall_C", is_on_wall),
    // Both conditions read GetIsInAir (the pending launch test they add is part of it).
    ("PlayerOnGroundCondition_C", |w, f| !fighter::in_air(w, f)),
    ("PlayerInAirCondition_C", |w, f| fighter::in_air(w, f)),
    // Not holding an enemy, under AirSpecialMax, and in air or with a pending launch upwards.
    // Check @79-801: !BPI_GetIsHoldingEnemy AND AirSpecialNumber < AirSpecialMax AND (GetIsInAir OR the launch).
    ("AirSpecialLimitCondition_C", |w, f| {
        !w.fighter(f).holds.holding_enemy
            && fighter::in_air(w, f)
            && crate::real(w.fighter(f).air.air_specials) < control::air_special_max(w, f)
    }),
    // Check @401 also writes the branch manager's PreviousAttackCancelled (noteAttackCancelled).
    ("CurrentAttackHitCondition_C", |w, f| {
        let hit = w.fighter(f).montages.montage_hit;
        crate::fighter::moves::note_attack_cancelled(w, f, hit)
    }),
    ("CurrentAttackNotHitCondition_C", |w, f| !w.fighter(f).montages.montage_hit),
    ("AllyNotWithinRangeCondition_C", |w, f| crate::fighter::match_side::ally_not_within_range(w, f)),
    // AlliesWithinDashDistance.Check: an ally within the owner's Comp_Pawn_TeleportToAlly
    // DashDistance, inclusive. Not the Not of the condition above (>= 2250 of the last ally, a literal).
    ("AlliesWithinDashDistance_C", |w, f| crate::fighter::match_side::allies_within_dash_distance(w, f)),
    ("NotRagedCondition_C", |w, f| !w.fighter(f).rage.rage_enabled),
    ("RageCondition_C", |w, f| w.fighter(f).rage.rage_enabled),
    // GetChargeValue(4) of Comp_Pawn_ChargeAttack against Percent (ChargeOver50Percent: 0.5) / 0.5.
    // Value 4 = PercentCharged = NewMultiplier(2) - 1 = min(elapsed / ChargeTotalTime, 1) (UpdateChargeValues; 1 from
    // StartAttackChargeMultipliersServer), and 0 when AttackMontage is not the active montage.
    ("ChargeOver50Percent_C", |w, f| charge_pct(w, f) >= 0.5),
    ("ChargeUnder50Percent_C", |w, f| charge_pct(w, f) < 0.5),
    // Charge100Percent Check (Charge100Percent): the actor's Comp_Pawn_ChargeAttack
    // (none: false @250), GetChargeValue(4) @145 >= 1.0 @192: the charge full, the same value as above.
    ("Charge100Percent_C", |w, f| charge_pct(w, f) >= 1.0),
    // ReachedAlly.Check: the owner's Comp_Pawn_TeleportToAlly.ReachedAlly (false without the component).
    ("ReachedAlly_C", |w, f| w.fighter(f).team.ally.reached),
    // Ability0Ready (AbilityIndex 0; Ability1Ready and Ability4Ready are children with index 1 and 4): Check is
    // the manager's TriggerAbilityCheck. The NotReady / ReadyNot children negate the parent's Check.
    ("Ability0Ready_C", |w, f| control::trigger_ability_check(w, f, 0)),
    ("Ability0NotReady_C", |w, f| !control::trigger_ability_check(w, f, 0)),
    ("Ability1Ready_C", |w, f| control::trigger_ability_check(w, f, 1)),
    ("Ability1NotReady_C", |w, f| !control::trigger_ability_check(w, f, 1)),
    ("Ability4Ready_C", |w, f| control::trigger_ability_check(w, f, 4)),
    ("Ability4ReadyNot_C", |w, f| !control::trigger_ability_check(w, f, 4)),
    // Ability2Ready (a child of Ability0Ready with AbilityIndex 2): Check @0-29 and CheckServer @0-29 forward the
    // parent's, which are the manager's TriggerAbilityCheck(index) (Ability0Ready Check @145) and
    // HasAbilityResources(index) (CheckServer @145), false without the manager (@223). Ability2NotReady (a child of
    // Ability2Ready): Not of the parent's Check @29, and Not of the parent's CheckServer @29. The port has the one
    // Check, as for every class here.
    ("Ability2Ready_C", |w, f| control::trigger_ability_check(w, f, 2)),
    ("Ability2NotReady_C", |w, f| !control::trigger_ability_check(w, f, 2)),
    // Ability3Ready: a child of Ability0Ready with no function of its own and AbilityIndex 3 (its class defaults):
    // the manager's TriggerAbilityCheck(3).
    ("Ability3Ready_C", |w, f| control::trigger_ability_check(w, f, 3)),
    // Ability5Ready: a direct child of Condition2 with its own Check and CheckServer, the same bodies as
    // Ability0Ready's (Check @145 the manager's TriggerAbilityCheck(AbilityIndex), false without the manager @223),
    // AbilityIndex 5 (its class defaults). Ability5NotReady (a child of Ability5Ready): Not of the parent's Check
    // @29. Rick's montages name them.
    ("Ability5Ready_C", |w, f| control::trigger_ability_check(w, f, 5)),
    ("Ability5NotReady_C", |w, f| !control::trigger_ability_check(w, f, 5)),
    // FighterHoldingEnemyCondition Check: BPI_GetIsHoldingEnemy (BaseFighter's flag, set by a hold buff's child
    // graph: Buff_HeldBy_C017 @3220 / @3391).
    ("FighterHoldingEnemyCondition_C", |w, f| w.fighter(f).holds.holding_enemy),
    // FighterNotHoldingEnemyCondition Check @79-154: Not(BPI_GetIsHoldingEnemy), the same flag. An actor without the
    // fighter interface reads the flag as false (@178) and passes.
    ("FighterNotHoldingEnemyCondition_C", |w, f| !w.fighter(f).holds.holding_enemy),
    // HasEducatedBuff Check @131-282: the owner's BPI_CheckIfHasBuff(Buff_Educated_C, the class of
    // Abilities/Shield/BL_Shaggy)'s HasBuff, which is Comp_State_Manager CheckHasBuff @757: one of the owner's buff
    // actors IsBuffOfClass that class. An actor without BPI_BaseFighter: false @306. NoHasEducatedBuff Check
    // @131-311: Not of the same HasBuff; without the interface true @335.
    // The class is given by VelmaBookProjectile's On Hit Buffs (chars/velma/gadgets.rs: an ally the book meets gets
    // Buff_Educated), so a Velma match can answer true; the rows behind HasEducatedBuff (Superman_IceBreath_Montage's
    // and its air twin's spawners of the pellet's Educated twin) are reached once a book has met a Superman ally.
    // Unverified: IsBuffOfClass is not read (a child class's actor, Buff_CooldownIncrease_C, may pass it); the port
    // tests the exact class, which matters only once something gives either buff.
    ("HasEducatedBuff_C", |w, f| has_buff_named(w, f, crate::name!("Buff_Educated_C"))),
    ("NoHasEducatedBuff_C", |w, f| !has_buff_named(w, f, crate::name!("Buff_Educated_C"))),
    // SatchelEquippedCondition Check @176: the owner's Comp_Spawner.IsEquippedComponents contains 'SatchelCharge'
    // (AnimNotify_Equip's list); SatchelNotEquippedCondition @258: its Not. Without a spawner both are false (@282,
    // @311); every fighter has one.
    ("SatchelEquippedCondition_C", |w, f| equipped(w, f, crate::name!("satchelcharge"))),
    ("SatchelNotEquippedCondition_C", |w, f| !equipped(w, f, crate::name!("satchelcharge"))),
    // PieEquippedCondition Check @51-409: the owner's Comp_Spawner.IsEquippedComponents contains 'Pie' OR
    // 'PieProp1'; PieNotEquippedCondition its Not. Without a spawner both are false (@433); every fighter has one.
    ("PieEquippedCondition_C", |w, f| equipped(w, f, crate::name!("pie")) || equipped(w, f, crate::name!("pieprop1"))),
    ("PieNotEquippedCondition_C", |w, f| {
        !(equipped(w, f, crate::name!("pie")) || equipped(w, f, crate::name!("pieprop1")))
    }),
    // HasSmoothMovesEquipped Check @79-254: the owner's BPI_CheckIfHasBuff(Buff_Harley_SmoothMoves_C)'s HasBuff; no
    // fighter interface false @278. The buff is the Smooth Moves perk's marker (Perk_Harley_SmoothMoves,
    // SpawnOnAllies): with it the rows behind it (HarleyQuinn_Special_SmoothMoves_Montage, _Air_Montage) are reached.
    ("HasSmoothMovesEquipped_C", |w, f| {
        crate::combat::buffs::has_buff(w, f, crate::name!("Buff_Harley_SmoothMoves_C"))
    }),
    // DownStickCondition Check @169-258: the owner's BPI_GetMoveAxisValues MoveY <= -0.1. Without the fighter
    // interface MoveY is 0 (@305): false. BPI_GetMoveAxisValues returns BaseFighter's MoveXAxisValue /
    // MoveYAxisValue as they stand (@0-27): the stick's value of the tick's input event (or the X / Y override, or a
    // bot's write), after the player input's dead zone and nothing else: no buffer, no 8-way direction, no facing.
    // MoveY below 0 is down (the fast fall reads MoveYAxisValue <= -0.4, BaseFighter @120863).
    ("DownStickCondition_C", |w, f| w.fighter(f).controls.inp.y <= -0.1),
    // NotFlyingCondition Check @0-254: the actor's Comp_Pawn_Fly (GetComponentByClass @0, IsValid @51): Not(Get Is
    // Flying) @145-219; no component: true @243.
    ("NotFlyingCondition_C", |w, f| !crate::fighter::moves::is_flying(w, f)),
    // OppositeFacingStickCondition Check @169-560: (MoveX > 0.3 AND BPI_IsFacingLeft) OR (MoveX < -0.3 AND NOT
    // BPI_IsFacingLeft), both strict. MoveX as above (below 0 is the stick to the left: BaseFighter's turn is
    // SetRotation(MoveXAxisValue < 0), @6307-6341); BPI_IsFacingLeft is BaseFighter's FacingLeft of that moment, not
    // the facing the montage started with. Without the fighter interface MoveX is 0 (@584): false.
    // Its CheckServer override returns true (@0): the server's second look (BPI_CheckConditionsServer, the branch
    // manager's ValidateBranch) never refuses it; the port has the one Check, as for every class here.
    ("OppositeFacingStickCondition_C", |w, f| {
        let me = w.fighter(f);
        let (x, facing) = (me.controls.inp.x, me.body.facing_dir);
        (x > 0.3 && facing < 0.0) || (x < -0.3 && facing > 0.0)
    }),
    // NotMinionCondition Check: the actor a Base_Fighter @0-65, Not(IsMinion) @79; a failed cast false @154.
    ("NotMinionCondition_C", |w, f| !w.fighter(f).life.is_minion),
    // NPCCondition Check: the actor's BPI_GetNotPlayerCharacter @79; without the interface false @155 -> @171.
    ("NPCCondition_C", |w, f| w.fighter(f).life.not_player_character),
    // EvadeButtonPressedCondition Check: the actor a Base_Fighter that IsLocallyControlled @65-111 (false: the return
    // value stays false @256), then IsButtonPressed(3) @190, Enum_InputButton Evade. Every fighter of the port is
    // locally controlled: offline the players and the AI pawns are (an AI pawn is locally controlled in standalone),
    // and under rollback each peer runs every fighter from its inputs, as for the input notifies; so a bot's answer is
    // the button its BT tasks press. The button test is the other IsButtonPressed readers' (chargeHeld): held this
    // tick (no forced release names the evade button).
    ("EvadeButtonPressedCondition_C", |w, f| w.fighter(f).controls.inp.dodge),
    // SpecialInputNotHeld Check: Not of BPI_IsButtonPressed(1) @131-208, Enum_InputButton Special; an actor without
    // BPI_BaseFighter false @232 (every fighter has it). Held this tick, as EvadeButtonPressedCondition.
    ("SpecialInputNotHeld_C", |w, f| !w.fighter(f).controls.inp.special),
];

/// The classes with variables (`COND_CLASSES`): one state per (fighter, class), BaseFighter.GetCondition @10252,
/// shared by every notify and montage of the fighter that names the class.
const CLASSES: &[(&str, CondClass)] =
    &[("HitEnemyCondition_C", CondClass::HitEnemy), ("HitNonPlayerCondition_C", CondClass::HitNonPlayer)];

/// The conditions entered after the classes.
const PLAIN_LATE: &[(&str, CondFn)] = &[
    // ChargeBetween50_100Percent Check: the actor's Comp_Pawn_ChargeAttack (none: false @322), GetChargeValue(4)
    // @145 >= 0.5 @226 AND < 1.0 @192.
    ("ChargeBetween50_100Percent_C", |w, f| charge_pct(w, f) >= 0.5 && charge_pct(w, f) < 1.0),
];

/// This module's entries, in registration order.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for &(n, c) in PLAIN {
        r.conditions.enter(n, Condition::Fn(c))?;
    }
    for &(n, c) in CLASSES {
        r.conditions.enter(n, Condition::Class(c))?;
    }
    for &(n, c) in PLAIN_LATE {
        r.conditions.enter(n, Condition::Fn(c))?;
    }
    Ok(())
}

/// A class's `make()`.
pub fn make(cls: CondClass) -> CondState {
    match cls {
        CondClass::HitEnemy | CondClass::HitNonPlayer => CondState::Hit { has_hit: false, bound: false },
        CondClass::FinnAimDash => CondState::FinnAimDash,
    }
}

/// The fighter's state of the class entered as `name`, made at its first use.
fn state_of(w: &mut World, f: FighterId, name: Name, cls: CondClass) -> &mut CondState {
    w.fighter_mut(f).buffs.cond_objs.get_or_insert_with(Default::default).entry(name).or_insert_with(|| make(cls))
}

/// A class's `check`.
// Their Check's DoOnce is made of function locals (Temp_bool_IsClosed_Variable, Temp_bool_Has_Been_Initd_Variable:
// cleared at every call), so it opens at every Check: each one binds SetHit to the owner's Comp_Pawn_HitBoxHandler
// broadcast first, then reads HasHit. A bind of a handler already bound adds nothing (Unverified:
// EX_AddMulticastDelegate taken as AddUnique, the 4.26 FMulticastScriptDelegate rule): `bound` is that one binding.
// HitCheck is the return value alone (Check sets it false @5 before reading it), so it is not kept.
pub fn check(w: &mut World, f: FighterId, name: Name, cls: CondClass) -> bool {
    match cls {
        // HitEnemyCondition. Check: SetHit bound to HitEnemyPlayerLight @691-765, then LandedAttackIDs contains
        // BPI_GetPlayerAttackCount @212-323 (the attack the owner is in has landed) AND HasHit @405: HasHit = false,
        // HitCheck = true and the handler unbound @453-549. HitEnemyPlayerLight is broadcast by the HitBoxHandler for
        // each enemy fighter its hit boxes hit, the light hits too, and from a wall bounce of a fighter this one sent.
        CondClass::HitEnemy => {
            let me = w.fighter(f);
            let landed = me.decay.landed_attack_ids.contains(&me.decay.attack_count);
            let CondState::Hit { has_hit, bound } = state_of(w, f, name, cls) else { unreachable!() };
            *bound = true;
            if !(*has_hit && landed) {
                return false;
            }
            *has_hit = false;
            *bound = false;
            true
        }
        // HitNonPlayerCondition. Check: SetHit bound to HitNonPlayer @266-340, then HasHit @46: HasHit = false,
        // HitCheck = true and the handler unbound @56-152. HitNonPlayer: a hit box of the owner landed on an actor
        // that is not a fighter (the melee tests on items, damage boxes, actors and projectiles).
        CondClass::HitNonPlayer => {
            let CondState::Hit { has_hit, bound } = state_of(w, f, name, cls) else { unreachable!() };
            *bound = true;
            if !*has_hit {
                return false;
            }
            *has_hit = false;
            *bound = false;
            true
        }
        CondClass::FinnAimDash => {
            state_of(w, f, name, cls); // the class's object is made at the first check
            crate::chars::finn::aimer::aim_dash_check(w, f, name)
        }
    }
}

/// A class's `reset`; a class without one does nothing.
pub fn reset(w: &mut World, f: FighterId, name: Name, cls: CondClass) {
    match cls {
        // HitEnemyCondition Reset @0-156: the parent's (Condition2, empty), HasHit and HitCheck false, the handler
        // unbound.
        CondClass::HitEnemy => {
            let CondState::Hit { has_hit, bound } = state_of(w, f, name, cls) else { unreachable!() };
            *has_hit = false;
            *bound = false;
        }
        // HitNonPlayerCondition: parent Condition2, no Reset of its own (Condition2's is empty: HasHit survives a
        // Reset).
        CondClass::HitNonPlayer => {}
        CondClass::FinnAimDash => {
            state_of(w, f, name, cls);
            crate::chars::finn::aimer::aim_dash_reset(w, f, name);
        }
    }
}

/// A class's `signal`, to one state: SetHit @0: HasHit = true, while bound.
pub fn signal(s: &mut CondState, cls: CondClass, got: CondSignal) {
    let sig = match cls {
        CondClass::HitEnemy => CondSignal::HitEnemyLight,
        CondClass::HitNonPlayer => CondSignal::HitNonPlayer,
        CondClass::FinnAimDash => return,
    };
    if let CondState::Hit { has_hit, bound } = s
        && got == sig
        && *bound
    {
        *has_hit = true;
    }
}

#[cfg(test)]
mod tests {
    use super::register;
    use crate::registry::Registry;

    /// The classes this module registers, in its order.
    const TS: &str = "OnWall_C,PlayerOnGroundCondition_C,PlayerInAirCondition_C,AirSpecialLimitCondition_C,\
        CurrentAttackHitCondition_C,CurrentAttackNotHitCondition_C,AllyNotWithinRangeCondition_C,\
        AlliesWithinDashDistance_C,NotRagedCondition_C,RageCondition_C,ChargeOver50Percent_C,ChargeUnder50Percent_C,\
        Charge100Percent_C,ReachedAlly_C,Ability0Ready_C,Ability0NotReady_C,Ability1Ready_C,Ability1NotReady_C,\
        Ability4Ready_C,Ability4ReadyNot_C,Ability2Ready_C,Ability2NotReady_C,Ability3Ready_C,Ability5Ready_C,\
        Ability5NotReady_C,FighterHoldingEnemyCondition_C,FighterNotHoldingEnemyCondition_C,HasEducatedBuff_C,\
        NoHasEducatedBuff_C,SatchelEquippedCondition_C,SatchelNotEquippedCondition_C,PieEquippedCondition_C,\
        PieNotEquippedCondition_C,HasSmoothMovesEquipped_C,DownStickCondition_C,NotFlyingCondition_C,\
        OppositeFacingStickCondition_C,NotMinionCondition_C,NPCCondition_C,EvadeButtonPressedCondition_C,\
        SpecialInputNotHeld_C,HitEnemyCondition_C,HitNonPlayerCondition_C,ChargeBetween50_100Percent_C";

    #[test]
    fn registers_the_ts_classes_in_order() {
        let mut r = Registry::empty();
        register(&mut r).expect("no class twice");
        let got: Vec<&str> = r.conditions.names().collect();
        let want: Vec<&str> = TS.split(',').map(str::trim).collect();
        assert_eq!(got, want);
    }
}
