//! A hit packet as the record writes it (`hit`: the Struct_HitBoxDamage object, its members by their data names).
//! A member of the struct is always written, `null` where the Rust packet has `None` (the data struct holds every
//! member); the Hitframe notify's own fields are left out where unset.

use mvs_sim::combat::packet::{DataEnum, DataVec, HitPacket, OnHitBuffRow, VacuumInfo};
use mvs_trace::Value;

use crate::conv::{Plain, num, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

/// An enum member: its enumerator's name (`AdditiveKBMode::NewEnumerator0`) or its byte value.
impl Plain for DataEnum {
    fn to_value(&self, x: &Wx) -> Value {
        match self {
            DataEnum::Name(n) => n.to_value(x),
            DataEnum::Value(v) => Value::Num(f64::from(*v)),
        }
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(e) = Self::from_value(v, l) {
            *self = e;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        match v {
            Value::Str(s) => Some(DataEnum::Name(l.name(s))),
            v => crate::conv::int(num(v, l)?, l).map(DataEnum::Value),
        }
    }
}

plain_struct!(DataVec { "X" ~ x, "Y" ~ y, "Z" ~ z });

plain_struct!(VacuumInfo {
    "UseVacuumKnockBack" ~ use_vacuum_knock_back, "VacuumStrengthAlpha" ~ vacuum_strength_alpha,
    "VacuumTowardsInstigatingFighter" ~ vacuum_towards_instigating_fighter,
    "VacuumTargetSocket" ~ vacuum_target_socket, "VacuumSocketOffset" ~ vacuum_socket_offset,
    "VacuumDuration" ~ vacuum_duration, "VacuumPause" ~ vacuum_pause,
    "ForceSmallHitEffects" ~ force_small_hit_effects, "AccountForAttackerVelocity" ~ account_for_attacker_velocity,
});

// Unverified: Struct_OnHitBuffs's member names; no trace holds a row.
plain_struct!(OnHitBuffRow {
    "AppliedBuffs" = applied_buffs, "ApplyBuffSelf" = apply_buff_self, "ApplyBuffHit" = apply_buff_hit,
    "ApplyBuffAlly" = apply_buff_ally, "ApplyBuffEnemy" = apply_buff_enemy,
    "RequireHitToApplyBuffToSelf" = require_hit_to_apply_buff_to_self, "StackCount" ~ stack_count,
    "BuffDuration" ~ buff_duration,
});

// The struct's members are written, the form the traces hold (a projectile's or an actor's HitBoxDamage); a Hitframe
// notify's own packet is not in the record (GAP_UNMAPPED), so its KnockBackDirection, OnHitSound and
// QueueHitPauseUntilPose have no key.
macro_rules! hit_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
    "AccountForVelocityWithVacuumKnockback" = account_for_velocity_with_vacuum_knockback,
    "AdditiveKBMode" = additive_kb_mode, "ApplyHitPauseToDamageDealer" = apply_hit_pause_to_damage_dealer,
    "ArmorPierce" = armor_pierce, "AttackAnimation" = attack_animation, "AttackDecayName" = attack_decay_name,
    "AttackDecayType" = attack_decay_type, "AttackID" = attack_id, "AttackTag" = attack_tag,
    "BaseKnockBack" = base_knock_back, "ControllerRumble" = controller_rumble, "Damage" = damage,
    "DestroyProjectiles" = destroy_projectiles, "DirectKnockbackTowardsSocket" = direct_knockback_towards_socket,
    "DirectionalKnockback" = directional_knockback, "DoLOSCheck" = do_los_check,
    "ForceKnockback" = force_knockback, "HitEachActorOnlyOnce" = hit_each_actor_only_once,
    "HitOnlyOneFighter" = hit_only_one_fighter, "HitPauseTime" = hit_pause_time, "HitReaction" = hit_reaction,
    "HitStunMult" = hit_stun_mult, "IgnoreActor" = ignore_actor, "IgnoreFacing" = ignore_facing,
    "IgnoreKBI" = ignore_kbi, "KBI_Multiplier" = kbi_multiplier, "KBScalingStartingDmg" = kb_scaling_starting_dmg,
    "KnockBackDirectionVector" = knock_back_direction_vector, "KnockBackMirror" = knock_back_mirror,
    "KnockBackScaling" = knock_back_scaling, "KnockbackSocketOffset" = knockback_socket_offset,
    "KnockbackTargetSocket" = knockback_target_socket, "OnHitAudio" = on_hit_audio,
    "OnHitEffectOverride" = on_hit_effect_override, "PlayOnHitEffects" = play_on_hit_effects,
    "QueueHitPauseUntilPoses" = queue_hit_pause_until_poses, "ReflectProjectiles" = reflect_projectiles,
    "VacuumInfo" = vacuum_info, "VacuumKnockback" = vacuum_knockback,
    "VelocityInsteadOfFacing" = velocity_instead_of_facing,
    "HitBoxTags" ~ hit_box_tags, "HitBoxScale" ~ hit_box_scale, "ChargeHitPauseMult" ~ charge_hit_pause_mult,
    "OnHitBuffs" ~ on_hit_buffs,
)
    };
}

// A packet the oracle built as an object literal (`sparse`) has its unset members absent from the record: they are left
// out here, and a record that lacks a `=` member loads as sparse.
impl Plain for HitPacket {
    fn to_value(&self, x: &Wx) -> Value {
        let mut o: Vec<(String, Value)> = Vec::new();
        hit_keys!(plain_keys put self, x, o;);
        if self.sparse {
            o.retain(|(_, v)| !matches!(v, Value::Null));
        }
        Value::Obj(mvs_trace::Obj::new(o))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(ob) = crate::conv::as_obj(v, l) else { return };
        self.sparse = hit_keys!(plain_keys absent ob;);
        if self.sparse {
            hit_keys!(plain_keys load_sparse self, ob, l;);
        } else {
            hit_keys!(plain_keys load self, ob, l;);
        }
        crate::conv::check_keys(ob, hit_keys!(plain_keys names), l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        crate::conv::via_default(v, l, <HitPacket as Plain>::load_value)
    }
}
