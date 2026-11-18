//! Comp_C017_Hover_Rage: the thruster fuel and the rage that pays for
//! weapon mode. Comp_C017_Hover_Rage ubergraph offsets; class defaults from Default__Comp_C017_Hover_Rage_C.
//!
//! The component's calls back into the pawn (owner closures) are not kept; here its
//! functions take the World and the fighter, and the owner's side (onWall, walking, changeSection, maxRage,
//! outOfRage) is pawn.rs's.

use super::pawn::out_of_rage;
use super::state::giant_w;
use crate::Real;
use crate::State;
use crate::combat::conditions::is_on_wall;
use crate::consts::DT;
use crate::fighter::anim::{MontageSlot, slot_jump_to_section};
use crate::fighter::{FighterId, grounded};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::CharHooks;
use crate::world::World;

// Default__Comp_C017_Hover_Rage_C
pub const FUEL_START: Real = 75.0;
pub const FUEL_MAX: Real = 100.0;
/// Seconds from empty to full.
pub const CHARGE_SPEED: Real = 2.0;
/// Seconds of hover on a full tank.
pub const MAX_HOVER_TIME: Real = 3.0;
pub const RAGE_MAX: Real = 100.0;
/// Seconds of weapon mode on a full meter.
pub const RAGE_TIME: Real = 12.0;
pub const ON_HIT_RAGE_MULT: Real = 0.8;
pub const ON_ALLY_HIT_RAGE_MULT: Real = 1.0;
pub const ON_ALLY_DEATH_RAGE_PERCENT: Real = 0.35;
/// PrimaryComponentTick.TickInterval 0.1 s: every 6th frame at the fixed step, as C019_CookGrenade's actor tick.
// Unverified: the frame the first tick falls on and the interval's carry-over are not read in the exe.
pub const HOVER_RAGE_TICK_FRAMES: i32 = 6;

/// `HoverRage`.
#[derive(State, Debug)]
pub struct HoverRage {
    pub fuel: Real,
    pub using_fuel: bool,
    /// 0 hover, 1 an attack (AnimNotify_C017_Use_Fuel).
    pub fuel_use_type: i32,
    pub attack_fuel_percent: Real,
    pub attack_section: Name,
    pub attack_montage: Name,
    /// The DoOnce on the empty tank's section jump (Temp_bool_IsClosed_Variable_2 @1899), reopened @38.
    pub empty_jumped: bool,
    pub rage: Real,
    pub using_weapon_mode: bool,
    pub respawn_rage: Real,
    /// For the HUD: a count of the SetFuelLevel calls and the value of the last one. Sim
    /// state: the HUD compares the count.
    pub fuel_ui: i32,
    pub fuel_ui_value: Real,
    /// The FuelAmount last sent as the cha_C017_hover_fuel RTPC (Pawn_C017 @3665), -1 none.
    pub fuel_rtpc: Real,
    /// The frames counted towards the component's next tick.
    pub frames: i32,
}

impl Default for HoverRage {
    /// The field initial values.
    fn default() -> Self {
        HoverRage {
            fuel: FUEL_START,
            using_fuel: false,
            fuel_use_type: 0,
            attack_fuel_percent: 0.0,
            attack_section: Name::NONE,
            attack_montage: Name::NONE,
            empty_jumped: false,
            rage: 0.0,
            using_weapon_mode: false,
            respawn_rage: 0.0,
            fuel_ui: 0,
            fuel_ui_value: 0.0,
            fuel_rtpc: -1.0,
            frames: 0,
        }
    }
}

impl HoverRage {
    /// C017_Fuel_Check_Condition: FuelAmount >= FuelPercent x FuelMaxAmount.
    pub fn has_fuel(&self, percent: Real) -> bool {
        self.fuel >= percent * FUEL_MAX
    }

    /// `fuelFraction`.
    pub fn fuel_fraction(&self) -> Real {
        self.fuel / FUEL_MAX
    }

    /// `rageFraction`.
    pub fn rage_fraction(&self) -> Real {
        self.rage / RAGE_MAX
    }

    /// SetFuelLevel @4716.
    fn set_fuel_level(&mut self, value: Real) {
        self.fuel_ui += 1;
        self.fuel_ui_value = value;
    }

    /// OnRep_FuelAmount (@441 of its own graph).
    fn on_rep(&mut self) {
        self.set_fuel_level(self.fuel / FUEL_MAX);
    }

    /// `clamped`.
    fn clamped(&self, add: Real) -> Real {
        max(min(add + self.rage, RAGE_MAX), 0.0)
    }
}

/// The component of a Giant (made by `init`).
fn hr(w: &mut World, f: FighterId) -> &mut HoverRage {
    giant_w(w, f).hover_rage.as_mut().expect("an Iron Giant's hover rage (made by init)")
}

/// `HoverRageOwner.changeSection`: Comp_Pawn_MontageNavigation.NotifyChangeSection(section, montage).
fn change_section(w: &mut World, f: FighterId, section: Name, montage: Name) {
    let on = w.fighter(f).montages.montage.as_ref().is_some_and(|m| m.montage.name == montage);
    if on && section != Name::NONE && section != name!("None") {
        slot_jump_to_section(w, f, MontageSlot::Main, section);
    }
}

/// ReceiveTick @2651 (`HoverRage.tick`, server). `dilation`: the owner's CustomTimeDilation (component ticks carry
/// it).
pub fn tick(w: &mut World, f: FighterId, dilation: Real) {
    {
        let h = hr(w, f);
        h.frames += 1;
        if h.frames < HOVER_RAGE_TICK_FRAMES {
            return;
        }
        h.frames = 0;
    }
    let dt = DT * crate::real(HOVER_RAGE_TICK_FRAMES) * dilation;
    // fuel use @2869
    let h = hr(w, f);
    if h.using_fuel {
        if h.fuel_use_type == 0 {
            // @3977: only while fuel is left; @235 -> @197 OnRep, SetFuelMulticast
            if h.fuel > 0.0 {
                h.fuel = max(min(h.fuel - (dt * (FUEL_MAX / MAX_HOVER_TIME)), FUEL_MAX), 0.0);
                h.on_rep();
                h.on_rep();
            }
        } else if h.fuel_use_type == 1 {
            if h.fuel > 0.0 {
                // @4327; @406 -> @387 OnRep, @15 SetFuelMulticast
                h.fuel = max(min(h.fuel - (dt * (FUEL_MAX * h.attack_fuel_percent)), FUEL_MAX), 0.0);
                h.on_rep();
                h.on_rep();
                h.empty_jumped = false;
            } else if !h.empty_jumped {
                // @2051 / @1899: once per empty stretch
                h.empty_jumped = true;
                let (section, montage) = (h.attack_section, h.attack_montage);
                change_section(w, f, section, montage);
            }
        }
    }
    // refill @2992
    let on_ground = is_on_wall(w, f) || grounded(w, f);
    let h = hr(w, f);
    if on_ground && !h.using_fuel {
        h.fuel = min((dt * (FUEL_MAX / CHARGE_SPEED)) + h.fuel, FUEL_MAX);
        // @61: OnRep, SetFuelMulticast
        h.on_rep();
        h.on_rep();
    }
    // @127-173, on either branch
    h.set_fuel_level(h.fuel / FUEL_MAX);
    // @3503: the ReachedMaxFuel / StartedUsingFuel broadcasts have no binder in the decoded Blueprints (only this
    // component names them), so their gates are not kept.
    // rage drain @3607
    if h.using_weapon_mode {
        h.rage = min(max(h.rage - ((RAGE_MAX / RAGE_TIME) * dt), 0.0), RAGE_MAX);
        if h.rage <= 0.0 {
            h.using_weapon_mode = false;
            out_of_rage(w, f);
        }
    }
}

/// SetUsingFuel @5299 (`setAttackFuel`; AnimNotify_C017_Use_Fuel Begin / End).
pub fn set_attack_fuel(w: &mut World, f: FighterId, on: bool, percent: Real, section: Name, montage: Name) {
    let h = hr(w, f);
    if !on {
        h.fuel_use_type = 0;
        h.using_fuel = false;
        return;
    }
    h.attack_section = section;
    h.attack_montage = montage;
    h.attack_fuel_percent = percent;
    h.fuel_use_type = 1;
    h.using_fuel = true;
    if h.fuel <= 0.0 {
        change_section(w, f, section, montage);
    }
}

/// UseFuel @5124.
pub fn use_fuel(w: &mut World, f: FighterId, percent: Real) {
    let h = hr(w, f);
    h.fuel = min(max(h.fuel - (FUEL_MAX * percent), 0.0), FUEL_MAX);
    // @353 -> @269 OnRep, @329 SetFuelLevel
    h.on_rep();
    h.set_fuel_level(h.fuel / FUEL_MAX);
}

/// AddRage @6940: a new total, ignored in weapon mode; the max broadcast has no once gate.
fn add_rage(w: &mut World, f: FighterId, total: Real) {
    let h = hr(w, f);
    if h.using_weapon_mode {
        return;
    }
    h.rage = total;
    if h.rage / RAGE_MAX >= 1.0 {
        giant_w(w, f).is_max_rage = true; // ReachedMaxRage (@13997)
    }
}

/// HitRageCalculation @6401 (`HoverRage.ownHit`): the owner's hit on a fighter of another team, by the hit box's
/// Damage. Called by `state::hover_rage_own_hit` (shared code's `dealtRage`) on a Giant that has the component.
pub fn own_hit(w: &mut World, f: FighterId, hit_box_damage: Real) {
    let total = hr(w, f).clamped(hit_box_damage * ON_HIT_RAGE_MULT);
    add_rage(w, f, total);
}

/// AllyHitRageBuild @7108 (an ally's HitByEnemy).
pub fn ally_hit(w: &mut World, f: FighterId, damage: Real) {
    let total = hr(w, f).clamped(ON_ALLY_HIT_RAGE_MULT * damage);
    add_rage(w, f, total);
}

/// OnAllyDeathRage @7356 (its OnExitedArena).
pub fn ally_death(w: &mut World, f: FighterId) {
    let total = hr(w, f).clamped(ON_ALLY_DEATH_RAGE_PERCENT * RAGE_MAX);
    add_rage(w, f, total);
}

/// ResetRage @7011: ForceEndRage broadcasts OutOfRage whether or not the mode was on.
pub fn reset_rage(w: &mut World, f: FighterId, from_respawn: bool) {
    let h = hr(w, f);
    h.rage = if from_respawn { h.respawn_rage } else { 0.0 };
    h.using_weapon_mode = false;
    out_of_rage(w, f);
    if hr(w, f).rage / RAGE_MAX >= 1.0 {
        giant_w(w, f).is_max_rage = true;
    }
}

/// SetRespawnRage @7700 (`HoverRage.setRespawnRage`), called by Buff_C017_FastWeaponMode (perks.rs) on a Giant that
/// has the component: a value other than RespawnRage (NotEqual_FloatFloat @7738) -> AddRage(New) @7748 (a new total,
/// ignored in weapon mode), then RespawnRage = New @7771.
pub fn set_respawn_rage(w: &mut World, f: FighterId, value: Real) {
    let v = value;
    if v == hr(w, f).respawn_rage {
        return;
    }
    add_rage(w, f, v);
    hr(w, f).respawn_rage = v;
}

/// OnOwnerReset @7670: ResetFuelLevel, ResetRage(true).
pub fn owner_reset(w: &mut World, f: FighterId) {
    // ResetFuelLevel @5005: SetFuelLevel(FuelMaxAmount), then SetFuelMulticast(FuelMaxAmount) and its OnRep
    let h = hr(w, f);
    h.set_fuel_level(FUEL_MAX);
    h.fuel = FUEL_MAX;
    h.on_rep();
    reset_rage(w, f, true);
}

/// Comp_C017_Hover_Rage ReceiveTick.
// Unverified: its place among the owner's component ticks is not read.
fn component_tick(w: &mut World, f: FighterId, dilation: Real) {
    tick(w, f, dilation);
}

/// The hooks of this file in Iron Giant's CharHooks (the component's tick).
pub fn hooks(h: &mut CharHooks) {
    h.component_tick = Some(component_tick);
}
