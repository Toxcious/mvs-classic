//! IceProjectile "Ice to Beat You!" (Buff_IceProjectile, a child of Buff_ProjectileApplyBuff) and
//! ProjectileGrayHealth "Retaliation Ready" (Buff_ProjectileGrayHealth).
//! Read: Buff_IceProjectile, Buff_ProjectileApplyBuff, Buff_ProjectileGrayHealth; Comp_Actor_Destructible GetAllies.
//!
//! Both classes have GiveBuffToSpawnedActors, GiveBuffToReflectedActors and DestroyBuffWhenOwnerReflected (their
//! CDOs). On the fighter they do nothing: BeginPlay's eligibility needs a Destructible owner that is a projectile,
//! and their DealtDamageModCondition in the fighter's own dealt pass writes only the class's Active and answers the
//! parent's true; their DealtDamageMods are DoNothing on every field (BuffParentActor's defaults). The copy on each
//! spawned actor binds the actor's handler's HitEnemyPlayer in its BeginPlay, ahead of the class's own handler (the
//! HitEnemyPlayerFirst signal).

use super::conds::actor_buff;
use super::hooks::{NO_ACTOR, NO_HOOKS};
use super::{PerkActorExt, enter, flag, kind, num};
use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_owner, actor_p};
use crate::combat::buffs::{Def, buff_of};
use crate::combat::defense::add_temp_health_pack;
use crate::combat::elem::{
    ElemExt, PROJECTILE_APPLY_BUFF_ACTOR, apply_buff_dealt_cond, elem_buff_mut, projectile_like,
};
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::fighter::FighterId;
use crate::fighter::common::{buff_def, lit, lookup_owner, peers};
use crate::name;
use crate::registry::entries::{ActorBuffHooks, BuffHooks};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Value;

/// Enum_ProjectileType NewEnumerator2 "Heavy" = 1 (Enum_ProjectileType Names).
const HEAVY: Real = 1.0;

/// BPI_GetProjectileAttackType of the DamageDealer (SpawnedActorParent's @0: the class's ProjectileAttackType): a
/// Projectile's or an Item's own `attackType`; another actor answers its class data's (0, Normal, without one).
fn attack_type(w: &World, a: ActorRef) -> Real {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(0.0, |p| crate::real(p.attack_type)),
        ActorRef::Item(id) => crate::real(crate::actors::item::item_attack_type(w, id)),
        _ => crate::math::narrow(actor_p(w, a).get("ProjectileAttackType").and_then(Value::as_f64).unwrap_or(0.0)),
    }
}

fn kb_hs(h: &HitPacket) -> bool {
    h.base_knock_back.unwrap_or(0.0) > 0.0 && h.hit_stun_mult.unwrap_or(0.0) > 0.0
}

// The fighter's own copy: the conditions answer true (Active is never read there: nothing bound).
fn always(_w: &mut World, _v: ActorRef, _o: FighterId, _dl: FighterId, _h: &HitPacket, _b: BuffRef, _hb: bool) -> bool {
    true
}

// ---------------------------------------------------------------- Buff_IceProjectile
// The parent's graph (the projectile_apply_buff family: BeginPlay's bind, OnProjectileHitsEnemy's decay == 1 and
// Active gates, SpawnBuffActor(Buff_Ice, the victim, the actor's fighter, 1 / 2 stacks, duration 0)) with its own
// DealtDamageModCondition (@0-659): the parent's first, then Active = (ProjectileAttackType == Heavy) AND NOT the
// victim's BPI_CheckIfHasBuff(Buff_Ice) AND BaseKnockBack > 0 AND HitStunMult > 0 (a DamageDealer that is no
// BPI_SpawnedActor gives type 0 @752); the parent's answer (true). Buff_Ice's own graph (its registry entry) takes
// the stacks from there, as for a character's ice hit.
// Unverified: a Victim that is no fighter is taken as holding no Buff_Ice (its own buffs are not read).
fn ice_dealt(w: &mut World, a: ActorRef, i: u32, v: ActorRef, h: &HitPacket, hit_box: bool) -> bool {
    let r = apply_buff_dealt_cond(w, a, i, v, h, hit_box);
    let iced = matches!(v, ActorRef::Fighter(f) if buff_of(w, f, name!("Buff_Ice_C")).is_some());
    let on = attack_type(w, a) == HEAVY && !iced && kb_hs(h);
    if let Some(b) = elem_buff_mut(w, a, i)
        && let ElemExt::ApplyBuff { active, .. } = &mut b.ext
    {
        *active = on;
    }
    r
}

// ---------------------------------------------------------------- Buff_ProjectileGrayHealth
// BeginPlay (@1313-1846, authority): the parent tests of Buff_ProjectileApplyBuff (projectileLike) -> bind
// HitEnemyPlayer -> OnHitEnemy. DealtDamageModCondition (@64-188): Active = BaseKnockBack > 0 AND HitStunMult > 0;
// true. OnHitEnemy @2373: Active -> ApplyGrayArmor @1888: AllyGrayHealth.HealthAmount = BaseGrayHealth +
// (CurrentStackAmount - 1) x GrayHealthPerExtraStack @1888-2022 (float32 steps); the actor's GetInstigator's
// Comp_Actor_Destructible.GetAllies @2323; each ally other than that Instigator @886 with a Comp_Armor @1044 (every
// fighter): AddTempHealthPack(AllyGrayHealth) @1193 (ProjectileAlly, CanOverride, Time 3). No decay gate, no range,
// no pack for the instigator himself. GetAllies (Comp_Actor_Destructible): the team mates other than the owner, a
// knocked-out one out of the list until its respawn.
// Unverified: the Instigator is the actor's owner (after a reflect the reflector, whose buffs the actor then
// carries; a thrown item's thrower); the Allies order is the fighters' order (each gets the same pack; the packs'
// TempHealthAdded signals come in that order).
fn gray_state(w: &World, a: ActorRef, i: u32) -> Option<(bool, bool)> {
    match actor_buff(w, a, i)?.ext {
        ElemExt::Perk(PerkActorExt::Gray { bound, active }) => Some((bound, active)),
        _ => None,
    }
}

fn set_gray(w: &mut World, a: ActorRef, i: u32, g: PerkActorExt) {
    if let Some(e) = elem_buff_mut(w, a, i) {
        e.ext = ElemExt::Perk(g);
    }
}

fn gray_on_hit(w: &mut World, a: ActorRef, i: u32) {
    if gray_state(w, a, i) != Some((true, true)) {
        return;
    }
    let Some(s) = actor_buff(w, a, i).map(|e| e.stacks) else { return };
    let d = Def::of(buff_def(w, ActorRef::Fighter(lookup_owner(w, a)), name!("Buff_ProjectileGrayHealth_C")));
    let g = d.obj("AllyGrayHealth");
    let per = crate::math::narrow((f64::from(s) - 1.0) * f64::from(num(d, "GrayHealthPerExtraStack")));
    let amount = num(d, "BaseGrayHealth") + per;
    // an actor with no owner carries no perk's buff (GrantBuffsFromInstigator has no Instigator)
    let Some(inst) = actor_owner(w, a) else { return };
    let kind = match g.and_then(|g| g.get("TempHealthType")).and_then(Value::as_str) {
        Some(t) => lit(w, t),
        None => name!("None"),
    };
    let can_override = flag(g, "CanOverride");
    let time = num(g, "Time");
    let team = w.fighter(inst).team.team;
    // `for (const o of inst.peers)`: by place, as a pack's TempHealthAdded handlers may write the World.
    for k in 0..peers(w).len() {
        let o = peers(w)[k];
        if o == inst || w.fighter(o).team.team != team || w.fighter(o).life.dead > 0.0 {
            continue;
        }
        add_temp_health_pack(w, o, kind, amount, can_override, time);
    }
}

fn gray_begin(w: &mut World, a: ActorRef, i: u32, _from: Option<FighterId>) -> bool {
    let bound = projectile_like(w, a);
    set_gray(w, a, i, PerkActorExt::Gray { bound, active: false });
    false
}

fn gray_dealt(w: &mut World, a: ActorRef, i: u32, _v: ActorRef, h: &HitPacket, _hb: bool) -> bool {
    if let Some((bound, _)) = gray_state(w, a, i) {
        set_gray(w, a, i, PerkActorExt::Gray { bound, active: kb_hs(h) });
    }
    true
}

fn gray_on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, a_arg: Option<&ActorBuffArg>) -> bool {
    let arg = a_arg.map(|x| ActorRef::Fighter(x.by));
    if s == ActorBuffSignal::HitEnemyPlayerFirst && arg.is_some() {
        gray_on_hit(w, a, i);
    }
    false
}

pub(super) fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    enter(
        r,
        &[
            kind(
                C::BuffIceProjectile,
                BuffHooks { dealt_cond: Some(always), ..NO_HOOKS },
                Some(ActorBuffHooks { dealt_cond: Some(ice_dealt), ..PROJECTILE_APPLY_BUFF_ACTOR }),
            ),
            kind(
                C::BuffProjectileGrayHealth,
                BuffHooks { dealt_cond: Some(always), ..NO_HOOKS },
                Some(ActorBuffHooks {
                    begin: Some(gray_begin),
                    dealt_cond: Some(gray_dealt),
                    on: Some(gray_on),
                    ..NO_ACTOR
                }),
            ),
        ],
    )
}
