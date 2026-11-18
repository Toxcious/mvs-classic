//! Who hit the fighter and what it hit: the attackers, the combo counters, the attack ids and combo decay
//! (the fighter's fields, written by the hit code in `combat/`).

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::fighter::FighterId;
use crate::names::Name;
use indexmap::{IndexMap, IndexSet};

/// `comboReset`: the combo counter's pending reset, one object the World holds and every fighter shares
/// (`World.combo_reset`).
#[derive(State, Debug, Default)]
pub struct ComboReset {
    /// The fighter's id (a player's index, a minion's base + n), -1 none.
    pub fighter: i32,
    pub pending: bool,
    pub left: Real,
}

/// The attackers and the combo counters.
#[derive(State, Debug, Default)]
pub struct Attackers {
    pub most_recent_attacker: Option<FighterId>,
    pub most_recent_friendly_fire: Option<FighterId>,
    pub assist_attacker: Option<FighterId>,
    pub assist_clear_tick: i32,
    pub last_attacker_off_ledge: bool,
    /// `previousAttacker`: a fighter or an actor.
    pub previous_attacker: Option<ActorRef>,
    pub hit_by_multiple_attackers: bool,
    pub force_additive_kb: bool,
    pub hit_stunned_before_hit: bool,
    pub zero_out_vel_for_kb: bool,
    pub hit_stun_ended_bound: bool,
    pub di_combo: Real,
    pub combo_reset_tick: i32,
    pub juggle_count: i32,
    pub juggle_damage: Real,
    pub hitstun_count: i32,
    pub hitstun_damage: Real,
    pub stun_source: bool,
    pub knocked_back_count: i32,
    pub last_knocked_back: Real,
    pub last_decay_stacks: Real,
}

/// The attack ids and combo decay.
#[derive(State, Debug, Default)]
pub struct ComboDecay {
    pub attack_count: i32,
    pub landed_attack_ids: IndexSet<i32>,
    pub has_hit_a_fighter: bool,
    pub recent_hit_actor: Option<ActorRef>,
    pub attack_decay: Vec<Name>,
    pub current_decay_sequence: i32,
    pub landed_decay_sequences: Vec<i32>,
    pub combo_res_map: IndexMap<Name, Real>,
    pub combo_resistance: bool,
}

/// `DecayHit`: what RegisterHitFromFighter / GetHitStunDecay read from the hitbox.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DecayHit {
    pub attack_id: i32,
    pub decay_type: i32,
    /// GetAttackDecayFromHitbox: the montage or projectile class name (HitBox.AttackDecayName for types 1/2/3/6).
    pub decay_name: Name,
    /// GetComboResAttackNameFromHitbox.
    pub combo_name: Name,
    pub base_knock_back: Real,
    /// LocalHitInformation.BaseKnockBack x ChargeKnockBackMultiplier (the SetMostRecentAttacker gate); `None`:
    /// `base_knock_back`.
    pub launch_knock_back: Option<Real>,
    /// The hit's AdditiveKBMode; `None`: Normal.
    pub additive_kb_mode: Option<crate::combat::DataEnum>,
    pub knock_back_scaling: Option<Real>,
    pub hit_stun_mult: Option<Real>,
}
