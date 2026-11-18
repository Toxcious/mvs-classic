//! AbilityRefundOnDodge, DodgeArmor, DodgeReflect, AbilityRefundOnTakeProjectileKB, OnKnockbackProjectileBlock and
//! Buff_ProjectileBlock, the shared class School Me Once grants.
//! Read: Buff_Perk_AbilityRefundOnDodge, Buff_DodgeArmor, Buff_DodgeReflect, Buff_Perk_AbilityRefundOnTakeProjectileKB,
//! Buff_OnKnockbackProjectileBlock, Buff_ProjectileBlock.
//! The signals: evadedAttack / afterEvadedAttack (dodgeTail), evadeTriggered (evadeServer),
//! knockedBack with its DamageDealer (receiveHit), hitByEnemy with its DamageDealer
//! (hitByEnemy). Every Begin's HasAuthority holds (the sim is the authority) and its owner is a
//! BP_FighterCharacter with every component the graphs look for (DamageManager, Destructible, AbilityManager, Armor,
//! HitBoxHandler, StateManager).

use super::hooks::{NO_HOOKS, armor_pack, sound};
use super::{PerkExt, def, enter, ext, ext_mut, kind, num, set_ext, stacks};
use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_dead, actor_place};
use crate::combat::buffs::{apply_dealt_damage_mods, buff_signal, remove_stack, spawn_buff};
use crate::combat::defense::add_timed_armor_pack;
use crate::combat::melee_actors::{fighter_hits_actor, fighter_hits_item, fighter_hits_projectile};
use crate::combat::{BuffClass, BuffRef, DataVec, HitPacket};
use crate::fighter::FighterId;
use crate::fighter::common::{latent_frames, peers, timer_frames, warn_once};
use crate::fighter::control::update_ability_cooldown;
use crate::fighter::moves::V2;
use crate::name;
use crate::registry::entries::BuffHooks;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Value;

/// Enum_InputDirection8Way Neutral.
const NEUTRAL: Real = 4.0;
const DEFAULT_DIR: Real = 5.0;

// ---------------------------------------------------------------- the refunds
// Both refund graphs (AbilityRefundOnDodge @2003-2229 -> @1543-1589 -> @1031-1398 -> @1399-1538 -> @15-957;
// AbilityRefundOnTakeProjectileKB @2634-2883 -> @962-1422 -> @1427-1566 -> @15-957): AbilityRefundPercent =
// SelectFloat(Stacked, Unstacked, CurrentStackAmount >= 2); a valid owner AbilityManager;
// UpdateAbilityCooldownServer(i, false, 0.0, percent) for every index of its Abilities @1338 / @1292, then the owner
// Destructible's GetAllies and, for each one with an AbilityManager, every index of its Abilities @753
// (updateAbilityCooldown). GetAllies: the other fighters of the team not knocked out, in the
// sim's order (as the ally reads, unverified).
fn refund_all(w: &mut World, f: FighterId, b: BuffRef) {
    let d = def(w, b);
    let k = if stacks(w, b) >= 2.0 { "StackedAbilityRefundPercent" } else { "UnstackedAbilityRefundPercent" };
    let pct = num(d, k);
    refund_slots(w, f, pct);
    let team = w.fighter(f).team.team;
    // `for (const o of f.peers)`: by place, as a refund's handlers may write the World.
    for i in 0..peers(w).len() {
        let o = peers(w)[i];
        if o == f || w.fighter(o).team.team != team || w.fighter(o).life.dead > 0.0 {
            continue;
        }
        refund_slots(w, o, pct);
    }
}

fn refund_slots(w: &mut World, f: FighterId, pct: Real) {
    let n = w.fighter(f).abilities.abilities.len();
    for i in 0..n {
        let slot = i32::try_from(i).expect("an ability slot");
        update_ability_cooldown(w, f, slot, false, 0.0, pct);
    }
}

fn refund_on_dodge(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    // ReceiveBeginPlay @1594 binds the owner's EvadedAttack @1729; OnEvadedAttack -> @2003 (no gate on the attacker).
    if s == BuffSignal::EvadedAttack {
        refund_all(w, f, b);
    }
    false
}

// ReceiveBeginPlay @1847 binds the owner DamageManager's KnockedBack @2017 and keeps Team Index @2313.
// OnOwnerKnockedBack @2363: a valid dealer Destructible, its TeamIndex != Team Index and its IsProjectile
// (BPI_Projectiles alone does not do) @2443-2624; a dealer with no Destructible (a buff actor's handler: no `by`)
// fails.
fn refund_on_kb(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(by) = a.by else { return false };
    if s != BuffSignal::KnockedBack || by.team == w.fighter(f).team.team || !by.is_projectile {
        return false;
    }
    refund_all(w, f, b);
    false
}

// ---------------------------------------------------------------- the evade direction (DodgeArmor, DodgeReflect)
// OnEvade (the owner's EvadeTriggered, bound @489): Direction = the evade's @1549 / @1263, then Delay(the event's
// InvincibilityDuration) @1576 / @1290 resuming @15: Direction = 5 (Default). One latent action per buff: a second
// evade while it is pending writes the direction and does not move its end. The class default is 5.
// Unverified: the Delay counts in the buff's tick from the next one (combat/buffs tick_buffs), as the port's other
// buff Delays; an InvincibilityDuration of 0 resumes at the next tick.
fn dir_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    let dir = def(w, b).num("Direction").unwrap_or(DEFAULT_DIR);
    set_ext(w, b, PerkExt::Dir { dir, left: 0.0 });
}

fn dir_evade(w: &mut World, b: BuffRef, a: &BuffSignalArg) {
    let n = a.dir.unwrap_or(DEFAULT_DIR);
    let frames = latent_frames(a.duration.unwrap_or(0.0));
    if let Some(PerkExt::Dir { dir, left }) = ext_mut(w, b) {
        *dir = n;
        if *left <= 0.0 {
            *left = frames;
        }
    }
}

fn dir_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    if let Some(PerkExt::Dir { dir, left }) = ext_mut(w, b)
        && *left > 0.0
    {
        *left -= 1.0;
        if *left == 0.0 {
            *dir = DEFAULT_DIR;
        }
    }
    false
}

fn dir_of(w: &World, b: BuffRef) -> Real {
    match ext(w, b) {
        Some(PerkExt::Dir { dir, .. }) => *dir,
        _ => DEFAULT_DIR,
    }
}

// ReceiveBeginPlay @290 binds EvadedAttack @425 and EvadeTriggered @489, TeamIndex = the owner Destructible's @734.
// OnEvadedAttack @808: a valid attacker Destructible @888; its TeamIndex != TeamIndex, (IsProjectile or
// BPI_Projectiles) and Direction == 4 (Neutral) @1214; a valid owner Comp_Armor: AddTimedArmorPack(Armor Pack,
// SelectFloat(StackedArmorDuration, UnstackedArmorDuration, CurrentStackAmount >= 2)) @1494.
fn dodge_armor_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s == BuffSignal::EvadeTriggered {
        dir_evade(w, b, a);
        return false;
    }
    let Some(by) = a.by else { return false };
    if s != BuffSignal::EvadedAttack || by.team == w.fighter(f).team.team || !by.projectile || dir_of(w, b) != NEUTRAL {
        return false;
    }
    let d = def(w, b);
    let pack = armor_pack(w, d, "Armor Pack");
    let dur = if stacks(w, b) >= 2.0 { num(d, "StackedArmorDuration") } else { num(d, "UnstackedArmorDuration") };
    add_timed_armor_pack(w, f, &pack, dur);
    false
}

// ReceiveBeginPlay @290 binds AfterEvadedAttack @425 and EvadeTriggered @489. OnEvadedAttack @554: a valid attacker
// Destructible @634, (IsProjectile or BPI_Projectiles) and Direction == 4 @862 (no team test), a valid owner
// HitBoxHandler @972: reflect_hit.
fn dodge_reflect_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s == BuffSignal::EvadeTriggered {
        dir_evade(w, b, a);
        return false;
    }
    let Some(by) = a.by else { return false };
    if s != BuffSignal::AfterEvadedAttack || !by.projectile || dir_of(w, b) != NEUTRAL {
        return false;
    }
    reflect_hit(w, f, b, by.actor);
    false
}

// ---------------------------------------------------------------- Buff_DodgeReflect's hit
// The owner's Comp_Pawn_HitBoxHandler.pfgDoDamage(null, AttackingActor, Damage To Projectile, ...) @1115, the packet's
// ReflectProjectiles = CurrentStackAmount >= 2 @1016 (Damage 1, BaseKnockBack 0, DestroyProjectiles true): the rules
// of a fighter's hit box on that actor from @15084 on (fighter_hits_projectile / fighter_hits_item /
// fighter_hits_actor): a reflect first where the packet asks it, else (or after a failed reflect: always for an
// actor of World.actors) the damage of 1 on an actor of another team, its own health / destroy rule deciding the rest.
// Deviation: the packet's KnockBackDirectionVector (1, 0) is turned by the owner's facing as a Hitframe's
// KnockBackDirection is (IgnoreFacing false); pfgDoDamage's tests ahead of @15084 (CheckIfActorHasBeenHit with
// HitEachActorOnlyOnce, DoLOSCheck false) are not run.
// The attacker passed the gate @634-862 with a valid Comp_Actor_Destructible: a buff actor's own handler (no
// Destructible) never reaches here (its DodgeBy has projectile false). Another kind of the port's lists (a marker,
// the mortar aim, the aim punch) is warned and not hit.
// LocalHitLocation of the call with no HitBox: the HitActor's BPI_GetRootCapsule location, else (0, 0, 0)
// (Comp_Pawn_HitBoxHandler @21042-21260; SpawnedActorParent returns its Capsule, @1467): the actor's place.
fn reflect_hit(w: &mut World, f: FighterId, b: BuffRef, actor: ActorRef) {
    if !matches!(actor, ActorRef::Projectile(_) | ActorRef::Item(_) | ActorRef::Actor(_)) {
        warn_once(
            w,
            name!(
                "Buff_DodgeReflect_C: the hit on an actor is not ported (a Projectile, an Item or a World.actors kind only)"
            ),
        );
        return;
    }
    let base = def(w, b).obj("Damage To Projectile");
    // The packet in the field names of a Hitframe's (KnockBackDirection), then the owner's dealt pass @5617-5904 with
    // the actor as Victim (no HitBox: pfgDoDamage(null, ...)), ahead of the direction and of @15084.
    let mut q = base.map_or_else(HitPacket::default, |p| HitPacket::from_props(&w.names, p, None));
    q.knock_back_direction =
        Some(q.knock_back_direction_vector.unwrap_or(DataVec { x: Some(1.0), y: Some(0.0), z: None }));
    q.reflect_projectiles = Some(stacks(w, b) >= 2.0);
    let p = apply_dealt_damage_mods(w, f, actor, &q, false);
    let kb = p.knock_back_direction;
    let kx = kb.and_then(|k| k.x).unwrap_or(1.0);
    let dx = kx * w.fighter(f).body.facing_dir;
    let dy = kb.and_then(|k| k.y).unwrap_or(0.0);
    if actor_dead(w, actor) {
        return;
    }
    match actor {
        // no HitBox: HitAnything's HitBoxInfo.HitBox invalid
        ActorRef::Projectile(pr) => fighter_hits_projectile(w, f, pr, &p, dx, dy, false, 0.0),
        ActorRef::Item(it) => {
            // (a dodged item hit: a thrown one)
            if crate::actors::item::is_owned(w, it) {
                let at = place(w, actor);
                fighter_hits_item(w, f, it, &p, -dx, dy, 0.0, false, at);
            }
        }
        ActorRef::Actor(id) => {
            let at = place(w, actor);
            fighter_hits_actor(w, f, id, &p, at, false, 0.0);
        }
        _ => {}
    }
}

/// The actor's place (its root capsule's location), (0, 0) for a kind that keeps none.
fn place(w: &World, a: ActorRef) -> V2 {
    actor_place(w, a).map_or(V2 { x: 0.0, y: 0.0 }, |(x, y, _)| V2 { x, y })
}

// ---------------------------------------------------------------- Buff_OnKnockbackProjectileBlock
// ReceiveBeginPlay @481 (authority, a valid owner DamageManager): binds its KnockedBack @651, Team Index = the owner
// Destructible's @947. OnOwnerKnockedBack @997: the DamageDealer's BPI_GetProjectileAttackType (0 when it is no
// BPI_SpawnedActor @2060), then a valid dealer Destructible @1202, its TeamIndex != Team Index, (its IsProjectile or
// BPI_Projectiles), the type != 2 (Light) @1250-1528, and a valid owner StateManager. Then with the timer not active,
// or CheckHasBuff(Buff_ProjectileBlock) false, or that buff's CurrentStackAmount < ProjectileBlockStacks: Delay(0.0)
// @2005 resuming @15: the owner's SpawnBuffActor(Buff_ProjectileBlock, the owner, the owner, 1, 0.0) @106,
// ProjectileBlockStacks = NewBuff.CurrentStackAmount @190, ProjectileBlockTimer = K2_SetTimerDelegate(
// OnProjectileBlockTimerExpired, BaseBuffDuration + (CurrentStackAmount - 1) x BuffDurationPerAdditionalStack)
// @262-453. Otherwise @2085: ProjectileBlockStacks = the buff's count, the timer cleared and set again @239 (no stack).
// OnProjectileBlockTimerExpired @2159: CheckHasBuff and BuffStacks >= ProjectileBlockStacks: its RemoveStack(1)
// @2552. ReceiveEndPlay @2594 unbinds; its timer and latent action die with the actor.
// Unverified: a spawn SweepBuffState blocks (a live Buff_ProjectileBlock that the timer test let through) returns no
// NewBuff: ProjectileBlockStacks reads 0 from None (taken as UE's Accessed None default).
// The timer is set outside the timer tick (the hit, the buff's tick): timer_frames, counted in the frame's timer pass.
fn block_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    set_ext(w, b, PerkExt::Block { stacks: 0.0, timer: 0.0, delay: 0.0 });
}

/// The owner's live Buff_ProjectileBlock and its stacks.
fn live_block(w: &World, f: FighterId) -> Option<(u32, Real)> {
    w.fighter(f)
        .buffs
        .buffs
        .iter()
        .find(|x| x.class == BuffClass::BuffProjectileBlock && x.frames > 0.0)
        .map(|x| (x.serial, x.stacks))
}

fn block_duration(w: &World, b: BuffRef) -> Real {
    let d = def(w, b);
    num(d, "BaseBuffDuration") + ((stacks(w, b) - 1.0) * num(d, "BuffDurationPerAdditionalStack"))
}

fn block_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::KnockedBack {
        return false;
    }
    let Some(by) = a.by else { return false };
    if by.team == w.fighter(f).team.team || !by.projectile || by.attack_type == 2 {
        return false;
    }
    let Some(PerkExt::Block { stacks: es, timer, .. }) = ext(w, b) else { return false };
    let (es, timer) = (*es, *timer);
    let blk = live_block(w, f);
    match blk {
        Some((_, n)) if timer > 0.0 && n >= es => {
            let t = timer_frames(Some(block_duration(w, b)));
            if let Some(PerkExt::Block { stacks, timer, .. }) = ext_mut(w, b) {
                *stacks = n;
                *timer = t;
            }
        }
        _ => {
            let d0 = latent_frames(0.0);
            // Delay(0.0): one pending latent action
            if let Some(PerkExt::Block { delay, .. }) = ext_mut(w, b)
                && *delay <= 0.0
            {
                *delay = d0;
            }
        }
    }
    false
}

fn block_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(PerkExt::Block { delay, .. }) = ext_mut(w, b) else { return false };
    if *delay <= 0.0 {
        return false;
    }
    *delay -= 1.0;
    if *delay > 0.0 {
        return false;
    }
    let nb = spawn_buff(w, f, name!("Buff_ProjectileBlock_C"), 0.0, Some(1), Some(f), true, None);
    let n = nb.and_then(|nb| super::entry(w, nb)).map_or(0.0, |x| x.stacks);
    let t = timer_frames(Some(block_duration(w, b)));
    if let Some(PerkExt::Block { stacks, timer, .. }) = ext_mut(w, b) {
        *stacks = n;
        *timer = t;
    }
    false
}

fn block_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(PerkExt::Block { timer, stacks: es, .. }) = ext_mut(w, b) else { return false };
    if *timer <= 0.0 {
        return false;
    }
    *timer -= 1.0;
    if *timer > 0.0 {
        return false;
    }
    let es = *es;
    if let Some((serial, n)) = live_block(w, f)
        && n >= es
    {
        remove_stack(w, f, BuffRef { fighter: f, serial }, 1);
    }
    false
}

// Buff_ProjectileBlock (Blueprints/Abilities/Shield). ReceiveBeginPlay @707: a valid owner DamageManager:
// HasProjectileArmor = true @908 (Invuln.projectile_armor: World.projectileShielded) and
// PF_Post_Event(sfx_cha_finn_shield_loop, the owner) @961; (not dedicated) PS_FinnShield on the owner's active mesh
// @1666 (render); authority: binds the DamageManager's HitByEnemy @1989. OnHitByEnemy @2054: a valid attacker
// Destructible, the attacker's BPI_GetProjectileAttackType (0 without BPI_SpawnedActor @2594), (its IsProjectile or
// BPI_Projectiles) and the type not in IgnoreProjectileTypes ([2, 4]: Light, Transcendent) @2545: RemoveStack(1)
// @2555 and BPI_ConditionalStackUpdate(1) @2574 (BuffParentActor @5324: broadcast ConditionalBuffUpdate, bound by
// Finn_Shop_ArmorBuns_Buff @1716 alone). ReceiveEndPlay @2619: HasProjectileArmor = false (a plain write, as
// Buff_ProjectileImunity's), the VFX destroyed, sfx_cha_finn_shield_loop_stop @2948.
fn shield_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    w.fighter_mut(f).invuln.projectile_armor = true;
    sound(w, f, name!("sfx_cha_finn_shield_loop"), None);
}

fn shield_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(by) = a.by else { return false };
    if s != BuffSignal::HitByEnemy || !by.projectile {
        return false;
    }
    let ignored = def(w, b)
        .get("IgnoreProjectileTypes")
        .and_then(Value::as_arr)
        .is_some_and(|l| l.iter().any(|v| v.as_f64() == Some(f64::from(crate::real(by.attack_type)))));
    if ignored {
        return false;
    }
    remove_stack(w, f, b, 1);
    // BPI_ConditionalStackUpdate(1) @2574 on the actor RemoveStack may have ended: its ConditionalBuffUpdate
    // broadcast reaches what bound it (BuffSignal ConditionalBuffUpdate: Finn_Shop_ArmorBuns_Buff).
    buff_signal(w, f, BuffSignal::ConditionalBuffUpdate, BuffSignalArg { source: Some(b), ..BuffSignalArg::default() });
    false
}

fn shield_end(w: &mut World, f: FighterId, _b: BuffRef) {
    w.fighter_mut(f).invuln.projectile_armor = false;
    sound(w, f, name!("sfx_cha_finn_shield_loop_stop"), None);
}

pub(super) fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    enter(
        r,
        &[
            kind(C::BuffPerkAbilityRefundOnDodge, BuffHooks { on: Some(refund_on_dodge), ..NO_HOOKS }, None),
            kind(C::BuffPerkAbilityRefundOnTakeProjectileKB, BuffHooks { on: Some(refund_on_kb), ..NO_HOOKS }, None),
            kind(
                C::BuffDodgeArmor,
                BuffHooks { begin: Some(dir_begin), tick: Some(dir_tick), on: Some(dodge_armor_on), ..NO_HOOKS },
                None,
            ),
            kind(
                C::BuffDodgeReflect,
                BuffHooks { begin: Some(dir_begin), tick: Some(dir_tick), on: Some(dodge_reflect_on), ..NO_HOOKS },
                None,
            ),
            kind(
                C::BuffOnKnockbackProjectileBlock,
                BuffHooks {
                    begin: Some(block_begin),
                    on: Some(block_on),
                    tick: Some(block_tick),
                    timer: Some(block_timer),
                    ..NO_HOOKS
                },
                None,
            ),
            kind(
                C::BuffProjectileBlock,
                BuffHooks { begin: Some(shield_begin), on: Some(shield_on), end: Some(shield_end), ..NO_HOOKS },
                None,
            ),
        ],
    )
}
