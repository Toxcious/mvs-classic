//! A character's hooks (`CharHooks`, the `CHARS` table): what the shared fighter code
//! calls on the character it runs. A fighter's entry is bound at
//! `World::new` (the World keeps `&'static CharHooks` per fighter) and a hook is copied out before its call.

#![allow(
    clippy::type_complexity,
    reason = "a hook table: each hook's signature reads clearest in place, beside its name"
)]

use crate::Real;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::{SetRef, SlotMix};
use crate::names::Name;
use crate::registry::signals::FighterSignal;
use crate::world::World;

/// `getTetherInfo`'s answer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TetherInfo {
    pub out: bool,
    pub attached_to_ally: bool,
}

/// `AllyAttachComp`: IsAttached and the call that sets it (Attach to Actor), which the character's module runs.
#[derive(Clone, Copy, Debug)]
pub struct AllyAttach {
    pub is_attached: bool,
    pub attach_to_actor: fn(&mut World, FighterId, Option<FighterId>, bool),
}

/// `lastDashTarget`'s answer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DashTarget {
    pub x: Real,
    pub y: Real,
    pub actor_y: Option<Real>,
}

/// `CharHooks`. `state` makes the fighter's [`CharState`]; `stateSkip` is the state type's own
/// `#[state(skip)]` fields.
#[derive(Clone, Copy, Debug, Default)]
pub struct CharHooks {
    pub state: Option<fn(&World, FighterId) -> CharState>,
    pub init: Option<fn(&mut World, FighterId)>,
    pub set_facing_override: Option<fn(&mut World, FighterId, bool)>,
    pub holding_item: Option<fn(&mut World, FighterId, bool)>,
    pub set_is_being_held: Option<fn(&mut World, FighterId, bool)>,
    /// `equip(f, p, begin)`: `p` the notify's parameters.
    pub equip: Option<fn(&mut World, FighterId, NotifyRef, bool)>,
    /// `spawner(f, p)`: `p` the spawn notify's parameters.
    pub spawner: Option<fn(&mut World, FighterId, NotifyRef)>,
    pub after_spawned_actor: Option<fn(&mut World, FighterId, ActorRef)>,
    pub jump_pressed: Option<fn(&mut World, FighterId)>,
    pub input_pre_triggered: Option<fn(&mut World, FighterId, i32, i32)>,
    pub evade_direction: Option<fn(&World, FighterId, i32) -> i32>,
    pub tick: Option<fn(&mut World, FighterId)>,
    pub end_jump: Option<fn(&mut World, FighterId)>,
    pub component_tick: Option<fn(&mut World, FighterId, Real)>,
    pub world_tick: Option<fn(&mut World, FighterId)>,
    pub timer_tick: Option<fn(&mut World, FighterId)>,
    pub attack_started: Option<fn(&mut World, FighterId)>,
    pub attack_ended: Option<fn(&mut World, FighterId)>,
    pub pressed: Option<fn(&mut World, FighterId, crate::input::AttackBtn)>,
    pub montage_ended: Option<fn(&mut World, FighterId, Name)>,
    pub knocked_back: Option<fn(&mut World, FighterId, Real, Real, Real, Option<ActorRef>)>,
    pub capsule_hit: Option<fn(&mut World, FighterId)>,
    pub crouch_changed: Option<fn(&mut World, FighterId, bool)>,
    pub hit_stun_start: Option<fn(&mut World, FighterId)>,
    pub hit_stun_ended: Option<fn(&mut World, FighterId)>,
    pub montage_blending_out: Option<fn(&mut World, FighterId, Name)>,
    pub montage_started: Option<fn(&mut World, FighterId, Name)>,
    pub charge_end: Option<fn(&mut World, FighterId, NotifyRef)>,
    pub charge_started: Option<fn(&mut World, FighterId, NotifyRef)>,
    pub hit_ally: Option<fn(&mut World, FighterId, FighterId)>,
    pub hit_ally_projectile: Option<fn(&mut World, FighterId, ActorRef)>,
    pub reads_recent_hit_actor: bool,
    pub reflected_projectile: Option<fn(&mut World, FighterId, ActorRef)>,
    pub hit_enemy: Option<fn(&mut World, FighterId, FighterId)>,
    /// `hitAnything(w, f, victim, hit, hitBox)`: `hit` is the packet the attacker dealt.
    /// The victim, the hit (HitBoxInfo.HitBoxDamage after the dealt mods), IsValid(HitCollision).
    pub hit_anything: Option<fn(&mut World, FighterId, ActorRef, &crate::combat::HitPacket, bool)>,
    pub armor_break: Option<fn(&mut World, FighterId, Name, Option<FighterId>, bool)>,
    pub decrement_shield_charge: Option<fn(&mut World, FighterId)>,
    pub ability_ammo_updated: Option<fn(&mut World, FighterId, i32, Real, bool)>,
    pub attack_decay_inserted: Option<fn(&mut World, FighterId)>,
    pub hit_pause_time_changed: Option<fn(&mut World, FighterId, Real)>,
    pub ability_cooldown_updated: Option<fn(&mut World, FighterId, i32, bool)>,
    pub updated_sub_mesh: Option<fn(&mut World, FighterId, Name, bool)>,
    pub blocked_projectile: Option<fn(&mut World, FighterId)>,
    pub ally_hit_by_enemy: Option<fn(&mut World, FighterId, FighterId, Real)>,
    pub evaded_attack: Option<fn(&mut World, FighterId, Option<FighterId>)>,
    pub ally_exited_arena: Option<fn(&mut World, FighterId, FighterId)>,
    /// A fighter's (a minion's too) OnExitedArena, for a pawn whose component bound it on that fighter (the hook
    /// tests its own bindings): called for every other fighter, any team.
    pub fighter_exited_arena: Option<fn(&mut World, FighterId, FighterId)>,
    pub bound_signal: Option<fn(&mut World, FighterId, FighterId, FighterSignal, bool, Real, Name, Option<FighterId>)>,
    pub death: Option<fn(&mut World, FighterId)>,
    pub initial_visibility: Option<fn(&mut World, FighterId)>,
    pub reset: Option<fn(&mut World, FighterId)>,
    pub other_reset: Option<fn(&mut World, FighterId, FighterId)>,
    pub match_reset: Option<fn(&mut World, FighterId)>,
    pub minion_defeated: Option<fn(&mut World, FighterId, FighterId)>,
    pub turned: Option<fn(&mut World, FighterId)>,
    pub anim_update: Option<fn(&mut World, FighterId, Real, bool)>,
    pub seq_players: Option<fn(&mut World, FighterId, Option<&mut Vec<i32>>)>,
    /// `pose(f, s)`: the pose scratch; true when the character posed it.
    pub pose: Option<fn(&World, FighterId, &mut [f32]) -> bool>,
    pub slot_source: Option<fn(&World, FighterId, &mut [f32])>,
    pub slot_mix: Option<fn(&World, FighterId) -> Option<&SlotMix>>,
    /// `slotMix` written: the same mix as `slot_mix`, mutable (a character with `slot_mix` has both).
    pub slot_mix_mut: Option<fn(&mut World, FighterId) -> Option<&mut SlotMix>>,
    pub slot_input: Option<fn(&World, FighterId, Name) -> Real>,
    pub slot_layer: Option<fn(&World, FighterId, Name) -> Real>,
    pub pose_updated: Option<fn(&mut World, FighterId)>,
    pub nav_pose: Option<fn(&mvs_data::fighter::Montage) -> bool>,
    pub layer_held: Option<fn(&World, FighterId) -> bool>,
    pub idle_anim: Option<fn(&World, FighterId) -> Name>,
    pub wall_blend_space_value: Option<fn(&World, FighterId) -> Real>,
    pub anim_set: Option<fn(&World, FighterId) -> SetRef>,
    pub branch_group_blocked: Option<fn(&mut World, FighterId, NotifyRef, Name, Real)>,
    pub branch_group_end_blocked: Option<fn(&mut World, FighterId, NotifyRef)>,
    pub activate_branch_override: Option<fn(&mut World, FighterId, bool)>,
    pub disguise_has_attack_branches: Option<fn(&World, FighterId) -> bool>,
    pub normal_hitboxes_off: Option<fn(&World, FighterId) -> bool>,
    pub movement_stop_anim: Option<fn(&mut World, FighterId, Name, Real)>,
    pub player_knocked_out: Option<fn(&mut World, FighterId, FighterId)>,
    pub occluded_event: Option<fn(&mut World, FighterId)>,
    pub not_occluded_event: Option<fn(&mut World, FighterId)>,
    pub is_occluded: Option<fn(&World, FighterId) -> bool>,
    pub get_speed: Option<fn(&World, FighterId) -> bool>,
    pub get_tether_info: Option<fn(&World, FighterId) -> TetherInfo>,
    pub landed_on_solid_ground: Option<fn(&mut World, FighterId)>,
    pub spawner_filtered: Option<fn(&mut World, FighterId, Name)>,
    pub set_com_angle: Option<fn(&mut World, FighterId, Real, bool, Real, Real)>,
    pub last_dash_target: Option<fn(&World, FighterId) -> Option<DashTarget>>,
    /// `allyAttach`: the fighter's AllyAttach component (`AllyAttachComp`), when it has one.
    pub ally_attach: Option<fn(&World, FighterId) -> Option<AllyAttach>>,
    /// `onStruggling`: the Struggling broadcast of fighter `v` (the second argument), asked of
    /// this fighter (the first), which answers for the bindings it holds in its own state (Comp_JakeEat's).
    pub struggling: Option<fn(&mut World, FighterId, FighterId)>,
    /// `bindHoldCharge`: the pawn's handlers of Comp_HoldCharge's three delegates
    /// ([`crate::combat::hits::bind_hold_charge`]).
    pub hold_charge: Option<crate::combat::hits::HoldChargeHandlers>,
}

/// `f.hooks?.slotMix?.(f)` written: the character's slot mix, mutable
/// (the `slot_mix` hook reads it), by the character's `slot_mix_mut` hook; `None` for a character without one.
pub fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    let h = w.fighter(f).hooks.char.slot_mix_mut?;
    h(w, f)
}
