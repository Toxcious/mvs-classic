//! CullTheWeak, PaintedTarget, Sniper, LastStand, ClearHeadspace, Inevitable.
//! Read: Buff_CullTheWeak, Buff_PaintedTarget, Buff_Sniper, Buff_LastStand, Buff_ClearHeadspace, Buff_Inevitable;
//! their defaults: the general perk table's buffs.
//! CullTheWeak, PaintedTarget, Sniper and Inevitable are DealtDamageModCondition overrides alone (no BeginPlay, hook
//! or timer of their own that does anything: Inevitable's bound handler is empty), on the fighter and, by
//! GiveBuffToSpawnedActors / GiveBuffToReflectedActors (their CDOs), on the actors it spawns or reflects
//! (combat/projectile-buffs grantSpawnedBuffs / updateBuffsOnReflect, the actor's dealt pass actorDealtDamageMods).
//! The condition is the same graph on either owner: BuffOwner is the fighter or the actor.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated tests are kept for their NaN result")]

use super::conds::{actor_buff, class_name};
use super::hooks::{NO_ACTOR, NO_HOOKS, pop, projectile_dealer};
use super::{PerkExt, Vals, def, enter, ext, ext_mut, kind, num, peer_by_id, set_ext, stacks};
use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_owner, actor_place};
use crate::combat::buffs::{Def, buff_collision_off, buff_of};
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::events::PopKind;
use crate::fighter::FighterId;
use crate::fighter::common::{buff_def, lookup_owner, peers, warn_once};
use crate::fighter::{actor_y, cap_radius, capsule_half, in_hitstun};
use crate::math::{max, min, ue};
use crate::name;
use crate::registry::entries::{ActorBuffHooks, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

// ---------------------------------------------------------------- Buff_CullTheWeak (Snowball Effect)
// DealtDamageModCondition @0-1440: the Victim's Comp_Actor_Damage valid @56, LocalMaxDamage = its CurrentDamage @146;
// over GetAllPlayers @241-1435, each player whose PlayerState PawnPrivate is valid @670 and not the Victim @926: its
// pawn's CurrentDamage >= LocalMaxDamage @1264 -> false @1334. The loop's end: true @1350. Owner and allies are not
// left out: the Victim alone of every pawn must have the most damage, a tie refuses.
// Its DealtDamageMods keep the parent's type DoNothing (data: the base struct's DamageModType DoNothing; the per-stack
// .1 is added to its number, the type stays): the getter's answer changes no field (dealt_mods_over reads the base
// type), so the buff never changes a hit, as in the build.
// Unverified: every fighter of the match counts as a valid pawn, one out of play waiting for its respawn too, with the
// damage it had (the defeat tail keeps the pawn). Inert either way.
// A minion is in `peers` but has no PlayerState: not in GetAllPlayers, left out.
// A HitActor that is no fighter: its Comp_Actor_Damage invalid @5-85 -> pop_flow, false.
fn victim_most_damaged(w: &World, victim: ActorRef) -> bool {
    let ActorRef::Fighter(v) = victim else { return false };
    let top = w.fighter(v).stats.damage;
    !peers(w).iter().any(|&o| o != v && !w.fighter(o).life.is_minion && w.fighter(o).stats.damage >= top)
}

fn cull(w: &mut World, v: ActorRef, _o: FighterId, _dl: FighterId, _h: &HitPacket, _b: BuffRef, _hb: bool) -> bool {
    victim_most_damaged(w, v)
}

fn cull_actor(w: &mut World, _a: ActorRef, _i: u32, v: ActorRef, _h: &HitPacket, _hb: bool) -> bool {
    victim_most_damaged(w, v)
}

// ---------------------------------------------------------------- Buff_PaintedTarget
// DealtDamageModCondition @0-225: the Victim's Comp_Pawn_HitstunHandler valid @51 -> CheckHitstunActive @145
// (HitstunSourcesActive > 0 || BuffstunSourcesActive > 0: Fighter.inHitstun). In the dealt pass, ahead of the hit's
// own hitstun: the hit itself does not paint its victim.
// Unverified: CheckHitstunActive taken as Fighter.inHitstun, as the port's other readers.
// A HitActor that is no fighter: no Comp_Pawn_HitstunHandler @51, false.
fn painted_victim(w: &World, v: ActorRef) -> bool {
    matches!(v, ActorRef::Fighter(f) if in_hitstun(w, f))
}

fn painted(w: &mut World, v: ActorRef, _o: FighterId, _dl: FighterId, _h: &HitPacket, _b: BuffRef, _hb: bool) -> bool {
    painted_victim(w, v)
}

fn painted_actor(w: &mut World, _a: ActorRef, _i: u32, v: ActorRef, _h: &HitPacket, _hb: bool) -> bool {
    painted_victim(w, v)
}

// ---------------------------------------------------------------- Buff_Sniper (Shirt Cannon Sniper)
// DealtDamageModCondition @0-518: BuffOwner's Comp_Actor_Destructible valid @51 and (its IsProjectile or BuffOwner
// implements BPI_Projectiles) @183, else false @507; then Vector_Distance(BuffOwner.GetInstigator() location, the
// Victim's location) >= DistanceToApply (1000) @399-445: where the thrower stands now, not how far the actor flew.
// On the fighter the owner is the pawn: no projectile, never (as the projectile damage boost: projectile_dealer, the
// same class test). The actor's Instigator: its owner (Projectile.owner, set again by a reflect; Item.owner, the last
// thrower). DestroyBuffWhenOwnerReflected: the reflect drops the thrower's copy and gives the reflector's
// (combat/projectile-buffs updateBuffsOnReflect). Its HitEffectOverride PS_HitImpact_Big goes on the hit with the
// mods (dealt_mods_over -> OnHitEffectOverride -> World hitVfx override).
// Unverified: a Destructible is taken as on every spawned actor, as the projectile boosts.
// A Victim that is no fighter: its actor location (x, y); a kind that keeps none is warned and refused.
fn sniper_range(w: &World, inst: FighterId, victim: ActorRef, d: impl Vals) -> bool {
    let at = match victim {
        ActorRef::Fighter(v) => Some((w.fighter(v).body.x, actor_y(w, v))),
        a => actor_place(w, a).map(|p| (p.0, p.1)),
    };
    let Some((ax, ay)) = at else {
        warn_once(w, name!("Buff_Sniper: no location kept for a HitActor"));
        return false;
    };
    let dist = ue::size2d([w.fighter(inst).body.x - ax, actor_y(w, inst) - ay]);
    dist >= num(d, "DistanceToApply")
}

fn sniper_actor(w: &mut World, a: ActorRef, i: u32, v: ActorRef, _h: &HitPacket, _hb: bool) -> bool {
    if !projectile_dealer(w, a) {
        return false;
    }
    let Some(inst) = actor_owner(w, a) else { return false };
    let Some(cls) = actor_buff(w, a, i).map(|b| b.class) else { return false };
    let own = lookup_owner(w, a);
    let n = class_name(w, cls);
    sniper_range(w, inst, v, Def::of(buff_def(w, ActorRef::Fighter(own), n)))
}

// ---------------------------------------------------------------- Buff_LastStand
// ReceiveBeginPlay @10 (authority): the owner's Comp_Pawn_DamageManager valid @154 -> bind its HitByEnemy to
// OnHitByEnemy @239-262 and call it at once with an empty hit @303; else K2_DestroyActor @591 (no owner but a pawn
// takes it: no GiveBuffToSpawnedActors). OnHitByEnemy -> @610: while !Activated, GetInstigator()'s
// Comp_Actor_Damage valid @753 and its CurrentDamage >= DamageToActivate - CurrentStackAmount x
// DamageReductionPerkStack @838-930 (110 - 10 S): Activated = true @1004 for the buff's life (no reset below it, no
// End unbind); the owner a BP_FighterCharacter and BPI_Base_Fighter -> BPI_PopText "Last Stand!" @1233-1745 (hud
// popText kind lastStand). The Instigator is SpawnBuffActor's: the owner for its own perk, the team mate whose perk
// it is for the ally grant (`add_perks`), so the team mate's damage is read then. DealtDamageModCondition @0:
// Activated. HitByEnemy: combat/buffs hit_by_enemy (DamageAndLaunch @3723, after the hit's damage is applied).
fn last_stand_check(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(PerkExt::LastStand { by, activated: false }) = ext(w, b) else { return };
    let Some(inst) = peer_by_id(w, *by) else { return };
    let d = def(w, b);
    let need = num(d, "DamageToActivate") - (stacks(w, b) * num(d, "DamageReductionPerkStack"));
    if !(w.fighter(inst).stats.damage >= need) {
        return; // NaN: not activated
    }
    if let Some(PerkExt::LastStand { activated, .. }) = ext_mut(w, b) {
        *activated = true;
    }
    pop(w, f, PopKind::LastStand);
}

fn last_stand_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    let by = from.map_or(-1, |o| w.fighter(o).id);
    set_ext(w, b, PerkExt::LastStand { by, activated: false });
    last_stand_check(w, f, b);
}

fn last_stand_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    if s == BuffSignal::HitByEnemy {
        last_stand_check(w, f, b);
    }
    false
}

fn last_stand_dealt(
    w: &mut World,
    _v: ActorRef,
    _o: FighterId,
    _dl: FighterId,
    _h: &HitPacket,
    b: BuffRef,
    _hb: bool,
) -> bool {
    matches!(ext(w, b), Some(PerkExt::LastStand { activated: true, .. }))
}

// ---------------------------------------------------------------- Buff_ClearHeadspace (Keep Your Distance)
// ReceiveBeginPlay @177: the owner a BP_FighterCharacter @207-272 -> IsFighter = true @10, FighterOverlap (a sphere,
// radius 500, overlapping Pawn and ignoring the world / item channels) K2_AttachToComponent(the owner's root, None,
// SnapToTarget, KeepRelative, KeepRelative, true) @63 and SetCollisionEnabled(QueryOnly) @134. No tick, timer or
// EndPlay of its own.
// DealtDamageModCondition @0-567: IsFighter -> false @501: the fighter's own copy never boosts its hit. On a spawned
// actor (GiveBuffToSpawnedActors) IsFighter stays false: GetOwner() (the actor).GetInstigator() as BPI_BaseFighter
// @43-170, its BPI_CheckIfHasBuff(Buff_ClearHeadspace) @184, that buff's IsClearOfFighters @432. The actor's
// Instigator: its owner (as Buff_Sniper's).
// IsClearOfFighters @0-173: FighterOverlap.GetOverlappingActors(BP_FighterCharacter) has fewer than 2 entries. The
// owner's own capsule is among them (UpdateOverlapsImpl ignores no owner: the sphere's actor is the buff, not the
// pawn), so clear is no other pawn but the owner within the sphere.
// The sphere sits at the owner's root, at its scale (KeepRelative: the sphere's world scale is the root's; a sphere's
// radius times the smallest axis).
// Only Pawn bodies count: the sphere's saved responses leave HurtBox, HitBox and KnockBack at their DefaultEngine.ini
// default (Ignore), and the Pawn profile answers it; a pawn's mesh has no body, so that is each pawn's root capsule.
// A minion is a BP_FighterCharacter (its pawn class's parent): it counts. Touching counts as an overlap.
// Unverified: the list is taken at the hit's moment, with the skips the trace module's fighter gather has (defeated,
// collision off, capsule off); the build's list is the one of the sphere's last overlap update.
/// FighterOverlap_GEN_VARIABLE.SphereRadius.
const HEADSPACE_RADIUS: Real = 500.0;

fn clear_of_fighters(w: &World, f: FighterId) -> bool {
    let x = w.fighter(f);
    let k = match x.body.actor_scale {
        Some([xy, z]) => min(xy, z),
        None => x.body.size_scale,
    };
    let r = HEADSPACE_RADIUS * k;
    let fy = actor_y(w, f);
    let fx = x.body.x;
    let mut n = 0;
    // every pawn of the match, the owner and the minions included
    for &o in peers(w) {
        let ox = w.fighter(o);
        let collision_off = ox.moves.pull.is_some() || buff_collision_off(w, o);
        if ox.life.dead != 0.0 || collision_off || ox.capsule.capsule_off {
            continue;
        }
        let cr = cap_radius(w, o);
        let seg = max(0.0, capsule_half(w, o) - cr);
        let oy = actor_y(w, o);
        let dy = if fy < oy - seg {
            oy - seg - fy
        } else if fy > oy + seg {
            fy - (oy + seg)
        } else {
            0.0
        };
        let dx = ox.body.x - fx;
        let reach = r + cr;
        if dx * dx + dy * dy <= reach * reach {
            n += 1;
        }
        if n >= 2 {
            return false;
        }
    }
    true
}

fn headspace_actor(w: &mut World, a: ActorRef, _i: u32, _v: ActorRef, _h: &HitPacket, _hb: bool) -> bool {
    let Some(inst) = actor_owner(w, a) else { return false };
    buff_of(w, inst, name!("Buff_ClearHeadspace_C")).is_some() && clear_of_fighters(w, inst)
}

// ---------------------------------------------------------------- Buff_Inevitable
// DealtDamageModCondition @0-553: the Victim's Comp_Actor_Damage valid @51 and its CurrentDamage >= DamageRequired
// (135) - CurrentStackAmount x DamageReductionPerkStack (10) @145-297, its Comp_Armor valid and Has Armor Equipped
// (CurrentArmorPacks not empty) @456 -> BrokeArmor = true @515 (never read) and true; else false. Its DealtDamageMods
// are ArmorBreak alone (dealt_mods_over reads it whatever the mod types): the hit pierces the armor. The same graph
// on a spawned actor (GiveBuffToSpawnedActors): BuffOwner is not read.
// ReceiveBeginPlay @274 (authority): the owner's Comp_Pawn_HitBoxHandler valid -> HitEnemyPlayer bound to OnHitEnemy,
// whose body is empty: nothing to port.
// A HitActor that is no fighter: no Comp_Actor_Damage @51 (as Buff_CullTheWeak's), false.
fn inevitable(w: &World, victim: ActorRef, s: Real, d: impl Vals) -> bool {
    let ActorRef::Fighter(v) = victim else { return false };
    let need = num(d, "DamageRequired") - (s * num(d, "DamageReductionPerkStack"));
    w.fighter(v).stats.damage >= need && !w.fighter(v).armor.armor_packs.is_empty()
}

fn inevitable_cond(
    w: &mut World,
    v: ActorRef,
    _o: FighterId,
    _dl: FighterId,
    _h: &HitPacket,
    b: BuffRef,
    _hb: bool,
) -> bool {
    inevitable(w, v, stacks(w, b), def(w, b))
}

fn inevitable_actor(w: &mut World, a: ActorRef, i: u32, v: ActorRef, _h: &HitPacket, _hb: bool) -> bool {
    let Some((cls, s)) = actor_buff(w, a, i).map(|b| (b.class, crate::real(b.stacks))) else { return false };
    let own = lookup_owner(w, a);
    let n = class_name(w, cls);
    inevitable(w, v, s, Def::of(buff_def(w, ActorRef::Fighter(own), n)))
}

fn never(_w: &mut World, _v: ActorRef, _o: FighterId, _dl: FighterId, _h: &HitPacket, _b: BuffRef, _hb: bool) -> bool {
    false
}

pub(super) fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    let actor = |dealt_cond| Some(ActorBuffHooks { dealt_cond: Some(dealt_cond), ..NO_ACTOR });
    enter(
        r,
        &[
            kind(C::BuffClearHeadspace, BuffHooks { dealt_cond: Some(never), ..NO_HOOKS }, actor(headspace_actor)),
            kind(
                C::BuffInevitable,
                BuffHooks { dealt_cond: Some(inevitable_cond), ..NO_HOOKS },
                actor(inevitable_actor),
            ),
            kind(C::BuffCullTheWeak, BuffHooks { dealt_cond: Some(cull), ..NO_HOOKS }, actor(cull_actor)),
            kind(C::BuffPaintedTarget, BuffHooks { dealt_cond: Some(painted), ..NO_HOOKS }, actor(painted_actor)),
            kind(C::BuffSniper, BuffHooks { dealt_cond: Some(never), ..NO_HOOKS }, actor(sniper_actor)),
            kind(
                C::BuffLastStand,
                BuffHooks {
                    begin: Some(last_stand_begin),
                    on: Some(last_stand_on),
                    dealt_cond: Some(last_stand_dealt),
                    ..NO_HOOKS
                },
                None,
            ),
        ],
    )
}
