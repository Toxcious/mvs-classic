//! Comp_JakeEat: the swallow's component on Jake's pawn (`JakeState::eat`,
//! `holding_actor`, `held_fighters`): the eat timer, the held fighters and their visuals, the spit and the ally drop on
//! the server, the struggle. Its hooks (`matchReset`: ReceiveBeginPlay's RemoveHeldFighterVisuals; `componentTick`;
//! `timerTick`) are joined in mod.rs and `hooks`. It enters no class (the buffs are eat_buffs.rs's).
//!
//! Offsets are its ubergraph's unless a function is named. AddHeldFighers @5007, RemoveHeldFighter @7412, the visuals
//! @8183 / @8801, the EatTimer (@6354) and SpitTimeEnd @9807, the struggle (StruggleServer @11613 -> @10739 / @10331,
//! StruggleVisuals @10997), SpitServer @9725 -> SpitClient @9515, DropAllyServer @11980, UnpauseTimer @9982, the
//! component's ReceiveTick @12446 -> @4837. JakeSpitEvent (the InputPreTriggered listener @8046: input 1 -> SpitServer,
//! 3 -> DropAllyServer) is spit.rs's (while `eat.spit_bound`), DropAllyClient and the ally's JakeEatAlly_Aim
//! ally_spit.rs's. Its class data: `components.pawn['Comp_JakeEat_GEN_VARIABLE']` (EatHoldTime 5, MaxStruggleCount 8,
//! StruggleMult 0.5, JakeAddedArmor).
//!
//! Not ported: IncrementCoopActions @5983 (the match's co-op stat), EjectServer (empty), BackupCheck (the Delay(0.1)
//! @6449 -> @15: the component's tick @4837 does the same removal every frame), EatOverridePack (no reader), the saved
//! activator fields (no reader).
// Deviation: BackupCheck's one frame ahead of the tick on the frame it fires is not modelled.

use super::ally_spit::{activate_ally_aim, drop_ally_client, spawn_ally_aim};
use super::pawn::holding_fighter;
use super::state::{jake, jake_of, jake_w};
use crate::Real;
use crate::combat::buffs::{buff_of, end_buff, spawn_buff};
use crate::combat::defense::{ArmorPack, add_armor_pack, remove_armor};
use crate::consts::DELTA_SECONDS;
use crate::fighter::anim::grouped::play_grouped;
use crate::fighter::common::{data_of, lit};
use crate::fighter::moves::{Impulse, bpi_allow_wall_play, bpi_disable_dash, disable_evade, enable_evade};
use crate::fighter::{FighterId, active_montage_name, grounded, is_attacking};
use crate::name;
use crate::names::Name;
use crate::registry::CharHooks;
use crate::registry::props::{flag_or, num_or, obj};
use crate::world::World;
use mvs_data::value::Props;

/// Buff_JakEat's class (the swallowed fighter's hold).
pub const JAK_EAT: &str = "Buff_JakEat_C";

/// Buff_JakEatSlow's class (Jake's own slow while he holds).
pub const JAK_EAT_SLOW: &str = "Buff_JakEatSlow_C";

const COMPONENT: &str = "Comp_JakeEat_GEN_VARIABLE";

/// A number of the component's class data, `default` where the data lacks it.
fn comp_num(w: &World, f: FighterId, k: &str, default: Real) -> Real {
    data_of(w, f)
        .fighter
        .components
        .pawn
        .get(COMPONENT)
        .and_then(|c| c.p.as_obj())
        .map_or(default, |p| num_or(p, k, default))
}

fn hold_time(w: &World, f: FighterId) -> Real {
    comp_num(w, f, "EatHoldTime", 5.0)
}

/// `JakeAddedArmor` as an armor pack (each field its default when the data lacks it).
fn added_armor(w: &World, f: FighterId) -> ArmorPack {
    let empty = Props::new();
    let c = data_of(w, f).fighter.components.pawn.get(COMPONENT).and_then(|c| c.p.as_obj());
    let a = c.and_then(|c| obj(c, "JakeAddedArmor")).unwrap_or(&empty);
    ArmorPack {
        kind: armor_type_of(w, a),
        can_override: flag_or(a, "CanOverride", true),
        amount: num_or(a, "ArmorAmount", 40.0),
        max_hits: num_or(a, "MaxHits", 1.0),
        threshold: num_or(a, "KnockBackThreshold", 8000.0),
        kb_res: num_or(a, "KnockBackResistanceMultiplier", 0.0),
        dmg_res: num_or(a, "DamageResistanceMultiplier", 1.0),
        hs_res: num_or(a, "HitStunResistanceMultiplier", 0.0),
    }
}

fn armor_type_of(w: &World, a: &Props) -> Name {
    match crate::registry::props::text(a, "ArmorType") {
        Some(s) => lit(w, s),
        None => name!("None"),
    }
}

fn armor_type(w: &World, f: FighterId) -> Name {
    added_armor(w, f).kind
}

// ---------------------------------------------------------------- the EatTimer
// Set and paused / unpaused in the tick groups, so a set or an unpause is pending until the frame's timer pass, which
// starts it without counting that frame; an active one fires in the first timer pass whose counted deltas pass its rate.

fn set_eat_timer(w: &mut World, f: FighterId) {
    let rate = hold_time(w, f);
    let e = &mut jake_w(w, f).eat;
    e.timer = 1;
    e.rate = f64::from(rate);
    e.elapsed = 0.0;
}

fn clear_eat_timer(w: &mut World, f: FighterId) {
    jake_w(w, f).eat.timer = 0;
}

/// K2_PauseTimerHandle: an active timer keeps what is left, a pending one its rate.
fn pause_eat_timer(w: &mut World, f: FighterId) {
    let e = &mut jake_w(w, f).eat;
    if e.timer == 2 {
        e.rate -= e.elapsed;
    }
    if e.timer == 1 || e.timer == 2 {
        e.timer = 3;
    }
}

/// K2_GetTimerRemainingTimeHandle (a float): 0 for an invalid handle (never set, or cleared by
/// K2_ClearAndInvalidateTimerHandle), -1 for a valid handle whose timer is gone (fired: 4), the rate while pending or
/// paused, else ExpireTime - InternalTime.
fn eat_remaining(w: &World, f: FighterId) -> Real {
    let e = &jake(w, f).eat;
    match e.timer {
        0 => 0.0,
        4 => -1.0,
        2 => crate::math::narrow(e.rate - e.elapsed),
        _ => crate::math::narrow(e.rate),
    }
}

/// The frame's timer pass (`CharHooks::timer_tick`): the fire first (FTimerManager::Tick: InternalTime += dt, then the
/// due timers), then a pending one goes active.
fn timer_tick(w: &mut World, f: FighterId) {
    let e = &mut jake_w(w, f).eat;
    if e.timer == 2 {
        e.elapsed += f64::from(DELTA_SECONDS);
        if e.elapsed > e.rate {
            e.timer = 4;
            spit_time_end(w, f);
        }
    } else if e.timer == 1 {
        e.timer = 2;
        e.elapsed = 0.0;
    }
}

// ---------------------------------------------------------------- the visuals

/// AddHeldFightersVisuals @8183-8800 (a multicast): BPI_DisableEvade, BPI_AllowWallPlay(false) @8219, CrouchLocked,
/// HoldingActor, BPI_SetIsHoldingEnemy(true), the AnimBP's BPI_HoldingFighter(true), BPI_DisableDash(true).
fn visuals_on(w: &mut World, f: FighterId) {
    disable_evade(w, f);
    bpi_allow_wall_play(w, f, false);
    w.fighter_mut(f).crouch.crouch_locked = true;
    jake_w(w, f).holding_actor = true;
    w.fighter_mut(f).holds.holding_enemy = true;
    holding_fighter(w, f, true);
    bpi_disable_dash(w, f, true);
}

/// RemoveHeldFighterVisuals @8801-9445: the reverse in another order, BPI_AllowWallPlay(true) @9385 (Jake's CDO is
/// false; ReceiveBeginPlay @4888-5002 runs this once: OwningFighter, then goto @3549 RemoveHeldFighterVisuals, so a
/// new pawn has wall play), then InputPreTriggered -= JakeSpitEvent @9445.
fn visuals_off(w: &mut World, f: FighterId) {
    w.fighter_mut(f).crouch.crouch_locked = false;
    jake_w(w, f).holding_actor = false;
    w.fighter_mut(f).holds.holding_enemy = false;
    bpi_disable_dash(w, f, false);
    holding_fighter(w, f, false);
    enable_evade(w, f);
    bpi_allow_wall_play(w, f, true);
    jake_w(w, f).eat.spit_bound = false;
}

fn same_team(w: &World, a: FighterId, b: FighterId) -> bool {
    w.fighter(a).team.team == w.fighter(b).team.team
}

// ---------------------------------------------------------------- Comp_JakeEat's functions other files call
// Called on a Jake. The port is the authority (a server RPC and a multicast run here).

/// AddHeldFighers(Fighter) @5007 (the build's spelling), authority. Caller: Buff_JakEat's attach @397 (eat_buffs.rs).
/// SetHeldFighterMulticast(add) @5079 -> @11628 AddUnique. Same team (Comp_Actor_Destructible TeamIndex) @5225:
/// AddArmorMulticast(true) @5392 -> @11786 Comp_Armor AddArmorPack(JakeAddedArmor), the ally's Comp_Spawner
/// CompSpawnActor(JakeEatAlly_Aim) @5678 (ally_spit.rs). Else BindOrUnbindStruggle(true) @6230 -> @10042 (the victim's
/// HitstunHandler.Struggling += StruggleServer). Both: SpawnBuffActor(Buff_JakEatSlow on Jake, instigator and caller
/// Jake, 0 stacks, 0 s, BreakIfBlocked false) @6091 -> SlowBuff @6175 (a second swallow's spawn meets the live one: no
/// CanOverride, so SweepBuffState returns none and SlowBuff is none). Then the DoOnce @6194: InputPreTriggered +=
/// JakeSpitEvent @6285, the visuals @6308, SetTimer(SpitTimeEnd, EatHoldTime) @6354, TimerActive @6438 (the Delay(0.1)
/// BackupCheck: see the head).
// Deviation: a second swallow's spawn meets the live Buff_JakEatSlow and spawns none; the first buff then outlives the
// swallow, as the port's spawn_buff has it.
pub fn add_held_fighters(w: &mut World, f: FighterId, victim: FighterId) {
    if jake_of(w, f).is_none() {
        return;
    }
    if !jake(w, f).held_fighters.contains(&victim) {
        jake_w(w, f).held_fighters.push(victim);
    }
    if same_team(w, victim, f) {
        let pack = added_armor(w, f);
        add_armor_pack(w, f, &pack);
        spawn_ally_aim(w, f, victim);
    } else if !jake(w, f).eat.struggle_bound.contains(&victim) {
        jake_w(w, f).eat.struggle_bound.push(victim);
    }
    let slow = spawn_buff(w, f, name!("Buff_JakEatSlow_C"), 0.0, Some(0), Some(f), false, Some(f));
    jake_w(w, f).eat.slow_buff = slow;
    if jake(w, f).eat.closed {
        return;
    }
    let e = &mut jake_w(w, f).eat;
    e.closed = true;
    e.spit_bound = true;
    visuals_on(w, f);
    set_eat_timer(w, f);
    jake_w(w, f).eat.timer_active = true;
}

/// RemoveHeldFighter(Fighter) @7412, authority. Caller: Buff_JakEat's EndPlay @964 (eat_buffs.rs, whether or not the
/// fighter was added), the component's tick @4763. Same team: Buff_ArmorPack on the fighter (instigator Jake, 0 stacks,
/// 5 s) @7773; else Buff_Weakened (2 stacks, 0 s) @7957. SetHeldFighterMulticast(remove) @7857 -> @11711 RemoveItem.
/// @6520 HeldFighters empty: K2_DestroyActor(SlowBuff) @6671, the visuals off @6707, the EatTimer cleared @6730,
/// AddArmorMulticast(false) @6821 -> @11832 RemoveArmor(JakeAddedArmor.ArmorType), the DoOnce reset @6908, then not
/// IsMovingOnGround and not BPI_IsAttacking -> MulticastBounceBack @7381 -> @12737 LaunchCharacter((0, 0, 1000), false,
/// true).
pub fn remove_held_fighter(w: &mut World, f: FighterId, victim: FighterId) {
    if jake_of(w, f).is_none() {
        return;
    }
    if same_team(w, victim, f) {
        spawn_buff(w, victim, name!("Buff_ArmorPack_C"), 5.0, Some(0), Some(f), true, Some(victim));
    } else {
        spawn_buff(w, victim, name!("Buff_Weakened_C"), 0.0, Some(2), Some(f), true, Some(victim));
    }
    jake_w(w, f).held_fighters.retain(|v| *v != victim);
    if !jake(w, f).held_fighters.is_empty() {
        return;
    }
    let slow = jake_w(w, f).eat.slow_buff.take();
    if let Some(b) = slow {
        end_buff(w, f, b);
    }
    visuals_off(w, f);
    clear_eat_timer(w, f);
    let kind = armor_type(w, f);
    remove_armor(w, f, kind);
    jake_w(w, f).eat.closed = false;
    if !grounded(w, f) && !is_attacking(w, f) {
        w.fighter_mut(f).moves.impulse = Some(Impulse { vx: 0.0, vy: 1000.0, xy: false, kind: 0, z: true, d: None });
    }
}

/// SpitServer @9725: Comp_Pawn_BranchManager IsAttacking -> nothing. Else CanAttack false @8167, SpitFighter cleared
/// @40, the loop over HeldFighters: another team -> AddUnique(SpitFighter), CanAttack true; the same team with one held
/// fighter -> DropAllyServer @877. After the loop CanAttack -> SpitClient @9515 (a client RPC, run here): PauseTimer,
/// TimerActive false, StuggleCount -= StuggleCount / Length (integer division). Caller: JakeSpitEvent (spit.rs).
pub fn spit_server(w: &mut World, f: FighterId) {
    if jake_of(w, f).is_none() || is_attacking(w, f) {
        return;
    }
    {
        let e = &mut jake_w(w, f).eat;
        e.can_attack = false;
        e.spit_fighter.clear();
    }
    let held = jake(w, f).held_fighters.clone();
    for v in &held {
        if !same_team(w, *v, f) {
            let e = &mut jake_w(w, f).eat;
            if !e.spit_fighter.contains(v) {
                e.spit_fighter.push(*v);
            }
            e.can_attack = true;
        } else if jake(w, f).held_fighters.len() == 1 {
            drop_ally_server(w, f);
        }
    }
    if !jake(w, f).eat.can_attack {
        return;
    }
    pause_eat_timer(w, f);
    let n = i32::try_from(jake(w, f).held_fighters.len()).expect("a held-fighter count");
    let e = &mut jake_w(w, f).eat;
    e.timer_active = false;
    if n > 0 {
        e.struggle_count -= e.struggle_count / n;
    }
}

/// DropAllyServer @11980: IsAttacking -> nothing. Else CanDropAlly false, the loop @2625 over HeldFighters of the same
/// team with a Buff_JakEat (BPI_CheckIfHasBuff): CanDropAlly true and BuffActor = that buff (the last one found). @3400
/// CanDropAlly -> DropAllyClient(Jake) @3410 (ally_spit.rs), then K2_DestroyActor(BuffActor) @3433. Callers: SpitServer
/// @877, JakeSpitEvent's input 3 (spit.rs / ally_spit.rs).
pub fn drop_ally_server(w: &mut World, f: FighterId) {
    if jake_of(w, f).is_none() || is_attacking(w, f) {
        return;
    }
    jake_w(w, f).eat.can_drop_ally = false;
    let mut found = None;
    for v in jake(w, f).held_fighters.clone() {
        if !same_team(w, v, f) {
            continue;
        }
        if let Some(b) = buff_of(w, v, name!("Buff_JakEat_C")) {
            jake_w(w, f).eat.can_drop_ally = true;
            found = Some((v, b));
        }
    }
    if !jake(w, f).eat.can_drop_ally {
        return;
    }
    drop_ally_client(w, f);
    if let Some((owner, b)) = found {
        end_buff(w, owner, b);
    }
}

/// UnpauseTimer @9982: K2_UnPauseTimerHandle(EatTimer) (a paused timer alone: pending until the frame's timer pass),
/// TimerActive true. Caller: ProjectileAimer_SpitJake @3572 (spit.rs).
pub fn unpause_eat_timer(w: &mut World, f: FighterId) {
    if jake_of(w, f).is_none() {
        return;
    }
    let e = &mut jake_w(w, f).eat;
    if e.timer == 3 {
        e.timer = 1;
    }
    e.timer_active = true;
}

// ---------------------------------------------------------------- the rest of Comp_JakeEat

/// SpitTimeEnd @9807: TimerActive false; HeldFighters not empty -> EjectServer (empty), BuffsToDestroy cleared, the loop
/// @1443: the same team -> the ally's Comp_Spawner ActivateActors(JakeEatAlly_Aim) @1835 (ally_spit.rs); else the
/// fighter's Buff_JakEat (BPI_CheckIfHasBuff) -> AddUnique(BuffsToDestroy), BindOrUnbindStruggle(false, the buff actor)
/// @2259 (no HitstunHandler on a buff: nothing), StuggleCount 0. Then @1276 each of BuffsToDestroy still valid ->
/// K2_DestroyActor @1170. Callers: the EatTimer, StruggleServer @10288.
fn spit_time_end(w: &mut World, f: FighterId) {
    jake_w(w, f).eat.timer_active = false;
    if jake(w, f).held_fighters.is_empty() {
        return;
    }
    let mut kill = Vec::new();
    for v in jake(w, f).held_fighters.clone() {
        if same_team(w, v, f) {
            activate_ally_aim(w, f, v);
            continue;
        }
        if let Some(b) = buff_of(w, v, name!("Buff_JakEat_C"))
            && !kill.iter().any(|k: &(_, _)| k.1 == b)
        {
            kill.push((v, b));
        }
        jake_w(w, f).eat.struggle_count = 0; // @2283, with or without the buff
    }
    for (v, b) in kill {
        end_buff(w, v, b);
    }
}

/// StruggleServer @11613, bound to a swallowed enemy's HitstunHandler.Struggling (its struggle input): TimerActive ->
/// StruggleVisuals @10997 (the current montage not Jake_Eat_Attack_Montage -> PlayMontage
/// Jake_Eat_Attack_Struggle_Montage @11151, group StruggleGroup), then @10739 StuggleCount < MaxStruggleCount x Length
/// -> + 1; @10331 StuggleCount x (EatHoldTime x StruggleMult / (MaxStruggleCount x Length)) > the EatTimer's remaining
/// time -> StuggleCount 0 @10303, SpitTimeEnd @10288.
fn struggle_server(w: &mut World, f: FighterId) {
    if !jake(w, f).eat.timer_active {
        return;
    }
    if active_montage_name(w, f) != Some(name!("Jake_Eat_Attack_Montage")) {
        play_grouped(w, f, name!("Jake_Eat_Attack_Struggle_Montage"), true, false);
    }
    let n = crate::real(i32::try_from(jake(w, f).held_fighters.len()).expect("a held-fighter count"));
    let max = comp_num(w, f, "MaxStruggleCount", 8.0) * n;
    if crate::real(jake(w, f).eat.struggle_count) < max {
        jake_w(w, f).eat.struggle_count += 1;
    }
    // Divide_FloatFloat by 0 (no fighter held: TimerActive is left true by a release, and the binding stays) gives 0.
    let step = if max == 0.0 { 0.0 } else { (hold_time(w, f) * comp_num(w, f, "StruggleMult", 0.5)) / max };
    if (crate::real(jake(w, f).eat.struggle_count) * step) > eat_remaining(w, f) {
        jake_w(w, f).eat.struggle_count = 0;
        spit_time_end(w, f);
    }
}

/// The Struggling broadcast of fighter `v` (`combat::holds`): Jake `f` answers when his component bound to it.
// Deviation: its listeners are run in the order the port's modules entered them, not the order the build bound them
// (one listener in this build).
pub fn struggling(w: &mut World, f: FighterId, v: FighterId) {
    if jake_of(w, f).is_some_and(|j| j.eat.struggle_bound.contains(&v)) {
        struggle_server(w, f);
    }
}

/// The component's ReceiveTick @12446 (authority) -> @4837: the loop over HeldFighters by index; a fighter without a
/// Buff_JakEat (or not a fighter) -> RemoveHeldFighter @4763. The loop reads the length each pass, so a removal skips
/// the fighter that took its index.
fn component_tick(w: &mut World, f: FighterId, _dt: Real) {
    let mut i = 0;
    while i < jake(w, f).held_fighters.len() {
        let v = jake(w, f).held_fighters[i];
        if buff_of(w, v, name!("Buff_JakEat_C")).is_none() {
            remove_held_fighter(w, f, v);
        }
        i += 1;
    }
}

/// eat.rs's part of the joined matchReset (Comp_JakeEat ReceiveBeginPlay: RemoveHeldFighterVisuals @3549), after the
/// new
/// state.
pub fn match_reset(w: &mut World, f: FighterId) {
    visuals_off(w, f);
}

/// The hooks this file joins in Jake's CharHooks (`component_tick`, `timer_tick`, `struggling`; `match_reset` is
/// mod.rs's).
pub fn hooks(h: &mut CharHooks) {
    h.component_tick = Some(component_tick);
    h.timer_tick = Some(timer_tick);
    h.struggling = Some(struggling);
}
