//! The eat: Pawn_C015's EatenFighters map (AddFighterToEat, RemoveFighterFromEat),
//! the chew (AnimNotify_C015Chew -> BPI_ChewOnEatenFighters -> CalculateEatValuesFromFighters), the spit
//! (AnimNotify_C015SpitFighter -> BPI_SpitFighters -> CalculateSpitFighters, OffsetSpitFighter), the hold buff
//! Buff_EatenByC015 (a Buff_HeldByEnemy child, `combat::holds`) and the eat conditions. Offsets are Pawn_C015's
//! ubergraph's unless a function is named.
//!
//! Not ported, no caller in this build: BPI_ClearEatenFighters (only AnimNotify_C015HandleEatenFighters, a notify no
//! montage of the kit plays), the ally eat (OnAllyInputTriggered @13869-14120, behind BPI_SetAllyCanBeEaten),
//! BPI_AddEatFromFighterGrapple, SpawnEscapeServer, Buff_EatenByC015InAir. With the ally eat dormant, the eat's
//! Hitframes give Buff_EatenByC015 to an enemy alone (ApplyBuffEnemy), so the map never holds a team mate: the ally
//! branches of the chew and the spit warn.

use super::pawn::{
    eat_meter_value, has_eaten_ally, has_eaten_fighters, is_eating_projectile, stored_projectile_class,
    update_eat_level,
};
use super::{taz, taz_mut, taz_of, taz_w};
use crate::Real;
use crate::combat::buffs::{buff_of, remove_buff};
use crate::combat::hits::direct_hit;
use crate::combat::holds::{Hold, HoldHit, HoldKind, consume_attached, raw_half};
use crate::combat::{DataVec, HitPacket};
use crate::events::NotifyRef;
use crate::fighter::anim::{add_invisibility, remove_invisibility};
use crate::fighter::common::data_of;
use crate::fighter::control::trigger_ability;
use crate::fighter::moves::sweep_move;
use crate::fighter::{FighterId, active_montage_name, actor_y, hold_def, in_hitstun, warn};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks, Condition};
use crate::registry::props::{arr, num, obj};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

use super::state::{EatenFighter, SpitOffset};
use crate::combat::BuffClass;

// ---- EatenFighters

/// Map_Add on EatenFighters, and the order Map_Keys gives (every loop of the pawn over the map takes the keys first):
/// the pairs in the order of their index in the map's sparse element array. An add takes the last freed index, else a
/// new one at the end; a remove frees its index. So a fighter eaten after another was spat out comes first if it took a
/// freed index ahead of one still held.
// Unverified: the TMap / TSparseArray rule (Map_Add takes the free list's head, RemoveAt pushes on it) is the stock
// 4.26 container's, not read in the exe; with HitOnlyOneFighter on both eat Hitframes the map holds two fighters only
// in a 2v2 where two eats overlap.
fn map_add(w: &mut World, f: FighterId, vic: FighterId, v: Real) {
    let s = taz_w(w, f);
    let slot = match s.eat_free.pop() {
        Some(i) => i,
        None => {
            s.eat_slots += 1;
            s.eat_slots - 1
        }
    };
    let i = s.eaten_fighters.iter().position(|e| e.slot >= slot).unwrap_or(s.eaten_fighters.len());
    s.eaten_fighters.insert(i, EatenFighter { f: vic, v, slot });
}

/// `addFighterToEat` (BPI_AddFighterToEat(Fighter) @4798-5436): a key already in the map: nothing (@4858).
/// BPI_CheckIfHasBuff(Buff_Tasty) @4952 is read and not used: Map_Add @5086 adds the fighter with Temp_int_Variable,
/// which nothing writes (0). Then on authority, the fighter's State Manager valid: a gate -> Delay(0.2) @5336 ->
/// EatenFightersText @568 (an empty function) and the gate reopened: nothing to port.
pub fn add_fighter_to_eat(w: &mut World, f: FighterId, vic: FighterId) {
    let Some(s) = taz_of(w, f) else { return };
    if s.eaten_fighters.iter().any(|e| e.f == vic) {
        return;
    }
    map_add(w, f, vic, 0.0);
}

/// `removeFighterFromEat` (BPI_RemoveFighterFromEat(Fighter) @4573): Map_Remove; the branch on Keys.Length <= 0 @4787
/// is empty.
pub fn remove_fighter_from_eat(w: &mut World, f: FighterId, vic: FighterId) {
    let Some(s) = taz_mut(w, f) else { return };
    let Some(i) = s.eaten_fighters.iter().position(|e| e.f == vic) else { return };
    let slot = s.eaten_fighters[i].slot;
    s.eat_free.push(slot);
    s.eaten_fighters.remove(i);
}

// ---- The chew

fn dv(x: Real, y: Real) -> Value {
    let mut o = Props::new();
    o.insert("X".into(), Value::Num(f64::from(x)));
    o.insert("Y".into(), Value::Num(f64::from(y)));
    Value::Obj(o)
}

/// The Struct_HitBoxDamage CalculateEatValuesFromFighters makes for an enemy (@2160-3287): Damage BaseChewDamage + 0,
/// no knockback, no direction, HitReaction 0, no hit pause, HitStunMult 0, PlayOnHitEffects and
/// ApplyHitPauseToDamageDealer true, AccountForVelocityWithVacuumKnockback true, KBI_Multiplier 1, the VacuumInfo
/// default but AccountForAttackerVelocity; every other field its zero.
fn chew_packet(w: &World, f: FighterId, rows: Option<Value>) -> Props {
    let base = num(&data_of(w, f).fighter.pawn, "BaseChewDamage").unwrap_or(2.0);
    let mut p = Props::new();
    let mut put = |k: &str, v: Value| {
        p.insert(k.to_owned(), v);
    };
    put("Damage", Value::Num(f64::from(base + 0.0)));
    put("BaseKnockBack", Value::Num(0.0));
    put("KnockBackScaling", Value::Num(0.0));
    put("KnockBackDirectionVector", dv(0.0, 0.0));
    put("AttackTag", Value::Str("None".into()));
    put("HitReaction", Value::Num(0.0));
    put("HitPauseTime", Value::Num(0.0));
    put("HitStunMult", Value::Num(0.0));
    put("OnHitAudio", Value::Null);
    put("OnHitEffectOverride", Value::Null);
    put("AttackID", Value::Num(0.0));
    put("PlayOnHitEffects", Value::Bool(true));
    put("ApplyHitPauseToDamageDealer", Value::Bool(true));
    put("AttackDecayType", Value::Num(0.0));
    put("AttackDecayName", Value::Str("None".into()));
    put("AdditiveKBMode", Value::Num(0.0));
    put("KBScalingStartingDmg", Value::Num(0.0));
    put("HitEachActorOnlyOnce", Value::Bool(false));
    put("AccountForVelocityWithVacuumKnockback", Value::Bool(true));
    put("IgnoreKBI", Value::Bool(false));
    put("KBI_Multiplier", Value::Num(1.0));
    let mut vac = Props::new();
    vac.insert("UseVacuumKnockBack".into(), Value::Bool(false));
    vac.insert("VacuumStrengthAlpha".into(), Value::Num(0.0));
    vac.insert("VacuumTowardsInstigatingFighter".into(), Value::Bool(false));
    vac.insert("VacuumTargetSocket".into(), Value::Str("None".into()));
    vac.insert("VacuumSocketOffset".into(), dv(0.0, 0.0));
    vac.insert("VacuumDuration".into(), Value::Num(0.0));
    vac.insert("VacuumPause".into(), Value::Num(0.0));
    vac.insert("ForceSmallHitEffects".into(), Value::Bool(false));
    vac.insert("AccountForAttackerVelocity".into(), Value::Bool(true));
    put("VacuumInfo", Value::Obj(vac));
    if let Some(r) = rows {
        put("OnHitBuffs", r);
    }
    p
}

/// `chewOnEatenFighters` (BPI_ChewOnEatenFighters(WeakStacks) @17208 -> @17028): on authority,
/// CalculateEatValuesFromFighters(WeakStacks) and UpdateEatLevelServer(its EatValueAddition, true, true) @17090.
/// CalculateEatValuesFromFighters: over Map_Keys(EatenFighters), a key with a valid Comp_Actor_Destructible @394:
/// - another team (@596 -> @1848): ChewDebuffs.StackCount = WeakStacks (a write of the pawn's variable, read only
///   here); the fighter's BPI_CheckIfHasBuff(Buff_Chicken) @2022; Comp_Pawn_HitBoxManager.pfgDoDamage(null, fighter, the
///   packet, OnHitBuffs = has the chicken ? ChewOnChickenDebuffs (Buff_Weakened, 5 stacks) : [ChewDebuffs]) @3462; then
///   LocalEatGained += Lerp(MinEatFromChewyingEnemy, MaxEatFromChewyingEnemy, FClamp(the map value / 10, 0, 1)) @3864.
///   Both are float properties absent from the class defaults (0): the chew adds nothing to the meter.
/// - the same team (@610-1667): ApplyDamage on Taz and a heal of the ally, EatFromChewyingAlly; never reached (above).
///   EatValueAddition = FClamp(LocalEatGained, 0, 1) @1742.
// Deviation: the pfgDoDamage of a notify on a held fighter goes through `holds.hold_hits` (`combat::holds`
// deal_hold_hits); the build deals it inside the notify.
pub fn chew_on_eaten_fighters(w: &mut World, f: FighterId, weak_stacks: Real) {
    let list: Vec<(FighterId, Real)> = taz(w, f).eaten_fighters.iter().map(|e| (e.f, e.v)).collect();
    let mut gained: Real = 0.0;
    for (vic, v) in list {
        if w.fighter(vic).team.team == w.fighter(f).team.team {
            warn(
                w,
                f,
                name!("taz: an ally in EatenFighters (the chew's ally branch): not ported, the ally eat is dormant"),
            );
            continue;
        }
        let pawn = &data_of(w, f).fighter.pawn;
        let mut chew = obj(pawn, "ChewDebuffs").cloned().unwrap_or_default();
        chew.insert("StackCount".into(), Value::Num(f64::from(weak_stacks)));
        let rows = if buff_of(w, vic, name!("Buff_Chicken_C")).is_some() {
            pawn.get("ChewOnChickenDebuffs").cloned()
        } else {
            let mut o = Props::new();
            o.insert("OnHitBuffs".into(), Value::Arr(vec![Value::Obj(chew)]));
            Some(Value::Obj(o))
        };
        let (lo, hi) =
            (num(pawn, "MinEatFromChewyingEnemy").unwrap_or(0.0), num(pawn, "MaxEatFromChewyingEnemy").unwrap_or(0.0));
        let p = chew_packet(w, f, rows);
        w.fighter_mut(f).holds.hold_hits.push(HoldHit { vic, p });
        let a = min(max(v / 10.0, 0.0), 1.0);
        gained += lo + (hi - lo) * a;
    }
    update_eat_level(w, f, min(max(gained, 0.0), 1.0), true, true);
}

// ---- The spit

fn is_dead(w: &World, f: FighterId) -> bool {
    let d = w.fighter(f).life.dead;
    d != 0.0 && !d.is_nan()
}

/// `spitFighters` (BPI_SpitFighters(Hit) @11228): on authority, CalculateSpitFighters(Hit). Over Map_Keys(EatenFighters),
/// a key with a valid Comp_Actor_Destructible @394:
/// - another team (@1738-2731): LocalHit = Hit; the map value >= 5 (never: the value is 0) adds SpitDamageFullTasty to
///   Damage and SpitScalingFullTasty to KnockBackScaling and sets LocalSpitFullTastyFighter; OnHitAudio =
///   sfx_cha_jake_sweetspot_hit and OnHitEffectOverride = PS_HitImpact_Big then, else both null (@2528 / @2600: the
///   notify's own sound and effect are overwritten); pfgDoDamage(null, fighter, LocalHit) @2731; then as below.
/// - both (@610-1289): the fighter's State Manager valid -> RemoveBuff(Buff_EatenByC015) @869 (its EndPlay inside the
///   call: RemoveFighterFromEat, the hitstun test before the EarlyBreak hit); Direction = (0, LocalHit.KnockBackDirection
///   .X x (FacingLeft ? 1 : -1), .Y) @998-1179; OffsetSpitFighter(fighter, Direction) @1289.
///   The team mate's branch takes LocalHit as it stands (zero, or the last enemy's); never reached (above).
///   After the loop, LocalSpitFullTastyFighter and Taz's GetAllies not empty: a C015_HealPickup at Taz @1545 (never).
///   The call is made by the World after the frame's mesh ticks (`spit_tick`, from mod.rs world_tick): the hit has to
///   come ahead of the hold's end, whose EndPlay reads the hitstun the hit started, so it cannot wait for `hold_hits`.
// Deviation: the build makes it inside the notify.
pub fn spit_fighters(w: &mut World, f: FighterId, hit: Props) {
    let list: Vec<(FighterId, Real)> = taz(w, f).eaten_fighters.iter().map(|e| (e.f, e.v)).collect();
    let mut local = Props::new();
    local.insert("KnockBackDirectionVector".into(), dv(0.0, 0.0));
    let mut full = false;
    for (vic, v) in list {
        if w.fighter(vic).team.team != w.fighter(f).team.team {
            local = hit.clone();
            let big = v >= 5.0;
            full = full || big;
            let (scaling, damage) = {
                let pawn = &data_of(w, f).fighter.pawn;
                (num(pawn, "SpitScalingFullTasty").unwrap_or(5.0), num(pawn, "SpitDamageFullTasty").unwrap_or(5.0))
            };
            let ks = num(&local, "KnockBackScaling").unwrap_or(0.0);
            local.insert("KnockBackScaling".into(), Value::Num(f64::from(ks + if big { scaling } else { 0.0 })));
            let dm = num(&local, "Damage").unwrap_or(0.0);
            local.insert("Damage".into(), Value::Num(f64::from(dm + if big { damage } else { 0.0 })));
            if !is_dead(w, vic) && !is_dead(w, f) {
                let mut hp = HitPacket::from_props(&w.names, &local, None);
                hp.on_hit_audio = big.then(|| name!("sfx_cha_jake_sweetspot_hit"));
                hp.on_hit_effect_override = big.then(|| name!("PS_HitImpact_Big"));
                hp.knock_back_direction = hp.knock_back_direction_vector;
                hp.on_hit_sound = hp.on_hit_audio;
                direct_hit(w, f, vic, &hp);
            }
        } else {
            warn(
                w,
                f,
                name!("taz: an ally in EatenFighters (the spit's ally branch): not ported, the ally eat is dormant"),
            );
        }
        remove_buff(w, vic, Some(name!("Buff_EatenByC015_C")), None);
        let d = obj(&local, "KnockBackDirectionVector");
        let (dx, dy) = (d.and_then(|d| num(d, "X")).unwrap_or(0.0), d.and_then(|d| num(d, "Y")).unwrap_or(0.0));
        // UE Y = X x (FacingLeft ? 1 : -1); sim x is -UE Y, and the facing is -1 facing left: X x facing.
        let facing = w.fighter(f).body.facing_dir;
        offset_spit_fighter(w, f, vic, dx * facing, dy);
    }
    if full {
        warn(w, f, name!("taz: CalculateSpitFighters full-Tasty heal pickup: not ported (the map value is never 5)"));
    }
}

/// OffsetSpitFighter(Target, Direction) @17863, a multicast: Delay(0.0) -> @605 Target.K2_SetActorLocation(Direction x
/// 200 + (0, Taz's Y, Z), sweep, no teleport) @818: the direction is not normalised. A second call while the Delay is
/// pending only rewrites the event's Target and Direction (the latent node is ignored): the last one is moved.
fn offset_spit_fighter(w: &mut World, f: FighterId, vic: FighterId, dx: Real, dy: Real) {
    taz_w(w, f).spit_offset = Some(SpitOffset { vic, dx, dy });
}

/// `spitOffsetTick`: the Delay(0.0) made in the pawn's mesh tick, after its actor tick: taken as made on an object with
/// no latent entry at its tick of the frame, which fires in that frame's leftover latent pass: the World's timer pass
/// (mod.rs timer_tick), ahead of the pawn's timers.
// Unverified: whether Taz has a latent entry at his tick then (a pending Delay of BaseFighter's graph or his own) is
// not read; with one the move waits for his tick of the next frame.
pub fn spit_offset_tick(w: &mut World, f: FighterId) {
    let Some(o) = taz_w(w, f).spit_offset.take() else { return };
    let x = w.fighter(f).body.x + (o.dx * 200.0);
    let y = actor_y(w, f) + (o.dy * 200.0);
    sweep_move(w, o.vic, x, y);
}

/// `spitTick`: the World's call of this frame's BPI_SpitFighters (mod.rs world_tick).
pub fn spit_tick(w: &mut World, f: FighterId) {
    let Some(n) = taz_w(w, f).spit_hit.take() else { return };
    let p = crate::fighter::match_side::notify_props(w, f, n);
    let hit = obj(p, "Hit").cloned().unwrap_or_else(|| {
        let mut o = Props::new();
        o.insert("KnockBackDirectionVector".into(), dv(0.0, 0.0));
        o
    });
    spit_fighters(w, f, hit);
}

// ---- Buff_EatenByC015 (a Buff_HeldByEnemy child, `combat::holds`)

/// Buff_EatenByC015_C's class.
const EATEN: crate::names::Name = name!("Buff_EatenByC015_C");

/// SetActorEnableCollision(false) on the owner in its BeginPlay @421 and (true) in its EndPlay @2232: the live entry
/// turns the owner's whole collision off (`buff_collision_off`).
// Deviation: the entry is live from SpawnBuffActor's list on, a step ahead of the parent's BeginPlay; nothing between
// reads the collision.
fn collision_off(_w: &World, _f: FighterId, _b: crate::combat::buffs::BuffRef) -> bool {
    true
}

/// On a spawned actor (a cast of the owner to BP_FighterCharacter fails): K2_DestroyActor @1921 before the parent's
/// BeginPlay, so the buff ends in its spawn and does nothing.
fn actor_begin(_w: &mut World, _a: crate::actors::ActorRef, _i: u32, _from: Option<FighterId>) -> bool {
    true
}

/// ReceiveBeginPlay @30-248, ahead of the parent's @275: the owner a BP_FighterCharacter (it always is); AttachOffset =
/// (0, 0, its CapsuleComponent.GetUnscaledCapsuleHalfHeight) (the field, written by a crouch: `raw_half`).
fn before_begin(w: &mut World, vic: FighterId, h: &mut Hold) -> bool {
    h.offset = Some(DataVec { x: Some(0.0), y: Some(0.0), z: Some(raw_half(w, vic)) });
    false
}

/// After the parent's ReceiveBeginPlay: Valid = true @285 (`applied`); the instigator's Comp_Pawn_AbilityManager
/// TriggerAbilityActual(Slot Param) @367 (the class's own int, absent from its defaults: 0); collision off (the entry
/// above); BPI_GetUIOffset / BPI_SetUIRelativeLocationTop / Bottom @533-1148 (the eaten fighter's HUD anchor: the
/// HUD's); the instigator's BPI_AddFighterToEat(owner) @1313; on authority the owner's and the instigator's
/// OnExitArenaWarning -> OnExitBlastBoxWarning @1393 / @1556 (`warning` below); not a dedicated server: the owner's
/// Comp_FighterVisibilityManager AddInvisibility @1861 and DestroyAttachedConsumableActors(owner) @1897
/// (`consume_attached`).
fn begin(w: &mut World, vic: FighterId) {
    let Some(h) = &mut w.fighter_mut(vic).holds.hold else { return };
    h.applied = true;
    let by = h.by;
    trigger_ability(w, by, 0);
    add_fighter_to_eat(w, by, vic);
    add_invisibility(w, vic);
    if let Some(h) = &mut w.fighter_mut(vic).holds.hold {
        consume_attached(h);
    }
}

/// ReceiveTick @4312: the parent's, then on authority @3925-4292: the instigator a Character, its anim instance's
/// GetCurrentActiveMontage (none: not in the list) not in ValidEatMontages (the two eats and the five throws) ->
/// K2_DestroyActor @15. ValidEatMontages of the class defaults, by montage name.
fn tick(w: &mut World, vic: FighterId) -> bool {
    let Some(h) = &w.fighter(vic).holds.hold else { return false };
    let by = h.by;
    let Some(cur) = active_montage_name(w, by) else { return true };
    let cur = w.names.str(cur);
    let valid = arr(hold_def(w, vic, EATEN), "ValidEatMontages").iter().any(|r| {
        let p = r.as_obj().and_then(|o| o.get("AssetPathName")).and_then(Value::as_str).unwrap_or("");
        p.rsplit_once('.').map_or(p, |(_, t)| t) == cur
    });
    !valid
}

/// ReceiveEndPlay @2183, with Valid: the parent's @2193 first (`hold_end`), collision on (the entry's end), the UI
/// offsets put back @2369 / @2514, the instigator's BPI_RemoveFighterFromEat(owner) @2679, on authority the warning
/// bindings removed @2854 / @3017, then the owner's HitstunHandler CheckHitstunActive @3262: not in hitstun -> the
/// instigator's Comp_Pawn_HitBoxHandler.pfgDoDamage(null, owner, EarlyBreakKnockback) @3801 (`hold_hits`: the World
/// deals it after the frame's ticks). Either way, not a dedicated server: RemoveInvisibility @3563.
/// The parent's end has ended the hold's buff stun already (its UpdatePlayerStatus), so a fighter let go by the
/// montage's end, the struggle or the instigator's hit is popped out by EarlyBreakKnockback (0 damage, 750 base, (0.5,
/// 1)); one the spit's hit put in hitstun is not.
fn end(w: &mut World, vic: FighterId, h: &Hold) {
    if !h.applied {
        return;
    }
    remove_fighter_from_eat(w, h.by, vic);
    if !in_hitstun(w, vic) {
        let p = obj(hold_def(w, vic, EATEN), "EarlyBreakKnockback").cloned();
        if let Some(p) = p {
            w.fighter_mut(h.by).holds.hold_hits.push(HoldHit { vic, p });
        }
    }
    remove_invisibility(w, vic);
}

/// OnExitBlastBoxWarning @4297 (a reliable server RPC): K2_DestroyActor. `holds::hold_arena_warning` skips the owner's
/// own broadcast while its collision is off (the whole time this buff lives), so only Taz's reaches it.
fn warning(_w: &mut World, _held: FighterId, _from: FighterId) -> bool {
    true
}

/// Buff_EatenByC015_C's kind (`combat::holds::hold_kind`).
pub static KIND: HoldKind = HoldKind {
    before_begin: Some(before_begin),
    begin: Some(begin),
    tick: Some(tick),
    end: Some(end),
    track: None,
    warning: Some(warning),
    exited_arena: None,
    latent: None,
};

// ---- The notifies

/// AnimNotify_C015Chew Received_NotifyBegin @10-215: the owner implements BPI_C015 -> BPI_ChewOnEatenFighters(Weak
/// Stacks) (class default 1). No NotifyEnd.
fn chew_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if taz_of(w, f).is_none() {
        return;
    }
    let stacks = num(crate::fighter::match_side::notify_props(w, f, n), "Weak Stacks").unwrap_or(1.0);
    chew_on_eaten_fighters(w, f, stacks);
}

/// AnimNotify_C015SpitFighter Received_NotifyBegin @10-215: the owner implements BPI_C015 -> BPI_SpitFighters(Hit) (the
/// throws' own Hit; the class default's is all zero). No NotifyEnd. The call is run by the World (`spit_tick`).
fn spit_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let Some(s) = taz_mut(w, f) else { return };
    let had = s.spit_hit.replace(n).is_some();
    if had {
        warn(w, f, name!("taz: two BPI_SpitFighters calls in one frame: the first is dropped"));
    }
}

// ---- The conditions (Condition2 Check(actor); the actor is the montage's owner)

// The meter conditions (C015EatEmpty, C015EatUnder25, C015EatMedium): DoesImplementInterface(BPI_C015) false -> false;
// else BPI_GetEatMeterValue (a failed cast after it: 0, which no fighter of the port reaches). C015EatMedium tests
// first that the actor's State Manager has no Buff_C015_EatProjectiles (the EatProjectiles perk's marker).
fn eat_empty(w: &mut World, f: FighterId) -> bool {
    eat_meter_value(w, f).is_some_and(|v| v <= 0.0)
}

fn eat_under_25(w: &mut World, f: FighterId) -> bool {
    eat_meter_value(w, f).is_some_and(|v| v < 0.25)
}

fn eat_medium(w: &mut World, f: FighterId) -> bool {
    let Some(v) = eat_meter_value(w, f) else { return false };
    if buff_of(w, f, name!("Buff_C015_EatProjectiles_C")).is_some() {
        return false;
    }
    (0.5..0.99).contains(&v)
}

/// C015EatLarge: BPI_GetStoredProjectile's class == C015Anvil (the EatProjectiles perk's store; the debug command's is
/// not ported).
fn eat_large(w: &mut World, f: FighterId) -> bool {
    taz_of(w, f).is_some() && stored_projectile_class(w, f) == name!("C015Anvil_C")
}

/// C015EatStored: the stored class is neither C015Trash nor C015Anvil.
fn eat_stored(w: &mut World, f: FighterId) -> bool {
    if taz_of(w, f).is_none() {
        return false;
    }
    let c = stored_projectile_class(w, f);
    c != name!("C015Trash_C") && c != name!("C015Anvil_C")
}

/// C015EatMiss: !BPI_HasEatenFighters && !BPI_IsEatingProjectile, each false for an actor without BPI_C015.
fn eat_miss(w: &mut World, f: FighterId) -> bool {
    taz_of(w, f).is_none() || (!has_eaten_fighters(w, f) && !is_eating_projectile(w, f))
}

/// C015HasEatenFightersCondition / C015NoEatenFightersCondition / C015EatenAlly_Condition: no BPI_C015 -> false; else
/// BPI_HasEatenFighters, its Not, BPI_HasEatenAlly.
fn has_eaten(w: &mut World, f: FighterId) -> bool {
    taz_of(w, f).is_some() && has_eaten_fighters(w, f)
}

fn no_eaten(w: &mut World, f: FighterId) -> bool {
    taz_of(w, f).is_some() && !has_eaten_fighters(w, f)
}

fn eaten_ally(w: &mut World, f: FighterId) -> bool {
    taz_of(w, f).is_some() && has_eaten_ally(w, f)
}

/// This file's registry entries (Buff_EatenByC015_C, the two notifies, the C015Eat* conditions), in registration order:
/// the buff, the hold (`hold_kind`), the notifies, the conditions.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        "Buff_EatenByC015_C",
        BuffEntry {
            actor: Some(ActorBuffHooks { begin: Some(actor_begin), ..ActorBuffHooks::default() }),
            ..BuffEntry::new(
                BuffClass::BuffEatenByC015,
                BuffHooks { actor_collision_off: Some(collision_off), ..BuffHooks::default() },
            )
        },
    )?;
    type Fns = Option<fn(&mut World, FighterId, NotifyRef, i32)>;
    let be = |begin: Fns| NotifyHandler { begin, ..NotifyHandler::default() };
    r.notifies.enter("AnimNotify_C015Chew_C", be(Some(chew_begin)))?;
    r.notifies.enter("AnimNotify_C015SpitFighter_C", be(Some(spit_begin)))?;
    for (name, f) in [
        ("C015EatEmpty_C", eat_empty as crate::registry::entries::CondFn),
        ("C015EatUnder25_C", eat_under_25),
        ("C015EatMedium_C", eat_medium),
        ("C015EatLarge_C", eat_large),
        ("C015EatStored_C", eat_stored),
        ("C015EatMiss_C", eat_miss),
        ("C015HasEatenFightersCondition_C", has_eaten),
        ("C015NoEatenFightersCondition_C", no_eaten),
        ("C015EatenAlly_Condition_C", eaten_ally),
    ] {
        r.conditions.enter(name, Condition::Fn(f))?;
    }
    Ok(())
}
