//! The general perks whose part in a hit is a DealtDamageModCondition of their own: the
//! melee damage / knockback boosts by air and ground, Buff_Perk_KBAngleDependent and its two children,
//! Buff_Perk_ChargeArmorBreak (with its copy on a spawned actor: GiveBuffToSpawnedActors) and
//! Buff_DamageBoostOnDebuffHit. None of them binds a hook, starts a timer or raises a pop text, sound or effect: the
//! condition is the whole graph (each class's only function besides the parent's). Their mods are their class
//! defaults, read by the dealt pass (`combat::buffs` dealt_mods_over: DealtDamageMods + the per-stack struct x S).
//! Read: Buff_Perk_AirMeleeDamageBoost_Small, Buff_Perk_AirMeleeKnockbackBoost_Small,
//! Buff_Perk_GroundMeleeDamageBoost_Small, Buff_Perk_GroundMeleeKnockbackBoost_Small, Buff_Perk_KBAngleDependent,
//! Buff_Perk_ChargeArmorBreak, Buff_DamageBoostOnDebuffHit, Comp_State_Manager (CheckHasDebuff),
//! Comp_Pawn_ChargeAttack (GetChargeValue).

use super::hooks::{NO_ACTOR, NO_HOOKS};
use super::{Vals, def, enter, entry, flag, kind, num, values_of};
use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_elem, actor_owner};
use crate::combat::buffs::Def;
use crate::combat::elem::{ElemBuff, charge_pct, elem_buff, instigator_charge};
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::fighter::FighterId;
use crate::fighter::common::{buff_def, lit, lookup_owner};
use crate::math::ue;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, BuffHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

// ---------------------------------------------------------------- the melee boosts by air / ground
// Buff_Perk_{Air,Ground}Melee{Damage,Knockback}Boost_Small DealtDamageModCondition: BuffOwner cast to
// BP_FighterCharacter @0 (failed: false @267 / @238), then IsValid(HitCollision) @79 AND (not, for Air @176)
// CharacterMovement.IsMovingOnGround() @108 -> ShouldApplyBuffMods @243 / @214. No victim or melee test: any hit of
// the owner's own hit box handler with a hit box (a Hitframe's) takes part; a packet a notify deals with no hit box
// (World.directHit) does not. _Medium / _Large are data-only children (their SuperStruct): the Small graph, their
// defaults (DealtDamageMods base 1.1 / 1.15 for damage, 1.03 / 1.1 / 1.15 for knockback, per stack Add .05 / .02).
// IsMovingOnGround: the sim's grounded (MOVE_Walking), as RefreshingKnockOut's gate.
// None of the twelve is GiveBuffToSpawnedActors / Reflected: no copy on an actor, so no actor-side entry.
fn air(
    w: &mut World,
    _v: ActorRef,
    owner: FighterId,
    _dl: FighterId,
    _h: &HitPacket,
    _b: BuffRef,
    hit_box: bool,
) -> bool {
    hit_box && !w.fighter(owner).body.on_floor
}

fn ground(
    w: &mut World,
    _v: ActorRef,
    owner: FighterId,
    _dl: FighterId,
    _h: &HitPacket,
    _b: BuffRef,
    hit_box: bool,
) -> bool {
    hit_box && w.fighter(owner).body.on_floor
}

// ---------------------------------------------------------------- Buff_Perk_KBAngleDependent
// DealtDamageModCondition: IsValid(HitCollision) @0 AND (BuffOwner a BP_FighterCharacter @94 OR a SpawnedActorParent
// @29) @197; then DegAtan2(HitInformation.KnockBackDirectionVector Y, X) @295 >= Min Angle @341 AND <= Max Angle
// @379 -> ShouldApplyBuffMods @455; else false @479. The hit as the pass has it (an earlier buff's direction mod
// counts), ahead of the facing sign (pfgDoDamage @10359-11499 flips X later): X is along the dealer's facing, so no
// mirroring. The packet holds the direction as KnockBackDirection (the Hitframe's name and the struct's alike).
// Children: Buff_Perk_HorizontalDamageBoost (Min -15, Max 30) and Buff_Perk_UpwardDamageBoost (75, 105), data only.
// The helper itself (Min / Max omitted: 0 / 0) is in no perk; entered for its own graph all the same.
// Neither child is GiveBuffToSpawnedActors / Reflected, so the SpawnedActorParent half of the cast never meets a copy
// in the port: no actor-side entry.
// A hit with no KnockBackDirection (an actor's packet) reads its KnockBackDirectionVector.
fn kb_angle(hit: &HitPacket, d: impl Vals) -> bool {
    let (x, y) = match hit.knock_back_direction.as_ref().or(hit.knock_back_direction_vector.as_ref()) {
        Some(v) => (v.x.unwrap_or(0.0), v.y.unwrap_or(0.0)),
        None => (1.0, 0.0),
    };
    let deg = ue::deg_atan2(y, x);
    deg >= num(d, "Min Angle") && deg <= num(d, "Max Angle")
}

fn kb_angle_cond(
    w: &mut World,
    _v: ActorRef,
    _o: FighterId,
    _dl: FighterId,
    hit: &HitPacket,
    b: BuffRef,
    hb: bool,
) -> bool {
    hb && kb_angle(hit, def(w, b))
}

// ---------------------------------------------------------------- Buff_Perk_ChargeArmorBreak
// DealtDamageModCondition: BuffOwner's Comp_Pawn_ChargeAttack @0 valid @51 (else return @383 with the output
// unwritten: false), GetChargeValue(4: PercentCharged) @145 >= ChargePercentBase + (CurrentStackAmount - 1) x
// ChargePercentPerStack @192-326 (1 + (S - 1) x -0.25: 1 / 0.75) -> ShouldApplyBuffMods @364. Its DealtDamageMods
// sets ArmorBreak alone (the pass ORs it into the hit's ArmorPierce: dealt_mods_over), no number.
// The fighter: GetChargeValue(4) is PercentCharged while the charge's AttackMontage is the active montage, else 0
// (`combat::elem` instigator_charge: the active-montage test, unverified).
// GiveBuffToSpawnedActors: its copy on a spawned actor reads the actor's own component (SpawnedActorParent's
// Comp_Pawn_ChargeAttack; GetChargeValue on a non-fighter owner returns the stored value @85 -> @236): the
// PercentCharged copied from the Instigator at its spawn or activation (`combat::elem` charge_pct), not the
// instigator's current one. S is the copy's stacks (the grant's).
// Unverified: every port actor that carries buffs is taken to have the component (SpawnedActorParent's), as
// combat/actor-charge does for its hit.
fn charge_ok(pct: Real, d: impl Vals, stacks: Real) -> bool {
    pct >= (num(d, "ChargePercentBase") + ((stacks - 1.0) * num(d, "ChargePercentPerStack")))
}

fn charge_cond(
    w: &mut World,
    _v: ActorRef,
    owner: FighterId,
    _dl: FighterId,
    _h: &HitPacket,
    b: BuffRef,
    _hb: bool,
) -> bool {
    let pct = instigator_charge(w, owner).map_or(0.0, |c| c.pct);
    let s = entry(w, b).map_or(0.0, |x| x.stacks);
    charge_ok(pct, def(w, b), s)
}

fn charge_actor_cond(w: &mut World, a: ActorRef, i: u32, _v: ActorRef, _h: &HitPacket, _hb: bool) -> bool {
    let Some(eb) = actor_buff(w, a, i) else { return false };
    let (cls, s) = (eb.class, crate::real(eb.stacks));
    let own = lookup_owner(w, a);
    let n = class_name(w, cls);
    charge_ok(charge_pct(w, a), Def::of(buff_def(w, ActorRef::Fighter(own), n)), s)
}

/// The actor's buff a hook was called with (its place in `elem.buffs`, or among the entries whose EndPlay runs).
pub(crate) fn actor_buff(w: &World, a: ActorRef, i: u32) -> Option<&ElemBuff> {
    elem_buff(w, a, i)
}

/// A buff class's name (`b.cls`); Name::NONE for a registered class the bind did not intern (no table has it).
pub(crate) fn class_name(w: &World, c: BuffClass) -> Name {
    match c {
        BuffClass::Data(n) => n,
        _ => lit(w, c.name(&w.names)),
    }
}

// ---------------------------------------------------------------- Buff_DamageBoostOnDebuffHit
// DealtDamageModCondition: the Victim's Comp_State_Manager @0 valid @51 (else false @214), its CheckHasDebuff @145
// -> ShouldApplyBuffMods @190. CheckHasDebuff (Comp_State_Manager @0-919): any key of its owner's Comp_Actor_Parent
// Children that is a valid BuffParentActor with IsDebuff @761 (the actor's own value: a buff's instance defaults).
// The port's children: the victim's buff list, those not already ending (frames > 0), as Buff_Cleanse's walk.
// Not GiveBuffToSpawnedActors: no actor-side entry.

/// Whether the fighter has a live buff whose class data says IsDebuff.
pub fn has_debuff(w: &World, f: FighterId) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|b| b.frames > 0.0 && flag(values_of(w, f, b), "IsDebuff"))
}

// A HitActor that is no fighter (a fighter's hit box on a projectile, item or actor): SpawnedActorParent has a
// Comp_State_Manager, so CheckHasDebuff walks the actor's own buff children (ElemState.buffs; an actor that never
// took a buff has none). IsDebuff is the class's: no actor buff class of the port writes it.
fn actor_has_debuff(w: &World, a: ActorRef) -> bool {
    let Some(Some(st)) = actor_elem(w, a) else { return false };
    let Some(owner) = actor_owner(w, a) else { return false };
    st.buffs.iter().any(|b| flag(buff_def(w, ActorRef::Fighter(owner), class_name(w, b.class)), "IsDebuff"))
}

fn debuff_cond(
    w: &mut World,
    v: ActorRef,
    _o: FighterId,
    _dl: FighterId,
    _h: &HitPacket,
    _b: BuffRef,
    _hb: bool,
) -> bool {
    match v {
        ActorRef::Fighter(f) => has_debuff(w, f),
        a => actor_has_debuff(w, a),
    }
}

pub(super) fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    const AIR: BuffHooks = BuffHooks { dealt_cond: Some(air), ..NO_HOOKS };
    const GROUND: BuffHooks = BuffHooks { dealt_cond: Some(ground), ..NO_HOOKS };
    const KB_ANGLE: BuffHooks = BuffHooks { dealt_cond: Some(kb_angle_cond), ..NO_HOOKS };
    enter(
        r,
        &[
            kind(C::BuffPerkAirMeleeDamageBoostSmall, AIR, None),
            kind(C::BuffPerkAirMeleeDamageBoostMedium, AIR, None),
            kind(C::BuffPerkAirMeleeDamageBoostLarge, AIR, None),
            kind(C::BuffPerkAirMeleeKnockbackBoostSmall, AIR, None),
            kind(C::BuffPerkAirMeleeKnockbackBoostMedium, AIR, None),
            kind(C::BuffPerkAirMeleeKnockbackBoostLarge, AIR, None),
            kind(C::BuffPerkGroundMeleeDamageBoostSmall, GROUND, None),
            kind(C::BuffPerkGroundMeleeDamageBoostMedium, GROUND, None),
            kind(C::BuffPerkGroundMeleeDamageBoostLarge, GROUND, None),
            kind(C::BuffPerkGroundMeleeKnockbackBoostSmall, GROUND, None),
            kind(C::BuffPerkGroundMeleeKnockbackBoostMedium, GROUND, None),
            kind(C::BuffPerkGroundMeleeKnockbackBoostLarge, GROUND, None),
            kind(C::BuffPerkKBAngleDependent, KB_ANGLE, None),
            kind(C::BuffPerkHorizontalDamageBoost, KB_ANGLE, None),
            kind(C::BuffPerkUpwardDamageBoost, KB_ANGLE, None),
            kind(
                C::BuffPerkChargeArmorBreak,
                BuffHooks { dealt_cond: Some(charge_cond), ..NO_HOOKS },
                Some(ActorBuffHooks { dealt_cond: Some(charge_actor_cond), ..NO_ACTOR }),
            ),
            kind(C::BuffDamageBoostOnDebuffHit, BuffHooks { dealt_cond: Some(debuff_cond), ..NO_HOOKS }, None),
        ],
    )
}
