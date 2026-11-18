//! A hit's packet: a record of named values (a Hitframe notify's properties, a projectile's HitBox data
//! or a packet an actor's graph builds), typed. The fields are Struct_HitBoxDamage's members and the Hitframe notify's
//! own beside them; a field the source does not hold is `None` (`undefined` there), and each reader keeps the
//! default each reader applies. A stage that writes a field (`{ ...p, Damage: d }`) sets it on a copy.
//! Where the notify names a member otherwise than the struct (KnockBackDirection, OnHitSound,
//! QueueHitPauseUntilPose, 'Direct Knockback Toward Socket', 'Account for Velocity with Vacuum Knockback'), both are
//! fields: a packet can hold both, and the readers take them in their own order.

use crate::Real;
use crate::State;
use crate::events::NotifyRef;
use crate::names::{Name, Names};
use mvs_data::value::{Props, Value};

/// An enum member of the data: an enumerator's name or its byte value, as the data file holds it (both are read:
/// both: `AdditiveKBMode` is a name in a notify, a number in a packet an actor's graph builds).
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum DataEnum {
    Name(Name),
    Value(i32),
}

/// A vector of the data (`{ X, Y }`, `{ X, Y, Z }`): the absent axes `None`.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct DataVec {
    pub x: Option<Real>,
    pub y: Option<Real>,
    pub z: Option<Real>,
}

/// `HitPacket`: the hit's parameters. `from` names where it came from (the notify, `None` for a packet built
/// whole), for what reads the source's other properties.
#[derive(State, Debug, Default)]
pub struct HitPacket {
    pub from: Option<NotifyRef>,
    /// A packet built as an object literal: its unset members are absent from the record (not null).
    pub sparse: bool,
    // Struct_HitBoxDamage
    pub account_for_velocity_with_vacuum_knockback: Option<bool>,
    /// 'Account for Velocity with Vacuum Knockback' (the notify's name of the member above).
    pub account_for_velocity_vacuum_notify: Option<bool>,
    pub additive_kb_mode: Option<DataEnum>,
    pub apply_hit_pause_to_damage_dealer: Option<bool>,
    pub armor_pierce: Option<bool>,
    /// AttackAnimation: the montage's object name.
    pub attack_animation: Option<Name>,
    pub attack_decay_name: Option<Name>,
    pub attack_decay_type: Option<DataEnum>,
    pub attack_id: Option<i32>,
    pub attack_tag: Option<Name>,
    pub base_knock_back: Option<Real>,
    /// ControllerRumble: the asset's object name.
    pub controller_rumble: Option<Name>,
    pub damage: Option<Real>,
    pub destroy_projectiles: Option<bool>,
    pub direct_knockback_towards_socket: Option<bool>,
    /// 'Direct Knockback Toward Socket' (the notify's name of the member above).
    pub direct_knockback_toward_socket: Option<bool>,
    pub directional_knockback: Option<bool>,
    pub do_los_check: Option<bool>,
    pub force_knockback: Option<bool>,
    pub hit_each_actor_only_once: Option<bool>,
    pub hit_only_one_fighter: Option<bool>,
    pub hit_pause_time: Option<Real>,
    pub hit_reaction: Option<DataEnum>,
    pub hit_stun_mult: Option<Real>,
    pub ignore_actor: Option<bool>,
    pub ignore_facing: Option<bool>,
    pub ignore_kbi: Option<bool>,
    pub kbi_multiplier: Option<Real>,
    pub kb_scaling_starting_dmg: Option<Real>,
    /// KnockBackDirection (the Hitframe notify's).
    pub knock_back_direction: Option<DataVec>,
    /// KnockBackDirectionVector (the struct's).
    pub knock_back_direction_vector: Option<DataVec>,
    pub knock_back_mirror: Option<bool>,
    pub knock_back_scaling: Option<Real>,
    pub knockback_socket_offset: Option<DataVec>,
    pub knockback_target_socket: Option<Name>,
    /// OnHitAudio (the struct's): the event's name.
    pub on_hit_audio: Option<Name>,
    /// OnHitSound (the notify's): the event's name.
    pub on_hit_sound: Option<Name>,
    pub on_hit_effect_override: Option<Name>,
    pub play_on_hit_effects: Option<bool>,
    /// QueueHitPauseUntilPoses (the struct's).
    pub queue_hit_pause_until_poses: Option<bool>,
    /// QueueHitPauseUntilPose (the notify's).
    pub queue_hit_pause_until_pose: Option<bool>,
    pub reflect_projectiles: Option<bool>,
    pub vacuum_info: Option<VacuumInfo>,
    pub vacuum_knockback: Option<bool>,
    pub velocity_instead_of_facing: Option<bool>,
    // the Hitframe notify's own
    pub hit_box_tags: Vec<Name>,
    pub hit_box_scale: Option<DataVec>,
    pub charge_hit_pause_mult: Option<Real>,
    pub on_hit_buffs: Vec<OnHitBuffRow>,
    /// ElementalBuffs.ElementalBuffs: the classes' names.
    pub elemental_buffs: Vec<Name>,
    /// 'Specific Scales Per Hit Box': per hit box tag, its scale.
    pub specific_scales: Vec<(Name, DataVec)>,
}

/// VacuumInfo (Struct_VacuumInfo).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct VacuumInfo {
    pub use_vacuum_knock_back: Option<bool>,
    pub vacuum_strength_alpha: Option<Real>,
    pub vacuum_towards_instigating_fighter: Option<bool>,
    pub vacuum_target_socket: Option<Name>,
    pub vacuum_socket_offset: Option<DataVec>,
    pub vacuum_duration: Option<Real>,
    pub vacuum_pause: Option<Real>,
    pub force_small_hit_effects: Option<bool>,
    pub account_for_attacker_velocity: Option<bool>,
}

/// A row of the Hitframe's OnHitBuffs (Struct_OnHitBuffs): the buff classes and to whom they go.
#[derive(State, Debug, Default)]
pub struct OnHitBuffRow {
    /// AppliedBuffs: the class's name.
    pub applied_buffs: Name,
    pub apply_buff_self: bool,
    pub apply_buff_hit: bool,
    pub apply_buff_ally: bool,
    pub apply_buff_enemy: bool,
    pub require_hit_to_apply_buff_to_self: bool,
    pub stack_count: Option<i32>,
    pub buff_duration: Option<Real>,
}

fn num(p: &Props, k: &str) -> Option<Real> {
    p.get(k).and_then(Value::as_f64).map(crate::math::narrow)
}

fn flag(p: &Props, k: &str) -> Option<bool> {
    p.get(k).and_then(Value::as_bool)
}

fn int(p: &Props, k: &str) -> Option<i32> {
    num(p, k).map(crate::math::to_int32)
}

/// A string of the data as a name.
/// Unverified: the bind interns every string of the data's hit packets (the World's names are read-only during a
/// match); one it left out reads as `Name::NONE`.
fn name_of(names: &Names, v: &Value) -> Option<Name> {
    v.as_str().map(|s| names.get(s).unwrap_or(Name::NONE))
}

fn name(names: &Names, p: &Props, k: &str) -> Option<Name> {
    p.get(k).and_then(|v| name_of(names, v))
}

fn vec_of(v: &Value) -> Option<DataVec> {
    let o = v.as_obj()?;
    Some(DataVec { x: num(o, "X"), y: num(o, "Y"), z: num(o, "Z") })
}

fn data_enum(names: &Names, v: &Value) -> Option<DataEnum> {
    match v {
        Value::Num(n) => Some(DataEnum::Value(crate::math::dbl::to_int32(*n))),
        Value::Str(_) => name_of(names, v).map(DataEnum::Name),
        _ => None,
    }
}

/// The items of a list of the data (`[...]`) or of a wrapped one (`{ key: [...] }`).
fn list<'a>(v: Option<&'a Value>, key: &str) -> &'a [Value] {
    match v {
        Some(Value::Arr(a)) => a.as_slice(),
        Some(Value::Obj(o)) => o.get(key).and_then(Value::as_arr).unwrap_or(&[]),
        _ => &[],
    }
}

/// The names of a list, into `out` (emptied first).
fn name_list(names: &Names, v: Option<&Value>, key: &str, mut out: Vec<Name>) -> Vec<Name> {
    out.clear();
    out.extend(list(v, key).iter().filter_map(|v| name_of(names, v)));
    out
}

impl VacuumInfo {
    pub fn from_props(names: &Names, p: &Props) -> VacuumInfo {
        VacuumInfo {
            use_vacuum_knock_back: flag(p, "UseVacuumKnockBack"),
            vacuum_strength_alpha: num(p, "VacuumStrengthAlpha"),
            vacuum_towards_instigating_fighter: flag(p, "VacuumTowardsInstigatingFighter"),
            vacuum_target_socket: name(names, p, "VacuumTargetSocket"),
            vacuum_socket_offset: p.get("VacuumSocketOffset").and_then(vec_of),
            vacuum_duration: num(p, "VacuumDuration"),
            vacuum_pause: num(p, "VacuumPause"),
            force_small_hit_effects: flag(p, "ForceSmallHitEffects"),
            account_for_attacker_velocity: flag(p, "AccountForAttackerVelocity"),
        }
    }
}

impl OnHitBuffRow {
    /// A row of the data (`OnHitBuffs.OnHitBuffs[i]`, a class's 'On Hit Buffs' row).
    pub fn from_props(names: &Names, p: &Props) -> OnHitBuffRow {
        OnHitBuffRow {
            applied_buffs: name(names, p, "AppliedBuffs").unwrap_or(Name::NONE),
            apply_buff_self: flag(p, "ApplyBuffSelf?").unwrap_or(false),
            apply_buff_hit: flag(p, "ApplyBuffHit?").unwrap_or(false),
            apply_buff_ally: flag(p, "ApplyBuffAlly?").unwrap_or(false),
            apply_buff_enemy: flag(p, "ApplyBuffEnemy?").unwrap_or(false),
            require_hit_to_apply_buff_to_self: flag(p, "RequireHitToApplyBuffToSelf").unwrap_or(false),
            stack_count: int(p, "StackCount"),
            buff_duration: num(p, "BuffDuration"),
        }
    }

    /// The rows of a wrapped list (`{ OnHitBuffs: [...] }`) or a plain one.
    pub fn rows(names: &Names, v: Option<&Value>) -> Vec<OnHitBuffRow> {
        Self::rows_into(names, v, Vec::new())
    }

    /// [`OnHitBuffRow::rows`] into `out` (emptied first).
    pub fn rows_into(names: &Names, v: Option<&Value>, mut out: Vec<OnHitBuffRow>) -> Vec<OnHitBuffRow> {
        out.clear();
        out.extend(list(v, "OnHitBuffs").iter().filter_map(Value::as_obj).map(|r| OnHitBuffRow::from_props(names, r)));
        out
    }

    /// One of the rows of `rows` meets `f`, read without collecting them.
    pub fn any(names: &Names, v: Option<&Value>, f: impl Fn(&OnHitBuffRow) -> bool) -> bool {
        list(v, "OnHitBuffs").iter().filter_map(Value::as_obj).any(|r| f(&OnHitBuffRow::from_props(names, r)))
    }
}

impl HitPacket {
    /// The packet of a data row's properties (a Hitframe notify's, a HitBox's): the members above read by their
    /// data names, the rest left to `from`.
    pub fn from_props(names: &Names, p: &Props, from: Option<NotifyRef>) -> HitPacket {
        let mut h = HitPacket::default();
        h.read_props(names, p, from);
        h
    }

    /// [`HitPacket::from_props`] into this packet, its four lists' buffers reused (a Hitframe's packet is read at
    /// every overlap test).
    pub fn read_props(&mut self, names: &Names, p: &Props, from: Option<NotifyRef>) {
        let nm = |k: &str| name(names, p, k);
        let en = |k: &str| p.get(k).and_then(|v| data_enum(names, v));
        let vc = |k: &str| p.get(k).and_then(vec_of);
        let mut specific_scales = std::mem::take(&mut self.specific_scales);
        specific_scales.clear();
        specific_scales.extend(
            list(p.get("Specific Scales Per Hit Box"), "")
                .iter()
                .filter_map(Value::as_obj)
                .filter_map(|e| Some((name(names, e, "Key")?, e.get("Value").and_then(vec_of)?))),
        );
        let hit_box_tags = name_list(names, p.get("HitBoxTags"), "HitBoxTags", std::mem::take(&mut self.hit_box_tags));
        let elemental_buffs =
            name_list(names, p.get("ElementalBuffs"), "ElementalBuffs", std::mem::take(&mut self.elemental_buffs));
        let on_hit_buffs = OnHitBuffRow::rows_into(names, p.get("OnHitBuffs"), std::mem::take(&mut self.on_hit_buffs));
        *self = HitPacket {
            from,
            sparse: false,
            account_for_velocity_with_vacuum_knockback: flag(p, "AccountForVelocityWithVacuumKnockback"),
            account_for_velocity_vacuum_notify: flag(p, "Account for Velocity with Vacuum Knockback"),
            additive_kb_mode: en("AdditiveKBMode"),
            apply_hit_pause_to_damage_dealer: flag(p, "ApplyHitPauseToDamageDealer"),
            armor_pierce: flag(p, "ArmorPierce"),
            attack_animation: nm("AttackAnimation"),
            attack_decay_name: nm("AttackDecayName"),
            attack_decay_type: en("AttackDecayType"),
            attack_id: int(p, "AttackID"),
            attack_tag: nm("AttackTag"),
            base_knock_back: num(p, "BaseKnockBack"),
            controller_rumble: nm("ControllerRumble"),
            damage: num(p, "Damage"),
            destroy_projectiles: flag(p, "DestroyProjectiles"),
            direct_knockback_towards_socket: flag(p, "DirectKnockbackTowardsSocket"),
            direct_knockback_toward_socket: flag(p, "Direct Knockback Toward Socket"),
            directional_knockback: flag(p, "DirectionalKnockback"),
            do_los_check: flag(p, "DoLOSCheck"),
            force_knockback: flag(p, "ForceKnockback"),
            hit_each_actor_only_once: flag(p, "HitEachActorOnlyOnce"),
            hit_only_one_fighter: flag(p, "HitOnlyOneFighter"),
            hit_pause_time: num(p, "HitPauseTime"),
            hit_reaction: en("HitReaction"),
            hit_stun_mult: num(p, "HitStunMult"),
            ignore_actor: flag(p, "IgnoreActor"),
            ignore_facing: flag(p, "IgnoreFacing"),
            ignore_kbi: flag(p, "IgnoreKBI"),
            kbi_multiplier: num(p, "KBI_Multiplier"),
            kb_scaling_starting_dmg: num(p, "KBScalingStartingDmg"),
            knock_back_direction: vc("KnockBackDirection"),
            knock_back_direction_vector: vc("KnockBackDirectionVector"),
            knock_back_mirror: flag(p, "KnockBackMirror"),
            knock_back_scaling: num(p, "KnockBackScaling"),
            knockback_socket_offset: vc("KnockbackSocketOffset"),
            knockback_target_socket: nm("KnockbackTargetSocket"),
            on_hit_audio: nm("OnHitAudio"),
            on_hit_sound: nm("OnHitSound"),
            on_hit_effect_override: nm("OnHitEffectOverride"),
            play_on_hit_effects: flag(p, "PlayOnHitEffects"),
            queue_hit_pause_until_poses: flag(p, "QueueHitPauseUntilPoses"),
            queue_hit_pause_until_pose: flag(p, "QueueHitPauseUntilPose"),
            reflect_projectiles: flag(p, "ReflectProjectiles"),
            vacuum_info: p.get("VacuumInfo").and_then(Value::as_obj).map(|v| VacuumInfo::from_props(names, v)),
            vacuum_knockback: flag(p, "VacuumKnockback"),
            velocity_instead_of_facing: flag(p, "VelocityInsteadOfFacing"),
            hit_box_tags,
            hit_box_scale: vc("HitBoxScale"),
            charge_hit_pause_mult: num(p, "ChargeHitPauseMult"),
            on_hit_buffs,
            elemental_buffs,
            specific_scales,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notify_row_reads_its_own_names() {
        let mut names = Names::default();
        let tag = names.intern("BodyHitBox");
        let mut p = Props::new();
        p.insert("Damage".into(), Value::Num(7.0));
        p.insert("KnockBackDirection".into(), Value::Obj([("X".to_owned(), Value::Num(1.0))].into_iter().collect()));
        p.insert("HitBoxTags".into(), Value::Arr(vec![Value::Str("BodyHitBox".into())]));
        p.insert("AdditiveKBMode".into(), Value::Num(2.0));
        let h = HitPacket::from_props(&names, &p, None);
        assert_eq!(h.damage, Some(7.0));
        assert_eq!(h.knock_back_direction, Some(DataVec { x: Some(1.0), y: None, z: None }));
        assert_eq!(h.knock_back_direction_vector, None);
        assert_eq!(h.hit_box_tags, vec![tag]);
        assert_eq!(h.additive_kb_mode, Some(DataEnum::Value(2)));
    }
}
