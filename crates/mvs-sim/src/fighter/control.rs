//! Input, abilities, the charge attack and the evade.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::world_actor::{world_actor_delayed_dodge_text, world_actor_dodge};
use crate::combat::buffs::buff_signal;
use crate::combat::defense::rage_release_input;
use crate::combat::disguise::mirror_section_jump;
use crate::combat::disguise::stop_copy_montage;
use crate::combat::hit::timer_ticks;
use crate::combat::hits::set_hold_charge_multiplier;
use crate::consts::AIR_SPECIAL_MAX;
use crate::consts::DELTA_SECONDS;
use crate::consts::DT;
use crate::consts::{AIR_EVADE_MAX, EVADE_BLINK_FRESNEL, EVADE_METER_COST, EVADE_RECHARGE_AMOUNT, MAX_EVADE_COUNT};
use crate::consts::{ATTACK_SPAM_WINDOW, RECENT_ATTACK_QUEUE};
use crate::events::Event;
use crate::events::Fid;
use crate::events::NotifyRef;
use crate::events::PopKind;
use crate::events::SoundKey;
use crate::events::UiAbilityOp;
use crate::events::UiStateOp;
use crate::fighter::Fighter;
use crate::fighter::FighterId;
use crate::fighter::anim::ForcedPress;
use crate::fighter::anim::GroupedEntry;
use crate::fighter::anim::{
    ChargeBtn, MontageKey, MontageSlot, MontageState, QueuedBtn, active_instance, owner_of, resolve,
};
use crate::fighter::anim::{
    LateBranch, Override, QueuedPress, RowOkR, active_grouped, buffer_press, clear_queued_attack, open_windows,
    override_attack, row_ok, stop_dash_montage, stop_grouped_entry, taunt_branch, window_press, write_press,
};
use crate::fighter::anim::{
    SetRef, active_of, clear_foreign_windows, conds_ok, has_active, proxy_clear, start_montage, stop_active_grouped,
    stop_anim_montage, stop_montage,
};
use crate::fighter::body::{PlatformRef, handle_platform_collision};
use crate::fighter::common::strip_enum;
use crate::fighter::common::{actor_by, latent_frames, timer_frames, warn_text};
use crate::fighter::match_side::branch_row_tutorial;
use crate::fighter::match_side::{
    EvadeTutorialByteKind, TutorialEvent, evade_tutorial_byte, handle_air_move_desaturation, tutorial_event_of,
    tutorial_world,
};
use crate::fighter::match_side::{branch_attack_tutorial, free_attack_tutorial, item_override};
use crate::fighter::match_side::{post_vo, sfx};
use crate::fighter::moves::{
    attack_enabled, branch_evade, clear_attack_cancelled, evade_enabled, is_falling, is_flying, jump_enabled,
    max_jumps, try_jump,
};
use crate::fighter::moves::{
    clear_evade_recharge_fold, end_jump_at_start, evade_base_invuln, evade_play_rate, evade_recharge,
    lateral_friction_event,
};
use crate::fighter::moves::{dash_dance, set_crouching};
use crate::fighter::moves::{input_type_direction_enabled, input_type_disabled_any};
use crate::fighter::signal_bound;
use crate::fighter::{grounded, has_attack_inst, in_air, in_hitstun, is_attacking, set_facing};
use crate::input::AttackBtn;
use crate::input::get_direction;
use crate::input::{Btn, Dir, InputState};
use crate::input::{get8_way, get8_way_no_facing};
use crate::math as js;
use crate::names::Name;
use crate::registry::entries::ActorHooks;
use crate::registry::signals::FighterSignal;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::setup::InputPrefs;
use crate::world::World;
use crate::world::flow::{DelegateCall, DelegateList, PawnList, broadcast_bound};
use crate::world::spawned_platform::platform_can_pass_through;
use indexmap::IndexMap;
use mvs_anim::montage::{
    MontagePlayer, MontageProxy, PROXY_ATTACK, PROXY_BRANCH_ATTACK, PROXY_EVADE, PROXY_EVADE_DROP, PROXY_NONE,
};
use mvs_data::def::{AbilityDef, NavSet};
use mvs_data::fighter::{AttackEntry, Branch, Montage, Notify};
use mvs_data::value::Props;
use mvs_data::value::Value;
use mvs_data::{CharData, GameData};

/// `spamDelayed`: a press held back by the attack spam rule.
#[derive(State, Debug)]
pub struct SpamDelayed {
    pub b: Btn,
    pub d: Dir,
    pub at: Real,
    pub force: Option<u8>,
    pub u: Option<Dir>,
}

/// The input and what the press rules keep.
#[derive(State, Debug, Default)]
pub struct Controls {
    pub prev: InputState,
    pub inp: InputState,
    /// The player's input preferences: the set-up's, not state.
    #[state(skip)]
    pub prefs: InputPrefs,
    pub bot_controlled: bool,
    /// `recentAttacks`: button and montage.
    pub recent_attacks: Vec<(Btn, Option<Name>)>,
    pub attack_spam_times: IndexMap<Option<Name>, Real>,
    pub spam_delayed: Option<SpamDelayed>,
    pub rot_lock_until: Real,
    pub rot_thru_atk_count: Option<i32>,
    /// BPI-written: the taunt slot.
    pub taunt_slot: Option<i32>,
    pub block_taunt_anims: bool,
    /// The UI state operations of the step, for the HUD: output, not state.
    #[state(skip)]
    pub ui_state_ops: Vec<UiStateOp>,
    /// `preSent`: this fighter is the one the frame's pre-triggers were last sent for (at
    /// most one fighter of the World holds it); a marker between calls, not state.
    #[state(skip)]
    pub pre_sent: bool,
    /// `preMask`: the buttons whose pre-trigger went out, while `pre_sent`.
    #[state(skip)]
    pub pre_mask: i32,
    /// `jumpSent`: as `pre_sent`, for the jump press's dispatch.
    #[state(skip)]
    pub jump_sent: bool,
}

/// `AbilityState`.
#[derive(State, Debug, Default)]
pub struct AbilityState {
    pub cooldown: bool,
    pub cd_frames: Real,
    pub ammo: Real,
    pub ammo_cooldown: bool,
    pub ammo_frames: Real,
    /// DefaultAbilityCooldown.
    pub default_cd: Real,
    /// ActiveAbilityCooldown.
    pub active_cd: Real,
    /// AmmoCooldown (GetMaxAmmoCooldown).
    pub ammo_max: Real,
}

/// Abilities and cooldowns.
#[derive(State, Debug, Default)]
pub struct Abilities {
    pub abilities: Vec<AbilityState>,
    pub attack_slot: i32,
    pub in_ability_timers: bool,
    pub instant_cooldowns: bool,
    pub cooldown_multiplier: Real,
    pub block_cooldowns: Option<Real>,
    /// `abilityOver`: the rows a graph wrote over the template's and passed to OverrideAbility, by slot.
    pub ability_over: Option<IndexMap<i32, AbilityOver>>,
    pub disable_cd: bool,
}

/// `HoldCharge`: Comp_HoldCharge's variables.
#[derive(State, Debug)]
pub struct HoldCharge {
    /// ChargePack (TMap<int, float>: the seconds held per charge number), in the map's order: keys[i] -> pack[i].
    pub keys: Vec<i32>,
    pub pack: Vec<Real>,
    /// ChargePackDefaults (BeginPlay @1548: the class data's ChargePack, each pack then set to 0 @30-442).
    pub defaults: Vec<Real>,
    /// The hold timer (K2_SetTimerDelegate(TimerEvent, (Default - Pack) x ChargeMultiplier, false) @1416, its handle
    /// Timer @1473): 0 none, -1 pending (set this frame: it starts in the frame's timer pass), else the timer passes
    /// left before it fires.
    pub timer: Real,
    /// The passes the timer fires after (`timerTicks`).
    pub timer_ticks: Real,
    /// ChargeMultiplier (EndHoldCHarge reads elapsed / ChargeMultiplier @105).
    pub multiplier: Real,
    /// AtMaxCharge.
    pub at_max: bool,
    /// LastHoldCHarge (UpdateChargeValues @1996).
    pub last_hold: Real,
}

impl Default for HoldCharge {
    /// The field initialisers.
    fn default() -> Self {
        HoldCharge {
            keys: Vec::new(),
            pack: Vec::new(),
            defaults: Vec::new(),
            timer: 0.0,
            timer_ticks: 0.0,
            multiplier: 1.0,
            at_max: false,
            last_hold: 0.0,
        }
    }
}

/// `chargeTimer`.
#[derive(State, Debug, Default)]
pub struct ChargeTimer {
    pub left: Real,
    pub ticks: Real,
    pub sec: Option<Name>,
}

/// The charge attack and the attacking flags.
#[derive(State, Debug, Default)]
pub struct ChargeAttack {
    pub charge_mult: Real,
    pub charge_dmg_mult: Real,
    pub charge_buff_mult: Real,
    pub charge_notify: i32,
    pub charge_start_time: Real,
    /// `chargeUi`: start, total.
    pub charge_ui: (Real, Real),
    /// How many times `chargeUi` was written, an equal value included: the HUD sends SetChargeAttackForPlayer on every
    /// write, as the build does. Not state: a restore keeps it counting up (a count taken back could equal the HUD's
    /// last seen one and hide a write), and it is no part of the hash or the trace.
    #[state(skip)]
    pub charge_ui_writes: u32,
    pub total_charged_time: Real,
    pub charge_released: bool,
    pub charge_attack_multiplier: Option<Real>,
    pub charge_total_time: Option<Real>,
    pub charge_timer: Option<ChargeTimer>,
    pub hold_charge: Option<HoldCharge>,
    pub charge_pct: Real,
    pub charge_hit_small_at: Real,
    pub charge_hit_large_at: Real,
    pub begin_attack: bool,
    pub attack_is_active: i32,
    pub anim_attack_off: i32,
    pub layer_missed: bool,
}

impl ChargeAttack {
    /// Every write of `chargeUi` goes through here, so the write count sees it.
    pub fn set_charge_ui(&mut self, v: (Real, Real)) {
        self.charge_ui = v;
        self.charge_ui_writes = self.charge_ui_writes.wrapping_add(1);
    }
}

/// A dodging fighter's entry of `DodgeState.fighters`.
#[derive(State, Debug, Default)]
pub struct DodgeEntry {
    /// The AttackID the entry holds (`null`: none yet).
    pub id: Option<i32>,
    /// The frame each AttackID was last over the fighter, by AttackID.
    pub seen: IndexMap<i32, i32>,
    /// The frame the entry was last confirmed in.
    pub at: i32,
}

/// `DodgeState`: a Comp_Pawn_HitBoxHandler's dodge variables, on a fighter or a spawned actor.
#[derive(State, Debug, Default)]
pub struct DodgeState {
    /// By the dodging fighter's id.
    pub fighters: IndexMap<i32, DodgeEntry>,
    pub gate: Real,
    pub fx_gate: Real,
    pub quest_gate: Real,
}

/// The evade and the evade meter.
#[derive(State, Debug, Default)]
pub struct Evade {
    pub evading: bool,
    pub last_evade_time: Real,
    /// EvadeCount: the meter, in steps of EvadeMeterCost (8.3): a float.
    pub evade_count: Real,
    pub evade_invuln_time: Real,
    pub evade_invuln: Real,
    pub last_evade_spent_meter: bool,
    pub dodged_successfully: bool,
    pub evade_regen_start: Real,
    pub evade_regen_once: bool,
    pub evade_crouch_rate: bool,
    pub dodge: Option<DodgeState>,
    pub no_ground_evade: bool,
    pub block_evade_montage: bool,
    /// The UI_Broker ShowArmorUnavailableForPlayer calls of BPI_ArmorDodgeCooldown (BaseFighter @108184, an
    /// AnimNotify_Armor refused on a full meter), counted: the HUD's player marker plays DodgeMeterEmpty
    /// (UI_IGv3_PlayerMarker OnArmorUnavailable @3602) when it goes up. Not in the record (nothing reads it): a
    /// load keeps it.
    pub armor_unavailable: i32,
}

/// `DodgeBy`: what a dodge reads of the thing it dodged.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DodgeBy {
    pub actor: crate::actors::ActorRef,
    pub fighter: Option<crate::fighter::FighterId>,
    pub team: i32,
    pub projectile: bool,
    pub is_projectile: bool,
    pub attack_type: i32,
}

/// `KEY_STRIDE`: a notify's key is its montage instance x KEY_STRIDE + its index.
pub const KEY_STRIDE: i32 = 4096;

// ---- the readers this module and `combat::defense` share

/// `f.data`: the fighter's character data (by its current key `ch`).
pub fn char_data(w: &World, f: FighterId) -> &CharData {
    char_data_in(&w.data, w, f)
}

/// [`char_data`] borrowed from `data` (the World's `Arc`, cloned by a caller that writes the World meanwhile).
pub fn char_data_in<'a>(data: &'a GameData, w: &World, f: FighterId) -> &'a CharData {
    &data.chars[w.names.str(w.fighter(f).ch)]
}

/// `f.data.components.pawn[comp]?.p`.
pub fn pawn_comp_in<'a>(data: &'a GameData, w: &World, f: FighterId, comp: &str) -> Option<&'a Props> {
    char_data_in(data, w, f).fighter.components.pawn.get(comp)?.p.as_obj()
}

/// `f.data.components.base[comp]?.p`.
pub fn base_comp_in<'a>(data: &'a GameData, w: &World, f: FighterId, comp: &str) -> Option<&'a Props> {
    char_data_in(data, w, f).fighter.components.base.get(comp)?.p.as_obj()
}

/// `f.data.components.pawn[comp]?.p[prop]` as a number.
pub fn pawn_num(w: &World, f: FighterId, comp: &str, prop: &str) -> Option<Real> {
    pawn_comp_in(&w.data, w, f, comp)?.get(prop)?.as_f64().map(crate::math::narrow)
}

/// `f.data.components.base[comp]?.p[prop]` as a number.
pub fn base_num(w: &World, f: FighterId, comp: &str, prop: &str) -> Option<Real> {
    base_comp_in(&w.data, w, f, comp)?.get(prop)?.as_f64().map(crate::math::narrow)
}

/// `f.data.pawn[prop]` as a number.
pub fn pawn_prop(w: &World, f: FighterId, prop: &str) -> Option<Real> {
    char_data(w, f).fighter.pawn.get(prop)?.as_f64().map(crate::math::narrow)
}

/// The name of a string of the data (a montage, an icon, a sound event the character data names), interned at bind.
pub fn data_name(w: &World, s: &str) -> Name {
    data_name_in(&w.names, s)
}

/// [`data_name`] in `names`.
pub fn data_name_in(names: &crate::names::Names, s: &str) -> Name {
    names.get(s).unwrap_or_else(|| panic!("the data's string {s:?} was not interned at bind"))
}

/// `f.events.push(ev)`: `make` gets the fighter's id.
pub fn emit(w: &mut World, f: FighterId, make: impl FnOnce(Fid) -> Event) {
    let me = w.fighter_mut(f);
    let ev = make(me.id);
    me.events.push(ev);
}

/// The montage `name` of the fighter's data.
pub fn montage_def<'a>(data: &'a GameData, w: &World, f: FighterId, name: Name) -> Option<&'a Montage> {
    char_data_in(data, w, f).fighter.montages.get(w.names.str(name))
}

/// Runs `then` on the player of `slot` (a montage player a caller holds); `None` when the slot holds no
/// montage or its montage is not in the fighter's data.
pub fn with_player<R>(
    w: &mut World,
    f: FighterId,
    slot: MontageSlot,
    then: impl FnOnce(&mut MontagePlayer<'_, MontageKey>) -> R,
) -> Option<R> {
    let data = w.data.clone();
    let key = slot_state(w.fighter(f), slot)?.montage;
    let m = montage_def(&data, w, f, key.name)?;
    let s = slot_state_mut(w.fighter_mut(f), slot)?;
    Some(then(&mut MontagePlayer { m, s }))
}

/// The state a slot names.
pub fn slot_state(me: &Fighter, slot: MontageSlot) -> Option<&MontageState> {
    let ms = &me.montages;
    match slot {
        MontageSlot::Main => ms.montage.as_ref(),
        MontageSlot::Dash => ms.dash_montage.as_ref(),
        MontageSlot::Fading(i) => ms.fading.get(usize::try_from(i).ok()?).map(|e| &e.m),
        MontageSlot::Grouped(i) => ms.grouped.as_ref()?.0.get(usize::try_from(i).ok()?).map(|e| &e.m),
        MontageSlot::Foreign => me.shapes.disguise.as_ref()?.copy.as_ref().map(|c| &c.m),
        MontageSlot::Poly => me.shapes.polymorph.as_ref()?.play.as_ref().map(|c| &c.m),
    }
}

/// [`slot_state`], to write.
pub fn slot_state_mut(me: &mut Fighter, slot: MontageSlot) -> Option<&mut MontageState> {
    let ms = &mut me.montages;
    match slot {
        MontageSlot::Main => ms.montage.as_mut(),
        MontageSlot::Dash => ms.dash_montage.as_mut(),
        MontageSlot::Fading(i) => ms.fading.get_mut(usize::try_from(i).ok()?).map(|e| &mut e.m),
        MontageSlot::Grouped(i) => ms.grouped.as_mut()?.0.get_mut(usize::try_from(i).ok()?).map(|e| &mut e.m),
        MontageSlot::Foreign => me.shapes.disguise.as_mut()?.copy.as_mut().map(|c| &mut c.m),
        MontageSlot::Poly => me.shapes.polymorph.as_mut()?.play.as_mut().map(|c| &mut c.m),
    }
}

/// `f.airSpecialMax`: the air specials a fighter has before it lands (a buff's override, else AIR_SPECIAL_MAX).
pub fn air_special_max(w: &World, f: FighterId) -> Real {
    w.fighter(f).air.air_special_max_set.unwrap_or(AIR_SPECIAL_MAX)
}

// ---- the call surface of actions: the button presses of a frame, attack execution and the attack spam
// delay

/// BaseFighter StickAttackXDeadzone.
const STICK_ATTACK_X_DEADZONE: Real = 0.25;

/// `pressDir`: the attack direction of this press. The bot passes it with
/// NormalAttackPressed / SpecialAttackPressed, players get it from the stick: PFGPlayerController's press handlers pass
/// BaseFighter.GetDirection of the pair (GamePadInputEnabled ? RawAnalogXMove / YMove : AxisMoveX2 / Y2;
/// @17131-17363, @38559-38732, @79344-79517), so one of Up / Down / Forward / Back / Neutral, the larger axis winning
/// and a tie going to the vertical (`get_direction`). The pair is the press handler's (`press_stick_begin`).
/// A player's directional action passes the absolute byte, which the attack lookup reads as Forward / Back against the
/// facing. A direction passed with the press (a bot's literal, `atk_dir`) goes on as it is, a diagonal too: the attack
/// lookup compares the byte exactly (BranchManagerGetAttack @2791-2878) and nothing on this path folds 6-9 to Up /
/// Down (the one fold, Comp_Pawn_BranchManager CovertToAttackDirection @0-393, folds the stick in the InputBufferHold
/// test, not the press's byte), so a diagonal finds only a row of that diagonal or of any direction.
pub fn press_dir(w: &World, f: FighterId) -> Dir {
    let me = w.fighter(f);
    let facing = me.body.facing_dir;
    let inp = &me.controls.inp;
    if let Some(fd) = inp.force_dir {
        return match fd {
            2 if facing > 0.0 => Dir::Fwd,
            2 => Dir::Back,
            3 if facing < 0.0 => Dir::Fwd,
            3 => Dir::Back,
            _ => dir_of(i32::from(fd)).expect("a directional action's byte"),
        };
    }
    match inp.atk_dir {
        Some(a) if a != Dir::Default => a,
        _ => get_direction(inp.x, inp.y, facing),
    }
}

/// `rawPressDir`: UnmodifiedDirInputParam (what Comp_AbilityOverrideManager.IsOverrideValid
/// compares): the same byte, as the controller passes GetDirection's and nothing folds it afterwards.
pub fn raw_press_dir(w: &World, f: FighterId) -> Dir {
    press_dir(w, f)
}

/// `swappedDir`: Comp_Pawn_BranchManager GetSwappedAttackDirs(IsInAir, Button, Direction)
/// @0-1201: the direction an attack lookup takes (BFGetAttack @34 with GetIsInAir, SetActiveAttackFromBranch @994 with
/// BPI_GetIsInAir). Only for button 0 (the basic attack), with SwappedAttackStyleGround on the ground or
/// SwappedAttackStyleAir in the air, and not IsBotControlled (@499-978): SwapForwardAndNeutralDirection @145-411 makes
/// Forward and Back Neutral and Neutral Forward, or Back facing left (BPI_IsFacingLeft); every other direction stays.
/// The unmodified one stays the override check's and the turn's. `b`: `None` for 'taunt' / 'none'.
pub fn swapped_dir(w: &World, f: FighterId, b: Option<Btn>, d: Dir, air: bool) -> Dir {
    let me = w.fighter(f);
    let c = &me.controls;
    let swapped = if air { c.prefs.swapped_air } else { c.prefs.swapped_ground };
    if b != Some(Btn::Attack) || c.bot_controlled || !swapped {
        return d;
    }
    match d {
        Dir::Fwd | Dir::Back => Dir::Neutral,
        Dir::Neutral if me.body.facing_dir < 0.0 => Dir::Back,
        Dir::Neutral => Dir::Fwd,
        _ => d,
    }
}

/// `pressStickBegin`: the stick a press handler reads. UPlayerInput::ProcessInputStack
/// 142e91450 runs every press and release delegate of a frame before any axis delegate, so on a gamepad a press
/// handler finds RawAnalogXMove / YMove, and the pawn's MoveXAxisValue / MoveYAxisValue, as the axis events of the
/// frame BEFORE left them; this frame's stick arrives after the handlers and before the pawn ticks. The port takes a
/// tick's presses and its stick as one InputState, so while the press handlers of a tick run (`actions`,
/// `buffer_during_hitpause`, `free_pause_actions`, `pawn_presses`) the fighter's input has the pair of the tick
/// before, `prev`: sim state, so a rollback gives the same press.
/// Only with `axis` set (GamePadInputEnabled after this frame's axis events). Keys, the d-pad and a bot write the
/// pawn's pair in their own events, which the port has as this tick's pair: their order against a press of the same
/// tick is not carried, the press reads the tick's last value as before. With the flag set in this very tick (the
/// stick left its dead zone) the handler read AxisMoveX2 / Y2, the keys' ints, which for a player not using both
/// devices is the pair of the tick before as well.
/// Returns the real pair for `press_stick_end`, or `None` when nothing was swapped.
fn press_stick_begin(w: &mut World, f: FighterId) -> Option<(Real, Real)> {
    let c = &mut w.fighter_mut(f).controls;
    let (i, p) = (&c.inp, &c.prev);
    if !i.axis || (i.x == p.x && i.y == p.y) {
        return None;
    }
    let real = (i.x, i.y);
    (c.inp.x, c.inp.y) = (c.prev.x, c.prev.y);
    Some(real)
}

/// `pressStickEnd`.
fn press_stick_end(w: &mut World, f: FighterId, real: Option<(Real, Real)>) {
    if let Some((x, y)) = real {
        let inp = &mut w.fighter_mut(f).controls.inp;
        (inp.x, inp.y) = (x, y);
    }
}

/// `forcedPress`.
pub fn forced_press(w: &World, f: FighterId) -> Option<ForcedPress> {
    let inp = &w.fighter(f).controls.inp;
    if !inp.force {
        return None;
    }
    inp.force_dir.map(|d| ForcedPress { dir: i32::from(d) })
}

/// `saveBranchInput`: Comp_Pawn_BranchManager InputTriggered_Event @3032-3345, at every
/// ButtonPress of a locally controlled pawn (BaseFighter ubergraph 131186 broadcasts InputTriggered before the press's
/// own handling; the air special over its limit still gets here, @4975 -> @3032): InputAxisX / InputAxisY =
/// BPI_GetMoveAxisValues, the pawn's MoveXAxisValue / MoveYAxisValue as the press handler finds them
/// (`press_stick_begin`: on a gamepad the pair of the frame before), and RightStickInputX = BPI_GetStickAttackX.
/// Before the windows are written, so whether or not a window is on. The port calls it where its press loops hand a
/// press to the manager (after the gates the build tests before ButtonPress); on the frames those loops do not run,
/// `save_presses_only`.
/// The same broadcast reaches the buffs bound to it (BuffSignal FighterInputTriggered): ButtonPress's InputType and
/// Direction (@131186), the presser's own buffs first (Buff_AttachToAlly binds its owner's), then the other fighters'
/// (Buff_C015_TeamFightCloud binds its ally's). The branch manager's binding (BeginPlay) is the delegate's first, so
/// the pair is written before the buffs hear the press.
/// ButtonPress also sets ButtonStates[InputType] (@131126; BaseFighter IsButtonPressed reads it), kept only under the
/// InputBufferHold preference for its test (holdOk); `button_releases` clears it.
/// Deviation: the build's other IsButtonPressed readers (the ButtonHold / ButtonPressed SectionJump notifies, glide,
/// charge, the bots) read the input level here (chargeHeld, the conditions), a second model of the same array that
/// can disagree after a forced press or a directional action's release.
pub fn save_branch_input(w: &mut World, f: FighterId, input: i32, dir: i32) {
    let me = w.fighter_mut(f);
    if me.controls.prefs.input_buffer_hold {
        me.branching.button_states = Some(me.branching.button_states.unwrap_or(0) | (1 << input));
    }
    let inp = &me.controls.inp;
    let (x, y, rsx) = (inp.x, inp.y, inp.rsx.unwrap_or(0.0));
    let b = &mut me.branching;
    b.br_axis_x = x;
    b.br_axis_y = y;
    b.br_stick_x = rsx;
    input_triggered(w, f, input, dir);
}

/// ButtonPress's InputType per press (Enum_InputButton).
fn button_of(b: Btn) -> i32 {
    match b {
        Btn::Attack => 0,
        Btn::Special => 1,
        Btn::Jump => 2,
        Btn::Dodge => 3,
        Btn::Item => 5,
    }
}

/// `pressInput`: ButtonPress's InputType and its Direction: the jump's ButtonPress(2, 5)
/// (@116509), the others the direction the controller passed (`press_dir`).
fn press_input(w: &mut World, f: FighterId, b: Btn) {
    let d = if b == Btn::Jump { Dir::Default } else { press_dir(w, f) };
    save_branch_input(w, f, button_of(b), dir_num(d));
}

/// `prePress`: BaseFighter InputPreTriggered(InputType, Direction): the first statement of
/// each press event, ahead of every test (NormalAttackPressed @126257: 0; Special Attack Pressed Logic @136859: 1;
/// EvadeButtonPressed @123338 / @188521: 3; ItemButtonPressed @168937: 5), the press's Direction (`press_dir`). The
/// pawn's binding takes it here (`CharHooks::input_pre_triggered`: Comp_JakeEat's JakeSpitEvent); the jump's (2, 5) is
/// `jump_pressed_dispatch`'s (`CharHooks::jump_pressed`), the taunt's (4, 5) tauntPress's.
/// `pressActions` sends each press's at its turn, before the press's own handling; controllerTick's `pre_trigger_end`
/// sends those of a frame whose press no loop got to (hitstun, dead, a pause; ahead of the handling there).
/// Deviation: on those frames all of the frame's pre-triggers come ahead of the presses' handling (in the build each
/// press is its own event, so one press's handling would come before the next one's pre-trigger); the Direction there
/// is the live stick's (`press_stick_begin` not applied). The buffs' bindings (BuffSignal InputPreTriggered:
/// polymorph's taunt, Buff_VelmaAttachToAlly's ActivatedLaunch) get each press's ahead of the pawn's (as tauntPress),
/// the jump's in `jump_pressed_dispatch`.
/// `Controls::pre_sent` / `pre_mask`: the frame's marker between the calls (as `jump_sent`), not sim state.
fn pre_press(w: &mut World, f: FighterId, b: Btn) {
    let h = w.fighter(f).hooks.char.input_pre_triggered;
    if (h.is_none() && w.fighter(f).buffs.buffs.is_empty()) || b == Btn::Jump {
        return;
    }
    if !w.fighter(f).controls.pre_sent {
        clear_pre_sent(w);
        let c = &mut w.fighter_mut(f).controls;
        c.pre_sent = true;
        c.pre_mask = 0;
    }
    let bit = 1 << button_of(b);
    let c = &mut w.fighter_mut(f).controls;
    if c.pre_mask & bit != 0 {
        return;
    }
    c.pre_mask |= bit;
    let dir = dir_num(press_dir(w, f));
    if !w.fighter(f).buffs.buffs.is_empty() {
        let arg = BuffSignalArg { input: Some(button_of(b)), dir: Some(crate::real(dir)), ..BuffSignalArg::default() };
        buff_signal(w, f, BuffSignal::InputPreTriggered, arg);
    }
    if let Some(h) = h {
        h(w, f, button_of(b), dir);
    }
}

/// Clears the module-wide `preSent`: no fighter is the marked one.
fn clear_pre_sent(w: &mut World) {
    for i in 0..w.fighters.len() {
        let id = w.fighters.nth(i).expect("a live fighter");
        let c = &mut w.fighter_mut(id).controls;
        c.pre_sent = false;
        c.pre_mask = 0;
    }
}

/// The presses in `PRESS_ORDER`.
const PRESS_ORDER: [Btn; 5] = [Btn::Attack, Btn::Special, Btn::Jump, Btn::Dodge, Btn::Item];

/// `preTriggerEnd`.
pub fn pre_trigger_end(w: &mut World, f: FighterId) {
    // (controllerTick's one call every frame)
    button_releases(w, f);
    let me = w.fighter(f);
    if me.hooks.char.input_pre_triggered.is_some() || !me.buffs.buffs.is_empty() {
        for b in PRESS_ORDER {
            if pressed(w, f, b) {
                pre_press(w, f, b);
            }
        }
    }
    clear_pre_sent(w);
}

/// `buttonReleases`: ButtonRelease(InputType) clears ButtonStates[InputType]: each release
/// event calls it (NormalAttackReleased @126303, SpecialAttackReleased @131661, the jump's @120934 / @127724, the
/// evade's @123497 / @162709, the taunt's @144204 / @146556, ItemButtonReleased @169712), a directional attack
/// action's own release (`force_release`) too while the button is still held. Kept only under InputBufferHold
/// (`save_branch_input`).
/// Unverified: the release events are taken as ungated (the port reads each key's up from the frame's input).
fn button_releases(w: &mut World, f: FighterId) {
    let me = w.fighter_mut(f);
    let Some(mut s) = me.branching.button_states.filter(|&s| s != 0) else { return };
    let (i, p) = (&me.controls.inp, &me.controls.prev);
    if (p.attack && !i.attack) || i.force_release == Some(AttackBtn::Attack) {
        s &= !1;
    }
    if (p.special && !i.special) || i.force_release == Some(AttackBtn::Special) {
        s &= !2;
    }
    if p.jump && !i.jump {
        s &= !4;
    }
    if p.dodge && !i.dodge {
        s &= !8;
    }
    if p.taunt_held && !i.taunt_held {
        s &= !16;
    }
    if p.item && !i.item {
        s &= !32;
    }
    me.branching.button_states = Some(s);
}

/// `inputTriggered`.
fn input_triggered(w: &mut World, f: FighterId, input: i32, dir: i32) {
    let arg =
        BuffSignalArg { presser: Some(f), input: Some(input), dir: Some(crate::real(dir)), ..BuffSignalArg::default() };
    if !w.fighter(f).buffs.buffs.is_empty() {
        buff_signal(w, f, BuffSignal::FighterInputTriggered, arg.clone());
    }
    let mut i = 0;
    while let Some(&o) = w.pawns.as_deref().unwrap_or(&w.players).get(i) {
        i += 1;
        if o != f && !w.fighter(o).buffs.buffs.is_empty() {
            buff_signal(w, o, BuffSignal::FighterInputTriggered, arg.clone());
        }
    }
    // The actors bound to it (FighterSignal InputTriggered).
    // Unverified: their place among the delegate's bindings (behind the buffs') is not read.
    signal_bound(w, f, FighterSignal::InputTriggered, false, crate::real(input), Name::NONE, None);
}

/// `savePressesOnly`: a frame whose press loops do not run (knockback hitstun) or a pause
/// the hitpause buffer leaves (no montage, a launch waiting): InputTriggered_Event has no hitpause test, so a press
/// that reaches ButtonPress saves the pair all the same. The evade press always does (EvadeButtonPressed @123236 ->
/// ButtonPress @123368 tests nothing, and PFGPlayerController calls it with no test of the pawn, @1505); the attack,
/// special and item presses only out of hitstun and buff stun, not wall sliding and not dead (ubergraph 117196 /
/// 117474 / 117925), the jump only out of hitstun and not dead (116236-116499). A taunt press of such a frame saves
/// nothing here (Deviation: TauntPressedLogic's gates, 144994-145183, are not taken over). A dead frame saves the
/// evade press's pair too: the controller still reaches the pawn (PFGPlayerController @9670-10008, Get Keyboard
/// Fighter's K2_GetPawn fallback @677 with no dead test); the other presses test isDead (the `free` test below).
/// Each press that reaches ButtonPress broadcasts InputTriggered (`save_branch_input`), in the order attack, special,
/// jump, evade, item.
/// Deviation: the frame's own press order (InputState.order) is not taken here; the pair each writes is the same.
pub fn save_presses_only(w: &mut World, f: FighterId) {
    let me = w.fighter(f);
    let free = me.hit.launch.is_none() && me.hit.hitstun <= 0.0 && !me.buffs.buff_stunned && me.life.dead <= 0.0;
    let atk = free && !me.wall.wall_sliding && attack_enabled(w, f);
    // (the attack and special presses' GetInputTypeDirectionEnabled reads the press's direction: in the bracket)
    let reach = |w: &World, b: Btn| {
        pressed(w, f, b)
            && match b {
                Btn::Dodge => true,
                Btn::Jump => free && jump_enabled(w, f),
                Btn::Attack => atk && press_type_enabled(w, f, 0),
                Btn::Special => atk && press_type_enabled(w, f, 1),
                Btn::Item => atk,
            }
    };
    let real = press_stick_begin(w, f);
    if ![Btn::Dodge, Btn::Jump, Btn::Attack, Btn::Special, Btn::Item].iter().any(|&b| reach(w, b)) {
        press_stick_end(w, f, real);
        return;
    }
    for b in PRESS_ORDER {
        if reach(w, b) {
            press_input(w, f, b);
        }
    }
    press_stick_end(w, f, real);
}

/// `preAttackRotation`: DoPreAttackRotation(AttackDirection, ForceAttackDirection,
/// RightStickX) on MoveXAxisValue `x`: AttackDirection 3 / 2 gives LocalAttackFacing -1 / +1 (@0-45, @785-830), Up /
/// Down take it from RightStickX past StickAttackXDeadzone (@973-1261) unless the fighter's IgnoreRightStickDiagonals
/// is on (@904-968: then 0), Neutral and Default leave 0. A forced press runs HandleLockedRotationWindow and turns only
/// to LocalAttackFacing; otherwise a non-zero MoveX turns to the stick first (@637-757: the sign alone, at any size),
/// else LocalAttackFacing (@1289-1371: SetRotation(LAF < 0)).
/// `dir`: the press's direction byte. The trigger of a window that held no press passes Default (5;
/// SetActiveAttackFromBranch @2582, Comp_Pawn_BranchManager ubergraph @9342): MoveX alone turns him, the right stick
/// does not. The byte is read as an absolute side, forced or not: a forced press's 2 / 3 is Right / Left, and so is a
/// non-forced Forward / Back (EActionDirection 2 / 3) when MoveX is 0 (a bot's NormalAttackPressed(2) with the stick
/// centred), whichever side the fighter faces. A player's non-forced Forward / Back has MoveX != 0, so the MoveX turn
/// decides first.
/// The free press passes the live pair and StickAttackX (BaseFighter @90730, @90948, @145735); a branch, the automatic
/// one and the delayed spammed attack the manager's saved pair and RightStickInputX (`execute_branch`).
pub fn pre_attack_rotation(w: &mut World, f: FighterId, force: Option<ForcedPress>, dir: Dir, x: Real, rsx: Real) {
    let ignore = w.fighter(f).controls.prefs.ignore_right_stick_diagonals;
    // LocalAttackFacing (@0-1284)
    let local_facing = |d: i32| -> Real {
        match d {
            3 => -1.0,
            2 => 1.0,
            _ if d <= 1 && !ignore => {
                if rsx < -STICK_ATTACK_X_DEADZONE {
                    -1.0
                } else if rsx > STICK_ATTACK_X_DEADZONE {
                    1.0
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    };
    if let Some(force) = force {
        let me = w.fighter_mut(f);
        me.controls.rot_lock_until = crate::real(me.clock.tick) + latent_frames(0.25);
        let laf = local_facing(force.dir);
        if laf != 0.0 {
            set_facing(w, f, laf);
        }
        return;
    }
    if x != 0.0 {
        set_facing(w, f, if x < 0.0 { -1.0 } else { 1.0 });
    } else {
        let laf = local_facing(dir_num(dir));
        if laf != 0.0 {
            set_facing(w, f, laf);
        }
    }
}

/// `pressEnabled`: GetJumpEnabled / GetAttackEnabled before a press's ButtonPress
/// (JumpPressedLocal @116333, NormalAttackPressed @117605, SpecialAttackPressed @118123, ItemButtonPressed @169131);
/// the evade press's ButtonPress has no such test (GetEvadeEnabled is EvadeFromBranch's).
/// NormalAttackPressed and SpecialAttackPressed AND GetInputTypeDirectionEnabled(0 / 1, Direction) in the same test
/// (@117548, @117999): the press's direction (`press_dir`, inside the press handler's stick bracket).
pub fn press_enabled(w: &World, f: FighterId, b: Btn) -> bool {
    match b {
        Btn::Dodge => true,
        Btn::Jump => jump_enabled(w, f),
        Btn::Attack => attack_enabled(w, f) && press_type_enabled(w, f, 0),
        Btn::Special => attack_enabled(w, f) && press_type_enabled(w, f, 1),
        Btn::Item => attack_enabled(w, f),
    }
}

/// GetInputTypeDirectionEnabled(type, the press's Direction).
fn press_type_enabled(w: &World, f: FighterId, t: u8) -> bool {
    !input_type_disabled_any(w, f, t) || input_type_direction_enabled(w, f, t, dir_num(press_dir(w, f)))
}

/// EvadeFromBranch's GetInputTypeDirectionEnabled(3, Direction == 4 ? 4 : Get8WayDirection) (@139063-139136), on
/// the pair (`x`, `y`) and the facing ahead of its own MoveX turn.
fn evade_type_enabled(w: &World, f: FighterId, neutral: bool, x: Real, y: Real) -> bool {
    if !input_type_disabled_any(w, f, 3) {
        return true;
    }
    let d = if neutral { Dir::Neutral } else { crate::input::get8_way(x, y, w.fighter(f).body.facing_dir) };
    input_type_direction_enabled(w, f, 3, dir_num(d))
}

/// An InputState's button `b`.
fn held(i: &InputState, b: Btn) -> bool {
    match b {
        Btn::Attack => i.attack,
        Btn::Special => i.special,
        Btn::Jump => i.jump,
        Btn::Dodge => i.dodge,
        Btn::Item => i.item,
    }
}

/// `pressed`.
pub fn pressed(w: &World, f: FighterId, k: Btn) -> bool {
    let c = &w.fighter(f).controls;
    let forced = match c.inp.force_press {
        Some(AttackBtn::Attack) => k == Btn::Attack,
        Some(AttackBtn::Special) => k == Btn::Special,
        None => false,
    };
    (held(&c.inp, k) && !held(&c.prev, k)) || forced || (k == Btn::Dodge && c.inp.neutral_evade)
}

/// `jumpPressedDispatch`: BaseFighter JumpPressedLocal's head (@122712-122732), once per
/// jump press and ahead of everything else the event does (the hitstun, JumpEnabled, InputEnabled and dead tests are
/// behind it, @116236): broadcast JumpPressedDispatch(self) to the actors bound to it (`jump_listeners`,
/// `ActorEntry::jump_pressed`), then InputPreTriggered(Jump), which the pawn's own graph binds
/// (`CharHooks::jump_pressed`: Pawn_C017's GroundHoverJump). The press loops call it at the jump press's turn, before
/// that press's handling, so the jump logic reads the state the listeners left; controllerTick's `jump_dispatch_end`
/// sends it on a frame where no loop got there. BPI_JumpPressedLocal (@99672, a bot's press: `jump_local`) is the same
/// event, so it broadcasts too.
/// `Controls::jump_sent` is the frame's marker between the two calls, not sim state: no fighter holds it outside
/// controllerTick.
pub fn jump_pressed_dispatch(w: &mut World, f: FighterId) {
    if w.fighter(f).controls.jump_sent {
        return;
    }
    clear_jump_sent(w);
    w.fighter_mut(f).controls.jump_sent = true;
    let mut dead = false;
    let mut i = 0;
    while let Some(&l) = w.fighter(f).frame.jump_listeners.get(i) {
        i += 1;
        let ActorRef::Actor(a) = l.actor else { continue };
        let Some(act) = w.actors.actors.get(a) else {
            dead = true;
            continue;
        };
        if act.dead {
            dead = true;
            continue;
        }
        let h = match w.registry.actors.get(w.names.str(act.cls)).map(|k| &k.hooks) {
            Some(ActorHooks::World(h)) => h.jump_pressed,
            _ => None,
        };
        if let Some(h) = h {
            h(w, a, f);
        }
    }
    if dead {
        let mut ls = std::mem::take(&mut w.fighter_mut(f).frame.jump_listeners);
        ls.retain(|l| match l.actor {
            ActorRef::Actor(a) => w.actors.actors.get(a).is_some_and(|act| !act.dead),
            _ => true,
        });
        w.fighter_mut(f).frame.jump_listeners = ls;
    }
    if !w.fighter(f).buffs.buffs.is_empty() {
        // the buffs' binding, then the pawn's
        let arg = BuffSignalArg { input: Some(2), dir: Some(5.0), ..BuffSignalArg::default() };
        buff_signal(w, f, BuffSignal::InputPreTriggered, arg);
    }
    let h = w.fighter(f).hooks.char.jump_pressed;
    if let Some(h) = h {
        h(w, f);
    }
}

/// Clears the module-wide `jumpSent`.
fn clear_jump_sent(w: &mut World) {
    for i in 0..w.fighters.len() {
        let id = w.fighters.nth(i).expect("a live fighter");
        w.fighter_mut(id).controls.jump_sent = false;
    }
}

/// `jumpDispatchEnd`.
pub fn jump_dispatch_end(w: &mut World, f: FighterId) {
    if pressed(w, f, Btn::Jump) {
        jump_pressed_dispatch(w, f);
    }
    clear_jump_sent(w);
}

// ---- actions

/// A notify's Conditions array as `condsOk` takes it (`None`: the notify has none).
fn notify_conds(w: &World, n: &Notify) -> Option<Vec<Option<Name>>> {
    notify_names(w, n, "Conditions")
}

/// The notify `n`'s flag `key` (`n.p[key]`, false when the notify or the flag is not in the data).
fn notify_flag(data: &GameData, w: &World, f: FighterId, n: NotifyRef, key: &str) -> bool {
    notify_def(data, w, f, n).and_then(|d| d.p.get(key)).and_then(Value::as_bool).unwrap_or(false)
}

/// A notify's array of names `key` as `condsOk` takes it (`None`: the notify has none).
fn notify_names(w: &World, n: &Notify, key: &str) -> Option<Vec<Option<Name>>> {
    let Some(Value::Arr(a)) = n.p.get(key) else { return None };
    Some(
        a.iter()
            .map(|c| match c {
                Value::Str(s) => Some(data_name(w, s)),
                _ => None,
            })
            .collect(),
    )
}

/// `f.condsOk(n.p.Conditions)` of the notify `n`.
fn notify_conds_ok(w: &mut World, f: FighterId, n: NotifyRef) -> bool {
    let data = w.data.clone();
    let conds = notify_def(&data, w, f, n).and_then(|d| notify_conds(w, d));
    conds_ok(w, f, conds.as_deref(), false)
}

/// The class of the notify `n` (`n.cls`), `""` for a notify the data lacks.
fn notify_cls<'a>(data: &'a GameData, w: &World, f: FighterId, n: NotifyRef) -> &'a str {
    notify_def(data, w, f, n).map_or("", |d| d.cls.as_str())
}

/// `attackingFlags`: the active AnimNotify_isAttackingFlag states, and the
/// AnimNotify_ConditionalIsAttacking ones (its child, Received_NotifyBegin / End @121-190 call the parent's only when
/// the owner's BPI_CheckConditions(Conditions) passes) whose conditions pass now.
/// Deviation: the conditions are read at each use, not latched at the begin (the one placement,
/// C018_Air_UpBlaster's Condition_C018HasNoAimPerk, holds for the whole match).
pub fn attacking_flags(w: &mut World, f: FighterId) -> Vec<NotifyRef> {
    let flag = crate::name!("AnimNotify_isAttackingFlag_C");
    let cond = crate::name!("AnimNotify_ConditionalIsAttacking_C");
    let mut out: Vec<NotifyRef> = active_of(w, f, flag, None).into_iter().map(|a| a.n).collect();
    if has_active(w, f, cond, -1.0) {
        for a in active_of(w, f, cond, None) {
            if notify_conds_ok(w, f, a.n) {
                out.push(a.n);
            }
        }
    }
    // A state of a montage stopped after the frame's dispatch keeps its Allow values until its late NotifyEnd: the
    // ...ThruATKFlag counters it raised are lowered there (AnimNotify_isAttackingFlag Received_NotifyEnd), with the
    // AttackIsActive count IsAttacking reads.
    let data = w.data.clone();
    let ends: Vec<NotifyRef> = w.fighter(f).branching.pending_ends.iter().map(|e| e.0).collect();
    for n in ends {
        let cls = notify_cls(&data, w, f, n);
        if cls == "AnimNotify_isAttackingFlag_C"
            || (cls == "AnimNotify_ConditionalIsAttacking_C" && notify_conds_ok(w, f, n))
        {
            out.push(n);
        }
    }
    out
}

/// `condAttacking`: whether a ConditionalIsAttacking state with NotifyBranchManager is
/// active and its conditions pass.
pub fn cond_attacking(w: &mut World, f: FighterId) -> bool {
    let cond = crate::name!("AnimNotify_ConditionalIsAttacking_C");
    if !has_active(w, f, cond, -1.0) {
        return false;
    }
    let data = w.data.clone();
    for a in active_of(w, f, cond, None) {
        let manager = notify_def(&data, w, f, a.n).and_then(|d| d.p.get("NotifyBranchManager"));
        if !matches!(manager, Some(Value::Bool(false))) && notify_conds_ok(w, f, a.n) {
            return true;
        }
    }
    false
}

/// `latePress`: a press in the frame of a montage stopped after the frame's dispatch, whose
/// windows are kept in `late_branch` until their late NotifyEnd. While that end owes the AttackIsActive count's
/// NotifyEnd (AttackIsActive above 0 until that dispatch) the free action does nothing, so the late end resolves the
/// press: Shaggy's mashed Knee lands and the press starts Shaggy_NeutralAttack1 through it. Not attacking, the press
/// also runs the free action: a free attack or special calls ClearQueuedAttack first (ubergraph 115600 / 118431) and a
/// second StartJump of the frame repeats the ground jump, so only an evade is left: the late end starts it again
/// through BPI_EvadeFromBranch (no gate on the ground). The press is written into every window `late_branch` kept:
/// those the earlier press was in and those kept for a row of no button. A buffered CanBranchToEvade window with no
/// stored press and no such row is not kept over a late stop (`stop_montage`), so an evade press does not reach it.
/// `b` `None`: the taunt, whose slot `taunt` names when the slot has a branch.
/// Deviation: several presses in that frame drop the buffer (`many`).
pub fn late_press(w: &mut World, f: FighterId, b: Option<Btn>, many: bool, taunt: Option<Box<Branch>>) {
    let Some(lb) = &w.fighter(f).branching.late_branch else { return };
    let data = w.data.clone();
    let evade_window =
        b == Some(Btn::Dodge) && lb.ws.iter().any(|&(n, _)| notify_flag(&data, w, f, n, "CanBranchToEvade"));
    let keep = !many && (is_attacking(w, f) || evade_window);
    if !keep {
        w.fighter_mut(f).branching.late_branch = None;
        return;
    }
    let force = match b {
        Some(Btn::Dodge) => evade_force(w, f),
        Some(Btn::Jump) | None => None,
        Some(_) => forced_press(w, f),
    };
    let press = b.map_or(QueuedBtn::Taunt, QueuedBtn::Press);
    let p = QueuedPress { press, dir: press_dir(w, f), force, taunt, raw: raw_press_dir(w, f) };
    let lb = w.fighter_mut(f).branching.late_branch.take().expect("the late branch tested above");
    let mut windows: Vec<i32> = lb.q.as_ref().map_or_else(Vec::new, |q| q.windows.clone());
    for &(_, i) in &lb.ws {
        if !windows.contains(&i) {
            windows.push(i);
        }
    }
    let q = write_press(w, f, lb.q, p, &windows);
    w.fighter_mut(f).branching.late_branch = Some(LateBranch { ws: lb.ws, cur: lb.cur, q });
}

/// `bufferDuringHitpause`: presses during our own hitpause (after landing a hit): the branch
/// manager's input event has no hitpause test (InputTriggered_Event @5132-5362; Trigger Current Attack Client rejects
/// only dead or hitstun, @5546-5754), so a press is buffered and an Immediate window acts on it under the pause. Free
/// actions are not run here: the free evade needs GetHitPauseTime <= 0 (ubergraph 114515-114716) and the fighter is
/// attacking.
pub fn buffer_during_hitpause(w: &mut World, f: FighterId) {
    let me = w.fighter(f);
    if me.montages.montage.is_none() || me.hit.launch.is_some() {
        save_presses_only(w, f);
        return;
    }
    let real = press_stick_begin(w, f);
    for b in PRESS_ORDER {
        if !pressed(w, f, b) {
            continue;
        }
        if b == Btn::Jump {
            jump_pressed_dispatch(w, f);
        }
        if matches!(b, Btn::Item | Btn::Attack | Btn::Special) && w.fighter(f).wall.wall_sliding {
            continue;
        }
        if !press_enabled(w, f, b) {
            continue;
        }
        press_input(w, f, b);
        if w.fighter(f).montages.montage.is_none() {
            // an earlier press of the frame cancelled the attack
            break;
        }
        if b == Btn::Special {
            air_special_cue(w, f);
        }
        window_press(w, f, b, true);
    }
    if let Some(slot) = w.fighter(f).controls.inp.taunt {
        taunt_press(w, f, i32::from(slot), true);
    }
    press_stick_end(w, f, real);
}

/// `freePauseActions`: presses under the pause of a hit that left no HitstunTimer (a bounce
/// out of hitstun, a hit with HitStun 0; `freeLaunch`): CheckHitstunActive is false, so the input events run as for any
/// fighter. The attack, special and item presses have no hitpause test (BaseFighter ubergraph 117605-117886,
/// 117925-118337: hitstun, wall slide, attack / input enabled, dead), nor has JumpPressedLocal (115409 -> 76454); only
/// the free evade wants GetHitPauseTime <= 0 (114515-114716). What starts sits at its first frame until the pause is
/// over (meshTickInner), and DMHitPauseEnd's ApplyKnockback then launches the fighter whatever it started.
/// The rest of `actions` runs here as well: the stop-anim notifies tick with the mesh under the dilation (no pause or
/// hitstun test in their NotifyTick; `stop_anim_ticks`, at the end of controllerTick as after `actions`),
/// Comp_Pawn_Rage's InputReleased_Event has none either (controllerTick), and TauntPressedLogic (144630-145221) tests
/// CheckHitstunActive, BaseFighter's own PendingLaunch and IsMovingOnGround, not the pause or the DamageManager's
/// launch.
/// Unverified: not seen in a capture.
pub fn free_pause_actions(w: &mut World, f: FighterId) {
    let real = press_stick_begin(w, f);
    free_pause_presses(w, f);
    press_stick_end(w, f, real);
}

/// `freePausePresses`.
fn free_pause_presses(w: &mut World, f: FighterId) {
    let presses: Vec<Btn> = PRESS_ORDER.into_iter().filter(|&b| pressed(w, f, b)).collect();
    if presses.iter().any(|&b| b != Btn::Jump && b != Btn::Dodge) {
        emit(w, f, |fighter| Event::CombatInput { fighter });
    }
    for b in presses {
        if b == Btn::Jump {
            jump_pressed_dispatch(w, f);
        }
        if matches!(b, Btn::Item | Btn::Attack | Btn::Special) && w.fighter(f).wall.wall_sliding {
            continue;
        }
        if !press_enabled(w, f, b) {
            continue;
        }
        if b == Btn::Special {
            air_special_cue(w, f);
        }
        press_input(w, f, b);
        if has_attack_inst(w, f) && window_press(w, f, b, true) {
            return;
        }
        // after ButtonPress, with or without an attack instance (`press_actions`)
        if is_attacking(w, f) || b == Btn::Dodge {
            continue;
        }
        if matches!(b, Btn::Attack | Btn::Special) {
            clear_queued_attack(w, f);
        }
        free_action(w, f, b);
    }
    if let Some(slot) = w.fighter(f).controls.inp.taunt {
        taunt_press(w, f, i32::from(slot), false);
    }
}

/// `COPY_INST` / `POLY_INST`: the instance numbers of a Disguise's copy and a Polymorph's mesh.
const COPY_INST: Real = 2048.0;
const POLY_INST: Real = 2049.0;

/// A second mesh's montage and its default blend-out: the copy's (`COPY_INST`, the montages of the Disguise's victim)
/// or the Polymorph's (`POLY_INST`, those of the data `Polymorph::from` names: a character, else the items part).
fn foreign_montage(data: &GameData, w: &World, f: FighterId, inst: Real) -> (Name, Real) {
    let me = w.fighter(f);
    let (name, key) = if inst == COPY_INST {
        let d = me.shapes.disguise.as_ref().expect("a disguise with its copy");
        (d.copy.as_ref().expect("the copy's play").m.montage.name, w.fighter(d.from).ch)
    } else {
        let p = me.shapes.polymorph.as_ref().expect("a polymorph with its play");
        (p.play.as_ref().expect("the polymorph's play").m.montage.name, p.from.expect("the polymorph's data"))
    };
    let n = w.names.str(name);
    let m = match data.chars.get(w.names.str(key)) {
        Some(c) => c.fighter.montages.get(n),
        None => data.items.as_ref().and_then(|i| i.montages.get(n)),
    };
    (name, crate::math::narrow(m.expect("a second mesh's montage in its data").blend_out))
}

/// A second mesh's play that the stop-anim ticks read: on, not stopped and not blending out.
fn foreign_live(w: &World, f: FighterId, inst: Real) -> bool {
    let me = w.fighter(f);
    let play = if inst == COPY_INST {
        me.shapes.disguise.as_ref().and_then(|d| d.copy.as_ref())
    } else {
        me.shapes.polymorph.as_ref().and_then(|p| p.play.as_ref())
    };
    play.is_some_and(|p| !p.stopped && !p.m.blending_out())
}

/// The grouped entry `i`'s instance number (0: none) and whether it fades.
fn grouped_inst(w: &World, f: FighterId, i: usize) -> (i32, bool) {
    let e = &w.fighter(f).montages.grouped.as_ref().expect("the grouped list of an active entry").0[i];
    (e.inst.unwrap_or(0), e.fade.is_some())
}

/// `stopAnimTicks`: the NotifyTicks of the stop-anim notifies.
/// - AnimNotify_MovementStopAnim: IsFalling (@477-523), only MoveY < 0 stops the montage (BPI_MovementStopAnim, default
///   blend-out). Otherwise any MoveX, MoveY < 0 or Jumping stops it, and |MoveX| >= 0.3 then calls
///   BPI_DashFromMovementStopAnim -> DashDanceClient(SkipDashAnim) unless IsFlying or IsFalling (@1357-1487): the dash
///   impulse with no dash montage. Nothing while HitstunBuffActive (MovementStopAnim @172-223): a montage played under a
///   buff stun (the dance of `combat::holds` buffStunVisuals) is not stopped by the stick.
/// - AnimNotify_CrouchStopAnim (361-421): nothing while the CMC IsFalling; otherwise MoveY < 0 stops it.
/// - AnimNotify_OffGroundStopAnim: CharacterMovement.IsFalling (@219-287): `is_falling`. False in a buff stun's
///   suspension and an item's weld (MOVE_Flying) and under a Blueprint's DisableMovement (MOVE_None:
///   Comp_C023A_AllyAttach @2534, Gizmo on an ally), though `grounded` is false there.
///   Unverified: Jumping (BPI_GetJumpInfo @1647) is not among the stops when not falling.
/// - Each tests Animation == GetCurrentActiveMontage first (MovementStopAnim @627-707, OffGroundStopAnim @343-423,
///   CrouchStopAnim @578-620): nothing while a grouped instance played over the montage is the active one.
/// - AnimNotify_OffWallStopAnim: on authority @42-84 (every fighter of the port), the owner a Base_Fighter, not
///   IsWallSliding @219, Animation == GetCurrentActiveMontage @368: BPI_MovementStopAnim(GetCurrentActiveMontage, its
///   GetDefaultBlendOutTime) @583-633, the montage's own blend-out. No input, hitstun or dash test. (C021_Attack_Wall
///   0.002-1.669 s: the wall attack stops when the wall slide ends; AttackNormalWall, its route, has no bytecode reader,
///   so only the smoke sweep plays it.)
pub fn stop_anim_ticks(w: &mut World, f: FighterId) {
    let data = w.data.clone();
    let ground_attack = crate::name!("AnimNotify_GroundAttack_C");
    let movement = crate::name!("AnimNotify_MovementStopAnim_C");
    let off_ground = crate::name!("AnimNotify_OffGroundStopAnim_C");
    let crouch = crate::name!("AnimNotify_CrouchStopAnim_C");
    let off_wall = crate::name!("AnimNotify_OffWallStopAnim_C");
    let ge = if w.fighter(f).montages.grouped.is_some() { active_grouped(w, f) } else { None };
    // `montage`'s own states (an attack instance of another slot group has its turn below; hasActive counts the states
    // of a Disguise's copy and a Polymorph's mesh too: they have their own turn below)
    let on0 = |w: &mut World, cls: Name| {
        let s = &w.fighter(f).shapes;
        if w.fighter(f).montages.grouped.is_some() || s.disguise.is_some() || s.polymorph.is_some() {
            !active_of(w, f, cls, Some(0.0)).is_empty()
        } else {
            has_active(w, f, cls, -1.0)
        }
    };
    let on = |w: &mut World, cls: Name, inst: Real| !active_of(w, f, cls, Some(inst)).is_empty();
    // AnimNotify_GroundAttack Received_NotifyTick: the owner's CharacterMovementComponent (none: nothing @122) not
    // Walking (MovementMode != 1 @229: Falling or Flying, `grounded` false), Animation == MeshComp's
    // GetCurrentActiveMontage @396, then Montage_Stop(0.0, GetCurrentActiveMontage) @532 on that anim instance: no
    // BPI_MovementStopAnim, no hitstun test, blend-out 0. Steven_Special_BarrierThrow_Montage holds the only instance.
    // A Disguise's copy (MeshComp the second mesh, the pawn's CMC): Montage_Stop(0) on that mesh (`stop_copy_montage`).
    // A Polymorph's mesh plays the Creature's montages, which have none.
    // Ahead of the stop-anim ticks below: the NotifyTicks of one montage run in its Notifies index order and
    // Steven_Special_BarrierThrow_Montage has GroundAttack at 3, OffGroundStopAnim at 4 and MovementStopAnim at 19, so
    // its Montage_Stop(0) comes first and the stopped montage is no longer the active one for theirs.
    // Deviation: the order is fixed here (GroundAttack first), not read from each montage's indices.
    if !grounded(w, f) {
        if w.fighter(f).montages.montage.is_some() && ge.is_none() && on0(w, ground_attack) {
            stop_montage(w, f, Some(0.0), None);
        } else if let Some(i) = ge {
            let (inst, fade) = grouped_inst(w, f, i);
            if inst != 0 && !fade && on(w, ground_attack, crate::real(inst)) {
                stop_grouped_entry(w, f, i, 0.0, false);
            }
        }
        if foreign_live(w, f, COPY_INST) && on(w, ground_attack, COPY_INST) {
            stop_copy_montage(w, f);
        }
    }
    let falling = is_falling(w, f);
    let (x, y) = {
        let inp = &w.fighter(f).controls.inp;
        (inp.x, inp.y)
    };
    // MovementStopAnim's stick test, its BPI_DashFromMovementStopAnim, CrouchStopAnim's
    let move_stop = y < 0.0 || (!falling && x != 0.0);
    let move_dash = !falling && !is_flying(w, f) && x.abs() >= 0.3;
    let crouch_stop = !falling && y < 0.0;
    let msa =
        w.fighter(f).montages.montage.is_some() && !w.fighter(f).buffs.buff_stunned && on0(w, movement) && move_stop;
    if w.fighter(f).montages.montage.is_some()
        && ge.is_none()
        && (msa
            || (falling && on0(w, off_ground))
            || (crouch_stop && on0(w, crouch))
            || (!w.fighter(f).wall.wall_sliding && on0(w, off_wall)))
    {
        stop_montage(w, f, None, None);
        if msa && move_dash {
            dash_dance(w, f, true);
        }
    }
    // The same NotifyTicks of an attack instance of another slot group while it is the active montage (the test above;
    // C017_WeaponMode_EnergyDisc_Montage 0.802-1.611 s): BPI_MovementStopAnim(GetCurrentActiveMontage, its default
    // blend-out) stops that instance alone.
    if let Some(i) = ge {
        let (inst, fade) = grouped_inst(w, f, i);
        if inst != 0 && !fade {
            let inst = crate::real(inst);
            let gm = !w.fighter(f).buffs.buff_stunned && on(w, movement, inst) && move_stop;
            if gm
                || (falling && on(w, off_ground, inst))
                || (crouch_stop && on(w, crouch, inst))
                || (!w.fighter(f).wall.wall_sliding && on(w, off_wall, inst))
            {
                let key = w.fighter(f).montages.grouped.as_ref().expect("the grouped list").0[i].m.montage.name;
                let blend = montage_def(&data, w, f, key).expect("a grouped montage in the data").blend_out;
                stop_grouped_entry(w, f, i, crate::math::narrow(blend), false);
                if gm && move_dash {
                    dash_dance(w, f, true);
                }
            }
        }
    }
    // The same NotifyTicks of a Disguise's copy (`combat::disguise`): MeshComp is the second mesh, so the test is
    // against that mesh's own active montage, the copy, and the owner is the pawn. MovementStopAnim does nothing while
    // HitstunBuffActive (@121-223); OffGroundStopAnim's and CrouchStopAnim's NotifyTicks (@219-650, @121-885) have no
    // such test. BPI_MovementStopAnim(the copy's montage, its
    // default blend-out) on the pawn: BaseFighter (@106241, authority) "Movement Stop Animation Multicast All", whose Montage_Stop(Blendout, Montage) on the pawn's Mesh finds
    // no instance of that montage; the pawn's override stops it on the second mesh (`CharHooks::movement_stop_anim`).
    // The dash of the MovementStopAnim case is the pawn's as ever. OffWallStopAnim on the copy: no hitstun test (above),
    // the pawn's IsWallSliding.
    // The same NotifyTicks of a Polymorph's mesh (`combat::polymorph`): the copy's rules, with that mesh's montage.
    // BPI_MovementStopAnim(its montage) reaches the pawn's Montage_Stop by montage, which finds no instance of it on the
    // pawn's Mesh: the creature montage plays on (a pawn's own override, Arya's, is called as for the copy). The dash
    // of the MovementStopAnim case is the pawn's as ever (its DashTriggered(false) plays the creature's turn montage).
    for inst in [COPY_INST, POLY_INST] {
        if !foreign_live(w, f, inst) {
            continue;
        }
        let cm = !in_hitstun(w, f) && on(w, movement, inst) && move_stop;
        if cm
            || (falling && on(w, off_ground, inst))
            || (crouch_stop && on(w, crouch, inst))
            || (!w.fighter(f).wall.wall_sliding && on(w, off_wall, inst))
        {
            if let Some(h) = w.fighter(f).hooks.char.movement_stop_anim {
                let (name, blend) = foreign_montage(&data, w, f, inst);
                h(w, f, name, blend);
            }
            if cm && move_dash {
                dash_dance(w, f, true);
            }
        }
    }
    // The same NotifyTick on a montage without attack state (C019_Nav_Wall_Jump_Montage 0.359-3.417 s): nothing while
    // HitstunBuffActive (@121-223), then the rule above with the montage's own blend-out.
    if let Some(key) = w.fighter(f).montages.dash_montage.as_ref().map(|d| d.montage.name)
        && !in_hitstun(w, f)
        && plain_active(w, f, movement)
        && move_stop
    {
        let blend = montage_def(&data, w, f, key).expect("the dash montage in the data").blend_out;
        stop_dash_montage(w, f, blend);
        if move_dash {
            dash_dance(w, f, true);
        }
    }
}

/// `plainActive`: a notify state of the montage without attack state that is on (its
/// NotifyTick runs).
pub fn plain_active(w: &World, f: FighterId, cls: Name) -> bool {
    let Some(d) = &w.fighter(f).montages.dash_montage else { return false };
    let Some(m) = montage_def(&w.data, w, f, d.montage.name) else { return false };
    let c = w.names.str(cls);
    d.active.iter().any(|&i| m.notifies[i].cls == c)
}

/// `actions`: the releases, the delayed spammed attack, then the press handlers on the stick
/// they read (`press_stick_begin`).
pub fn actions(w: &mut World, f: FighterId) {
    charge_release_input(w, f);
    rage_release_input(w, f);
    fire_spam_delayed(w, f);
    let real = press_stick_begin(w, f);
    press_actions(w, f);
    press_stick_end(w, f, real);
}

/// CharHooks.pressed's button of an attack or special press.
fn attack_btn(b: Btn) -> Option<AttackBtn> {
    match b {
        Btn::Attack => Some(AttackBtn::Attack),
        Btn::Special => Some(AttackBtn::Special),
        _ => None,
    }
}

/// The pawn's own press handling (`CharHooks::pressed`) of an attack or special press.
fn pawn_pressed(w: &mut World, f: FighterId, b: Btn) {
    if let Some(ab) = attack_btn(b)
        && let Some(h) = w.fighter(f).hooks.char.pressed
    {
        h(w, f, ab);
    }
}

/// A press's rank in the frame's key order (`InputState::order`): its index, or the list's length when it is not there.
fn order_rank(order: &[Btn], b: Btn) -> usize {
    order.iter().position(|&o| o == b).unwrap_or(order.len())
}

/// `pressActions`.
fn press_actions(w: &mut World, f: FighterId) {
    let mut presses: Vec<Btn> = PRESS_ORDER.into_iter().filter(|&b| pressed(w, f, b)).collect();
    if presses.iter().any(|&b| b != Btn::Jump && b != Btn::Dodge) {
        emit(w, f, |fighter| Event::CombatInput { fighter });
    }
    if let Some(order) = w.fighter(f).controls.inp.order
        && presses.len() > 1
    {
        let order: Vec<Btn> = order.iter().collect();
        // (a stable sort)
        presses.sort_by_key(|&b| order_rank(&order, b));
    }
    // (`late_press`)
    let many = presses.len() > 1;
    // a press ended the frame's handling here: the later ones reach the pawn's own alone
    let mut dropped = false;
    for b in presses {
        // Pawn_C017's NormalAttackPressed / SpecialAttackPressed overrides call the parent event, then their own
        // handling (`weaponInput`), whatever the parent did (@17650-17670, @16388-16408: no test between the two
        // calls). Each press is its own call from the controller, so a later press of the frame reaches the override
        // too.
        // InputPreTriggered, ahead of the press's every test
        pre_press(w, f, b);
        if b == Btn::Jump {
            jump_pressed_dispatch(w, f);
        }
        if !dropped {
            dropped = press_turn(w, f, b, many);
        }
        pawn_pressed(w, f, b);
    }
    if dropped {
        return;
    }
    if let Some(slot) = w.fighter(f).controls.inp.taunt {
        taunt_press(w, f, i32::from(slot), false);
    }
}

/// One press of `press_actions` (the loop's `try` body): whether it ended the frame's handling (a branch window
/// took it).
fn press_turn(w: &mut World, f: FighterId, b: Btn, many: bool) -> bool {
    // ItemButtonPressed: not in hitstun, not wall sliding, not dead; then ButtonPress and, unless IsAttacking, BFGetAttack
    // with the Item button (only an override answers it). The attack and special presses have the same wall slide gate
    // before ButtonPress (ubergraph 117657 / 118108), so they neither start an attack nor reach a branch window.
    if matches!(b, Btn::Item | Btn::Attack | Btn::Special) && w.fighter(f).wall.wall_sliding {
        return false;
    }
    // JumpEnabled / AttackEnabled, ANDed with the tests below: a disabled press does nothing.
    if !press_enabled(w, f, b) {
        return false;
    }
    // Every press tests CheckHitstunActive before anything else (HitstunSourcesActive or BuffstunSourcesActive above
    // 0): the item, attack and special presses (ubergraph 117196 / 117474 / 117925) and the jump (116236) do nothing
    // then, the evade press (116543) goes to the escape (116757: only with EscapeAvailable, never true here:
    // CanUseEscape is false in the CDO and nothing sets it). A knockback hitstun's frame does not come here; a buff
    // stun's does, and so does a press that took a hit earlier in the frame: the jump press's own JumpPressedDispatch,
    // broadcast ahead of its test (@122712 -> @122755 -> @116236), reaches HarleyJackInTheBox AfterJumpTriggered
    // (@8827-9155: pfgDoDamage with AllyHitBox, then ResetJump(0)), so the jump itself is refused and a later press of
    // the frame too; or an earlier press's overlap update (a free attack's DoPreAttackRotation turn). Each press is its
    // own input event with its own CheckHitstunActive, so the test is made again per press, on the handler's sources
    // (HitstunSourcesActive: `stun_source`, set by the hit's AddHitstunSource @2556; the buff stun's
    // HitstunBuffActive), not `in_hitstun` (which counts a pull without a buff stun). The jump and the item, attack and
    // special presses also test isDead (116432; 117196 / 117474 / 117925 with the InputEnabled gates); a dead pawn's
    // evade press still saves the pair (PFGPlayerController @9670-10008 reaches its EvadeButtonPressed).
    // The evade press is not the escape path: EvadeButtonPressed (@123236 -> @123319 -> ButtonPress @123368) tests
    // nothing (StartEvade tests hitstun), so it reaches InputTriggered_Event in a hitstun too.
    let me = w.fighter(f);
    if me.attackers.stun_source || me.buffs.buff_stunned || me.life.dead > 0.0 {
        if b == Btn::Dodge {
            press_input(w, f, b);
            late_press(w, f, Some(b), many, None);
        }
        return false;
    }
    if b == Btn::Special {
        air_special_cue(w, f);
    }
    // Evade press (ubergraph 123319 / 188502): WasEvading = isEvading, ButtonPress, then while isEvading no StartEvade;
    // in the air with every air evade used and WasEvading, the cooldown cue.
    let was_evading = w.fighter(f).evade.evading;
    // ButtonPress -> InputTriggered_Event, before the windows and the free action
    press_input(w, f, b);
    late_press(w, f, Some(b), many, None);
    if has_attack_inst(w, f) {
        let done = window_press(w, f, b, false);
        if b == Btn::Dodge && w.fighter(f).evade.evading {
            if was_evading && in_air(w, f) && crate::real(w.fighter(f).air.air_evades) == AIR_EVADE_MAX {
                sfx(w, f, Some(crate::name!("ui_ability_cooldown_alt")));
            }
            return done;
        }
        if done {
            return true;
        }
    }
    // IsAttacking after ButtonPress, with or without an attack instance: the attack (115469), special (118371), item
    // (169333) and jump (115409) presses and the free evade (StartEvade @114515) do nothing while the branch manager
    // counts an active attack. Without a montage that is a buff's AddActiveAttack, which can be made inside
    // ButtonPress's own broadcast (Buff_C015AttachToDogpile BeginPlay @703, spawned by the TeamFightCloud perk).
    if is_attacking(w, f) {
        return false;
    }
    // Each press is its own input event: a later press of the frame sees what the earlier one started. Not attacking,
    // the attack and special press call ClearQueuedAttack before BFGetAttack (ubergraph 115600 / 118431), whether or
    // not an attack starts.
    if matches!(b, Btn::Attack | Btn::Special) {
        clear_queued_attack(w, f);
    }
    free_action(w, f, b);
    false
}

/// `pawnPresses`: the pawn's own press handling (`CharHooks::pressed`) on a frame `actions`
/// does not run (a hit pause, hitstun, dead). PFGPlayerController calls NormalAttackPressed / SpecialAttackPressed with
/// no test of the pawn's state (@144214, @151325, @98748 ...); the tests (hitstun, wall slide, attack / input enabled,
/// dead, the input type's direction: BaseFighter ubergraph 117474-117876, 117925-118337) are inside the parent's event,
/// and Pawn_C017's overrides run NormalInputeServer / SpecialInputeServer after it whatever it did (@17650-17670,
/// @16388-16408). So the handler's own tests decide. The frame's order is the press order (`actions`).
/// Deviation: the parent's handling of every press of such a frame runs before this (in the build, per press).
pub fn pawn_presses(w: &mut World, f: FighterId) {
    let Some(h) = w.fighter(f).hooks.char.pressed else { return };
    let (a, s) = (pressed(w, f, Btn::Attack), pressed(w, f, Btn::Special));
    if !a && !s {
        return;
    }
    let mut special_first = false;
    if a && s
        && let Some(o) = w.fighter(f).controls.inp.order
    {
        let o: Vec<Btn> = o.iter().collect();
        special_first = order_rank(&o, Btn::Special) < order_rank(&o, Btn::Attack);
    }
    let real = press_stick_begin(w, f);
    if s && special_first {
        h(w, f, AttackBtn::Special);
    }
    if a {
        h(w, f, AttackBtn::Attack);
    }
    if s && !special_first {
        h(w, f, AttackBtn::Special);
    }
    press_stick_end(w, f, real);
}

/// `bfTaunt`: BFTaunt(slot) (BotTaunt): BranchedTauntSlot = slot, then TauntPressedLogic.
pub fn bf_taunt(w: &mut World, f: FighterId, slot: i32) {
    let in_hitpause = w.fighter(f).hit.hitpause > 0.0;
    taunt_press(w, f, slot, in_hitpause);
}

/// `tauntPress`: BFTaunt(slot) -> BranchedTauntSlot, TauntPressedLogic (ubergraph 144630):
/// only when not in hitstun, not dead, attack and input enabled, not BlockTauntAnims, not Jumping / JumpScheduled /
/// PendingLaunch and IsMovingOnGround. Then ButtonPress(4, 5): a branch window with CanBranchToTaunt offers only the
/// slot's GetTauntBranchStruct (Montage, Conditions, Priority, AllowPreAttackRotation true). Not attacking, it calls
/// BFGetAttack with that struct (no spam delay for taunts), AddRecentAttackToQueue, DoPreAttackRotation(5) (the turn to
/// the stick), BeginAttack and plays the montage. An empty slot (no montage) does nothing.
pub fn taunt_press(w: &mut World, f: FighterId, slot: i32, in_hitpause: bool) {
    // TauntPressedLogic @144630: broadcast InputPreTriggered(4, 5) ahead of every test (its listener:
    // `combat::polymorph`).
    // Unverified: the locally controlled press event's own broadcast @144583 (then ButtonPress, goto 79091) is taken as
    // the same press's, sent once.
    if !w.fighter(f).buffs.buffs.is_empty() {
        let arg = BuffSignalArg { input: Some(4), dir: Some(5.0), ..BuffSignalArg::default() };
        buff_signal(w, f, BuffSignal::InputPreTriggered, arg);
    }
    // the pawn's binding (`CharHooks::input_pre_triggered`), after the buffs'
    if let Some(h) = w.fighter(f).hooks.char.input_pre_triggered {
        h(w, f, 4, 5);
    }
    // TauntPressedLogic @144927 (Pawn_C017's weapon mode)
    if w.fighter(f).controls.block_taunt_anims {
        return;
    }
    // GetAttackEnabled @144761
    if !attack_enabled(w, f) {
        return;
    }
    // 144994-145183: !(Jumping || JumpScheduled || PendingLaunch) && IsMovingOnGround
    if in_hitstun(w, f) || w.fighter(f).life.dead > 0.0 || !grounded(w, f) {
        return;
    }
    let me = w.fighter(f);
    if me.moves.cmd_jumped
        || me.moves.ground_jump.is_some()
        || (me.hit.launch.is_some() && !me.hit.free_launch)
        || me.moves.jump_launched
    {
        return;
    }
    // GetTauntBranchStruct
    let br = taunt_branch(w, f, slot);
    // TauntSlotToExecute (Comp_Pawn_TauntManager; AnimNotify_PlayEmoteVFX reads it): absent while it is 0.
    let record_slot = |w: &mut World| {
        let c = &mut w.fighter_mut(f).controls;
        if slot != c.taunt_slot.unwrap_or(0) {
            c.taunt_slot = Some(slot);
        }
    };
    // ButtonPress(4, 5) @145231
    save_branch_input(w, f, 4, dir_num(Dir::Default));
    // Unverified: not attacking, the taunt press drops the late buffer as a free attack's ClearQueuedAttack does; no
    // ClearQueuedAttack on the free taunt path is read.
    late_press(w, f, None, false, br.clone().map(Box::new));
    if w.fighter(f).montages.montage.is_some() {
        // ButtonPress(4, 5): the buffered windows take the press (an empty slot too), an Immediate one acts now. An
        // automatic Immediate window is in CurrentImmediateBranchData as well (BranchNotifyStart @586-1127) and
        // SetActiveAttackFromBranch tests the taunt (@3484-3917) before it skips automatic windows (@3922), so a taunt
        // press inside one finds the slot's branch (Shaggy_Taunt_ShirtRip 0-0.413 s). The press stores ButtonPress's
        // Direction 5 (Default, @145231), not the stick's, so the branch's DoPreAttackRotation at the window's end turns
        // as the free taunt's does.
        let wrote = buffer_press(w, f, QueuedBtn::Taunt, Dir::Default, None, br.as_ref());
        // SetActiveAttackFromBranch @3734 sets TauntSlotToExecute = BranchedTauntSlot (BFTaunt's slot) where a window
        // takes the taunt.
        // Deviation: a buffered window's is written at the press here (its branch is this press's too), not at the
        // window's end.
        if wrote {
            record_slot(w);
        }
        let data = w.data.clone();
        let imm = open_windows(w, f, None).into_iter().any(|o| {
            notify_flag(&data, w, f, o.n, "Immediate") && notify_flag(&data, w, f, o.n, "CanBranchToTaunt") && {
                let conds = notify_def(&data, w, f, o.n).and_then(|d| notify_names(w, d, "BranchWindowConditions"));
                conds_ok(w, f, conds.as_deref(), false)
            }
        });
        if let Some(b) = &br
            && imm
            && row_ok(w, f, &branch_row(w, b))
        {
            record_slot(w);
            execute(w, f, ExecuteKind::Branch, Some(b), None, Dir::Default);
            return;
        }
        if is_attacking(w, f) || in_hitpause {
            return;
        }
    }
    // TauntSlotToExecute = slot @145310, ahead of BFGetAttack
    record_slot(w);
    let Some(br) = br else { return };
    if !row_ok(w, f, &branch_row(w, &br)) {
        return;
    }
    // DoPreAttackRotation(5, false, StickAttackX) @145735
    free_rotation(w, f, None, Dir::Default);
    let attack = data_name(w, &br.attack);
    add_recent_attack(w, f, Btn::Attack, Some(attack));
    attack_begin(w, f, true);
    w.fighter_mut(f).abilities.attack_slot = br.ability_slot.unwrap_or(-1);
    start_montage(w, f, attack, &PROXY_NONE, None);
}

/// `rowOk`'s row of a branch: its conditions (an empty one the none name) and ability slot.
fn branch_row(w: &World, br: &Branch) -> RowOkR {
    let conditions = br.conditions.iter().map(|c| c.as_deref().map_or(Name::NONE, |s| data_name(w, s))).collect();
    RowOkR { conditions: Some(conditions), ability_slot: br.ability_slot }
}

/// `freeRotation`: the free press's call: its own direction, the live pair and StickAttackX.
fn free_rotation(w: &mut World, f: FighterId, force: Option<ForcedPress>, dir: Dir) {
    let (x, rsx) = {
        let inp = &w.fighter(f).controls.inp;
        (inp.x, inp.rsx.unwrap_or(0.0))
    };
    pre_attack_rotation(w, f, force, dir, x, rsx);
}

/// `freeAction`: normal (non-window) actions: attack tables, jump, dodge.
pub fn free_action(w: &mut World, f: FighterId, b: Btn) -> bool {
    match b {
        Btn::Jump => {
            // JumpPressedLocal, when not attacking (ubergraph 115409 -> 76454; a drop through a platform returns before
            // it): CrouchJump = isCrouching || bIsCrouched, then StartJump. A jump branched out of an attack
            // (BPI_StartJump) keeps the last value.
            // The drop (76454 -> 76755): isPushingDown, not GetIsInAir and the movement base's owner is a
            // BasePlatformBP: isDroppingDown, HandlePlatformCollision, FastFallTriggered = false, SetCrouching(false),
            // and no jump. The same steps run on a SpawnedPlatformParent whose CanPassThrough(the fighter's team) is
            // true (76807-77060; Morty's ship: AlliesDropThrough true, EnemiesDropThrough false). The ship's pieces stay
            // solid all the same (Ship.canPassThrough), so there the press only does these four things.
            // A bot's BPI_DownPressed right before its JumpPressed (BotMoveTo) has set isPushingDown by now; the port
            // runs that call with the fast fall handler, after this.
            let me = w.fighter(f);
            let direct = me.controls.inp.down_btn && !me.crouch.down_btn && !in_hitstun(w, f) && me.life.dead <= 0.0;
            if (me.crouch.pushing_down || direct) && !in_air(w, f) && drop_base(w, f) {
                let me = w.fighter_mut(f);
                me.platforms.dropping_down = true;
                let vy = me.body.vy;
                handle_platform_collision(w, f, vy);
                w.fighter_mut(f).fast_fall.ff_triggered = false;
                set_crouching(w, f, false);
                return true;
            }
            let me = w.fighter_mut(f);
            me.jump.crouch_jump = me.crouch.crouching || me.capsule.cmc_crouched;
            try_jump(w, f)
        }
        Btn::Dodge => {
            // StartEvade -> EvadeFromBranch -> 39171: an air evade refused (count or EvadeGate) goes to 43606, the
            // cooldown cue when AirEvadeNumber == AirEvadeMax. BPI_EvadeFromBranch (a window's branch) has no cue.
            // EvadeFromBranch @139063-139336: GetEvadeEnabled (and InputEnabled) before the body, so a disabled evade
            // does nothing, the cue neither.
            if !evade_enabled(w, f) {
                return false;
            }
            let neutral = evade_force(w, f).is_some();
            // GetInputTypeDirectionEnabled(3, ...) of the same test (@139136)
            let (x, y) = (w.fighter(f).controls.inp.x, w.fighter(f).controls.inp.y);
            if !evade_type_enabled(w, f, neutral, x, y) {
                return false;
            }
            let ok = try_dodge(w, f, neutral);
            if !ok && in_air(w, f) && crate::real(w.fighter(f).air.air_evades) == AIR_EVADE_MAX {
                sfx(w, f, Some(crate::name!("ui_ability_cooldown_alt")));
            }
            ok
        }
        Btn::Attack | Btn::Special | Btn::Item => {
            // BFGetAttack: BranchManagerGetAttack (CheckForOverride before the attack table), then the spam delay. Its
            // "AirAttackAvailable?" is the function's parameter (EX_LocalVariable @404), not the cooldown flag: true
            // for normals and AirSpecialNumber < AirSpecialMax for the air special (118638), where an attack that
            // carries AirSpecialLimitCondition has already passed the same test, so the drop at 598 is never reached.
            // The lookup's direction is GetSwappedAttackDirs' (BFGetAttack @34, GetIsInAir @11); the override check
            // (@126 UnmodifiedDir) and the turn take the press's own.
            let d = press_dir(w, f);
            let air = in_air(w, f);
            // the table BFGetAttack was given (GetIsInAir, 118523)
            let air_special = b == Btn::Special && air;
            let sd = swapped_dir(w, f, Some(b), d, in_air(w, f));
            let Some(e) = pick_attack(w, f, b, sd, d, None) else { return false };
            if spammed_attack(w, f, b, sd, e.montage, d) {
                return true;
            }
            // Pre-attack rotation: the fighter turns to the stick's side (or the forced direction) first. The free
            // path calls DoPreAttackRotation whatever AllowPreAttackRotation says (only the branch path checks it).
            let force = forced_press(w, f);
            free_rotation(w, f, force, d);
            add_recent_attack(w, f, b, Some(e.montage));
            attack_begin(w, f, true);
            w.fighter_mut(f).abilities.attack_slot = e.ability_slot.unwrap_or(-1);
            start_montage(w, f, e.montage, &PROXY_ATTACK, None);
            // The air special's free start (ubergraph 118583 -> 30916, after the montage play): AirSpecialNumber + 1
            // unless ByPassAirAttack (the attack has no AirSpecialLimitCondition). No movement mode test here.
            let limit = crate::name!("AirSpecialLimitCondition_C");
            if air_special && e.conditions.as_ref().is_some_and(|c| c.contains(&limit)) {
                increment_air_special(w, f);
            }
            if matches!(b, Btn::Attack | Btn::Special) {
                free_attack_tutorial(w, f, b == Btn::Special, air, crate::real(dir_num(d)));
            }
            true
        }
    }
}

/// `forceAttack`: BaseFighter BPI_ForceAttack(Attack) (ubergraph 105081 -> 105065
/// PerformedAttack = false -> 102439; caller: Comp_JakeEat). Out of hitstun (CheckHitstunActive: the handler's
/// sources), not attacking (IsAttacking) and not dead (@102439-102692), else nothing:
/// - ButtonPress(row button, row direction, false) @102702: the InputTriggered broadcast with that pair
///   (`save_branch_input`; a special's air special cue is the branch manager's InputTriggered_Event);
/// - ClearQueuedAttack @102753 and EndJump(0, false, false) @102789, whether or not an attack starts;
/// - BFGetAttack([row], [], button, direction, AirAttackAvailable? true, ForceAttackDirection false) @102852:
///   CheckForOverride before the row, the row's conditions and ability check, then HandleDelayedSpammedAttack;
/// - with a montage (@102978): AddRecentAttackToQueue, DoPreAttackRotation(direction, false, StickAttackX) on the live
///   MoveX, the attack start (AirControlGate, DashFallGate, CheckLateralFriction: `attack_begin` without its
///   CancelInvuln, which this path never calls), BeginAttack, RemoveQueuedHitPause @103181, the montage, then 37660
///   (NormalAttackPressedServer / SpecialAttackPressedServer). No air special count (the free special's 30916 is not on
///   this path). Returns whether an attack started (PerformedAttack).
///   Deviation: RemoveQueuedHitPause (@103181) is not taken: no attack start of the port runs it (the free press's own
///   @90685 / @90994 included).
///
/// The row's button 0 goes through GetSwappedAttackDirs (BFGetAttack @34): under the swap a Forward, Back or Neutral row
/// finds no match in its one-row table. ButtonPress sets ButtonStates[button] (`save_branch_input`; no release follows
/// here).
/// Deviation: ButtonPress also offers the press to an open Immediate branch window (the manager's
/// InputTriggered_Event); the port does not (IsAttacking false leaves only a window of a montage whose attack flag has
/// ended).
pub fn force_attack(w: &mut World, f: FighterId, row: &AttackEntry) -> bool {
    let me = w.fighter(f);
    if me.attackers.stun_source || me.buffs.buff_stunned || is_attacking(w, f) || me.life.dead > 0.0 {
        return false;
    }
    let b = match row.required_button_input {
        0 => Some(Btn::Attack),
        1 => Some(Btn::Special),
        5 => Some(Btn::Item),
        _ => None,
    };
    let d = row.required_direction_input;
    save_branch_input(w, f, row.required_button_input, d);
    if b == Some(Btn::Special) {
        air_special_cue(w, f);
    }
    clear_queued_attack(w, f);
    end_jump_at_start(w, f);
    let Some(b) = b else {
        let msg = format!("BPI_ForceAttack: button {} has no attack press", row.required_button_input);
        warn_text(w, &msg);
        return false;
    };
    let d = dir_of(d).expect("an attack row's direction byte");
    let sd = swapped_dir(w, f, Some(b), d, in_air(w, f));
    let Some(e) = pick_attack(w, f, b, sd, d, Some(std::slice::from_ref(row))) else { return false };
    if spammed_attack(w, f, b, sd, e.montage, d) {
        return false;
    }
    add_recent_attack(w, f, b, Some(e.montage));
    free_rotation(w, f, None, d);
    attack_begin(w, f, false);
    w.fighter_mut(f).abilities.attack_slot = e.ability_slot.unwrap_or(-1);
    // Unverified: BPI_ForceAttack's play is taken as one of BaseFighter's free-attack proxies (BeginAttack = false).
    start_montage(w, f, e.montage, &PROXY_ATTACK, None);
    true
}

/// `bpiSetBlockTaunt`: BaseFighter BPI_SetBlockTaunt(BlockTaunt): BlockTauntAnims =
/// BlockTaunt (read by TauntPressedLogic @144927). Callers: Pawn_C017's weapon mode (writes the field itself), Gizmo
/// (C023A).
pub fn bpi_set_block_taunt(w: &mut World, f: FighterId, block: bool) {
    w.fighter_mut(f).controls.block_taunt_anims = block;
}

/// `incrementAirSpecial`: BPI_IncrementAirSpecialNumber (ubergraph 99261) and the free
/// start's own add: AirSpecialNumber + 1.
pub fn increment_air_special(w: &mut World, f: FighterId) {
    w.fighter_mut(f).air.air_specials += 1;
    // HandleAirMoveDesaturation @31055 / @99334
    handle_air_move_desaturation(w, f);
}

/// `triggerAirSpecialCost`: Trigger Current Attack Client @9406-9895 (a branch, an
/// automatic branch, the delayed spammed attack), before the montage plays: BPI_IncrementAirSpecialNumber(1) when the
/// CMC IsFlying or IsFalling (so not in the knockback mode and not for a ground jump before liftoff), the attack's
/// button is the special and it carries AirSpecialLimitCondition.
pub fn trigger_air_special_cost(w: &mut World, f: FighterId, special: bool, conds: Option<&[Name]>) {
    let limit = crate::name!("AirSpecialLimitCondition_C");
    if !special || !conds.is_some_and(|c| c.contains(&limit)) {
        return;
    }
    if is_flying(w, f) || is_falling(w, f) {
        increment_air_special(w, f);
    }
}

/// `execute`: Trigger Current Attack Client. Returns false for a jump or an evade that did
/// not start (@7377, @8508: the function leaves without clearing the maps); everything else ends at @5768
/// (ActiveAttack reset, Map_Clear of CurrentBranchData and CurrentAutoBranchData).
/// `dir`: the stored press's direction (BranchInfo.BranchInputDirection, SetActiveAttackFromBranch @3995 -> @2582);
/// Default for the branch of a window that held no press (`pre_attack_rotation`).
pub fn execute(
    w: &mut World,
    f: FighterId,
    kind: ExecuteKind,
    br: Option<&Branch>,
    force: Option<ForcedPress>,
    dir: Dir,
) -> bool {
    if !execute_branch(w, f, kind, br, force, dir) {
        return false;
    }
    // ... and the windows of another anim instance with the maps; a bot's jump keeps the Immediate ones (@7240)
    let keep = kind == ExecuteKind::Jump && w.fighter(f).controls.bot_controlled;
    clear_foreign_windows(w, f, keep);
    // @5768 -> @5993 PreviousAttackCancelled = false on every exit past ActiveAttack's reset (both
    // StopImemediateBranchClear branches: @5968 -> @6036 -> @5982 -> @5993; the evade @8443 / @8503, the taunt @6982,
    // the jump @7251), after the evade path's read (@7579, branchEvade)
    clear_attack_cancelled(w, f);
    let b = &mut w.fighter_mut(f).branching;
    b.late_auto = None;
    // Trigger Current Attack Client clears CurrentBranchData when it is done
    b.late_branch = None;
    true
}

/// `executeBranch`. The stick: the evade path (@7737-8457) saves the pawn's
/// MoveXAxisValue / MoveYAxisValue, writes InputAxisX / InputAxisY (the last press's pair, `save_branch_input`) over
/// them for BPI_EvadeFromBranch and puts the saved pair back after it; the attack path (@9006-9342) writes InputAxisX
/// alone over MoveXAxisValue and calls BPI_HandlePreAttackRotation with RightStickInputX, so the turn reads the press's
/// stick, not the window end's. The jump path reads the live pair (StartJump). A trigger at the press itself (an
/// Immediate window) finds the pair it has just saved.
/// Deviation: the attack path does not put MoveXAxisValue back, and the evade path does not put the pair back when
/// BPI_EvadeFromBranch answers CanEvade false (@8239 -> @8508). On a gamepad the next frame's axis event writes it
/// again, so the rest of the frame (the manager's own tick, the move commands) reads the press's pair; on keys the
/// value stays until the next Left / Right key event. The port gives the press's pair to the turn and the evade alone
/// and has the live pair back after the call.
pub fn execute_branch(
    w: &mut World,
    f: FighterId,
    kind: ExecuteKind,
    br: Option<&Branch>,
    force: Option<ForcedPress>,
    dir: Dir,
) -> bool {
    match kind {
        ExecuteKind::Jump => {
            // BPI_StartJump (BaseFighter): StartJump only with CurrentJumpNumber < MaxJumpNumber, so a branch cannot
            // reach StartJump's wall traces with the jumps spent, where the free press (JumpPressedLocal) can. The
            // jump stops the montage itself, and only when it starts.
            return crate::real(w.fighter(f).jump.jumps) < max_jumps(w, f) && try_jump(w, f);
        }
        ExecuteKind::Dodge => {
            let neutral = force.is_some_and(|p| p.dir == dir_num(Dir::Neutral));
            // @7459-7617: ApplyCancelEvadeBuffs
            branch_evade(w, f, neutral);
            // BPI_EvadeFromBranch -> EvadeFromBranch (ubergraph 139063 -> 116543 -> 114740): while isEvading the event
            // goes to 77199 and starts nothing (in the air only the cooldown cue at 43606, which the press's caller
            // plays), so an evade's own window (CanBranchToEvade) cannot start the evade again. BPI_EvadeFromBranch
            // still answers CanEvade true (@141) unless in the air with every air evade used (@75 -> @157). Before
            // EvadeFromBranch it calls DoPreAttackRotation(Direction, false, 0) (@89), with no test of isEvading
            // (RotateInAnyMovementMode is on in the class default): he turns to the press's X (the pair written in at
            // @7925-8050). Direction is the evade press's (5, or 4 for Neutral Evade), for which LocalAttackFacing
            // stays 0. With the evade disabled (GetEvadeEnabled false, EvadeFromBranch @139336 pops) the same: the
            // count test, the turn and CanEvade true, no evade. Not evading, the turn @89 comes ahead of
            // EvadeFromBranch all the same, so the evade's Get8WayDirection reads the turned facing (`try_dodge`,
            // whose own turn to MoveX @116642-116729 then finds him turned). With every air evade used nothing past
            // the count test @75 runs: no stop, no EndJump.
            if in_air(w, f) && crate::real(w.fighter(f).air.air_evades) >= AIR_EVADE_MAX {
                return false;
            }
            let (bx, by) = {
                let b = &w.fighter(f).branching;
                (b.br_axis_x, b.br_axis_y)
            };
            pre_attack_rotation(w, f, None, Dir::Default, bx, 0.0);
            if w.fighter(f).evade.evading || !evade_enabled(w, f) || !evade_type_enabled(w, f, neutral, bx, by) {
                return true;
            }
            let inp = &mut w.fighter_mut(f).controls.inp;
            let live = (inp.x, inp.y);
            (inp.x, inp.y) = (bx, by);
            // StopAnimMontage(null): the active montage alone, an instance of another slot group when one is.
            if !stop_active_grouped(w, f) {
                stop_montage(w, f, None, None);
            }
            let r = try_dodge(w, f, neutral);
            let inp = &mut w.fighter_mut(f).controls.inp;
            (inp.x, inp.y) = live;
            return r;
        }
        ExecuteKind::Branch => {}
    }
    let Some(br) = br else { return true };
    let special = strip_enum(&br.button) == "AB_SpecialAttack";
    let attack = data_name(w, &br.attack);
    // Trigger Current Attack
    add_recent_attack(w, f, if special { Btn::Special } else { Btn::Attack }, Some(attack));
    branch_row_tutorial(w, f, &br.button, &br.direction);
    let conds: Vec<Name> = br.conditions.iter().map(|c| c.as_deref().map_or(Name::NONE, |s| data_name(w, s))).collect();
    trigger_air_special_cost(w, f, special, Some(&conds));
    attack_begin(w, f, true);
    w.fighter_mut(f).abilities.attack_slot = br.ability_slot.unwrap_or(-1);
    // Comp_Pawn_BranchManager @8712: StopAnimMontage(null) before the play
    stop_active_grouped(w, f);
    // ... and before the turn (@9342), whatever the stop ended
    if br.allow_pre_attack_rotation {
        let (bx, bs) = {
            let b = &w.fighter(f).branching;
            (b.br_axis_x, b.br_stick_x)
        };
        pre_attack_rotation(w, f, force, dir, bx, bs);
    }
    // Trigger Current Attack Client's attack play @12224 (proxy 2CFFE2); its taunt play @6381 (proxy 604235) binds
    // nothing that writes.
    // Unverified: the port keeps the clears of a play with no proxy for the taunt play.
    // A taunt's row has no button (tauntBranch).
    let proxy = if br.button.is_empty() { PROXY_NONE } else { PROXY_BRANCH_ATTACK };
    start_montage(w, f, attack, &proxy, None);
    true
}

/// `attackBegin`: attack start (attack table, branch, automatic branch): CancelInvuln(false).
/// invuln false: BPI_ForceAttack's start, which has no CancelInvuln (@102439-104487, 37660-38151).
pub fn attack_begin(w: &mut World, f: FighterId, invuln: bool) {
    end_jump_at_start(w, f);
    if invuln {
        cancel_invuln(w, f, false);
    }
    lateral_friction_event(w, f, true, false);
}

/// `airAttackCoolDown`: BaseFighter AirAttackCoolDown (AnimNotify_SetAirAttackCoolDown):
/// only from available; Delay(cd) sets it back. A Delay already pending is not restarted (UE latent Delay ignores
/// repeat calls). Jumps, air evades, platform evades, landing and SetJumpData also set it back.
pub fn air_attack_cool_down(w: &mut World, f: FighterId, cd: Real) {
    let air = &mut w.fighter_mut(f).air;
    if !air.air_attack_available {
        return;
    }
    air.air_attack_available = false;
    if air.air_attack_delay <= 0.0 {
        air.air_attack_delay = js::max(1.0, js::round(cd * 60.0));
    }
}

/// `addRecentAttack`: Comp_Pawn_BranchManager AddRecentAttackToQueue (locally controlled):
/// called by every started attack (free start and branch), evade (the air and up entries carry their montage, the
/// ground evade none) and jump (ground, double, wall; no montage). It clears the SpammedAttackDelayTimer, inserts at 0,
/// stamps AttackSpamTimes and keeps 4 entries.
pub fn add_recent_attack(w: &mut World, f: FighterId, btn: Btn, montage: Option<Name>) {
    let me = w.fighter_mut(f);
    let time = me.clock.time;
    let c = &mut me.controls;
    c.spam_delayed = None;
    c.recent_attacks.insert(0, (btn, montage));
    c.attack_spam_times.insert(montage, time);
    while crate::real(u32::try_from(c.recent_attacks.len()).unwrap_or(u32::MAX)) > RECENT_ATTACK_QUEUE {
        c.recent_attacks.pop();
    }
}

/// `spammedAttack`: HandleDelayedSpammedAttack (from BFGetAttack, so only for attacks
/// started outside a branch): the newest entry is a jump or an evade, the new attack is neither (nor a taunt), and its
/// montage is entry 1 or 2 -> if less than AttackSpamTimeWindow has passed since that montage's last entry, the attack
/// is not returned; its button and directions are kept and a timer for the rest of the window runs
/// DoDelayedSpammedAttack. A repeated press sets the timer again (same expiry). `d`: the matched row's direction
/// (BranchManager @1153, DelayedSpammedAttackDirection, after GetSwappedAttackDirs), `u`: the press's unmodified one
/// (@1208, DelayedSpammedAttackUnmoddedDir).
/// Timer frame (stock UE 4.26 FTimerManager, not read in the exe): a timer set in the pawn tick is pending until that
/// frame's timer tick (after the actor tick groups), which starts it without counting the frame's delta; it fires at
/// the first later timer tick with elapsed > rate, and the montage first advances a frame later: the trigger runs at
/// the start of the next tick. The rate is a float32 world-clock difference, so the clock's step rounding decides the
/// tie (the start + 46 frames at 100 s, + 47 at 3900 s in memory).
/// The rule is read in the exe: InternalSetTimer 142e3de10, FTimerManager::Tick 142e45520, called from UWorld::Tick
/// 142b4e150 after the tick groups and latent actions with the world's DeltaSeconds.
#[allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "the negated comparison `!(delay < 0)` and `!(el > rate)`, which a NaN passes"
)]
pub fn spammed_attack(w: &mut World, f: FighterId, b: Btn, d: Dir, montage: Name, u: Dir) -> bool {
    let force = forced_press(w, f).map(|p| u8::try_from(p.dir).expect("a directional action's byte"));
    let me = w.fighter_mut(f);
    let c = &me.controls;
    let q = &c.recent_attacks;
    if q.first().is_none_or(|e| e.0 != Btn::Jump && e.0 != Btn::Dodge) {
        return false;
    }
    let at = |i: usize| q.get(i).and_then(|e| e.1);
    if at(1) != Some(montage) && at(2) != Some(montage) {
        return false;
    }
    // LocalSpammedAttackDelay = (GetGameTimeInSeconds - AttackSpamTimes[montage]) - AttackSpamTimeWindow (float);
    // the timer rate is its negation. FTimerManager's own clock adds DeltaSeconds per timer tick and fires at the
    // first tick with elapsed > rate.
    let last = c.attack_spam_times.get(&Some(montage)).copied().unwrap_or(-1e9);
    let delay = (me.clock.time - last) - ATTACK_SPAM_WINDOW;
    if !(delay < 0.0) {
        return false;
    }
    let rate = -delay;
    let mut k = 0.0;
    // (FTimerManager's clock: a double sum of the float frame times)
    let mut el: crate::Clock = 0.0;
    while !(el > f64::from(rate)) {
        el += f64::from(DELTA_SECONDS);
        k += 1.0;
    }
    let at = crate::real(me.clock.tick) + k + 1.0;
    me.controls.spam_delayed = Some(SpamDelayed { b, d, at, force, u: (u != d).then_some(u) });
    true
}

/// `fireSpamDelayed`: DoDelayedSpammedAttackLogic: BranchManagerGetAttack over all attacks
/// with the kept button and direction (conditions as they are now), then Trigger Current Attack Client: nothing when
/// dead or in hitstun (step), else the branch path (queue entry, StopAnimMontage, pre-attack rotation from the kept
/// input direction on the manager's saved pair, montage; no AirAttackAvailable check). The lookup takes the kept row
/// direction (@147), the turn the unmodified one (Trigger Current Attack Client's UnmoddedDir, @333).
pub fn fire_spam_delayed(w: &mut World, f: FighterId) {
    let me = w.fighter(f);
    let Some(s) = &me.controls.spam_delayed else { return };
    if crate::real(me.clock.tick) < s.at {
        return;
    }
    let s = w.fighter_mut(f).controls.spam_delayed.take().expect("the delayed attack tested above");
    // BranchManagerGetAttack @147 takes DelayedSpammedAttackDirection as its UnmodifiedDir too, so the override check
    // compares the kept row direction, not the live stick.
    let Some(e) = pick_attack(w, f, s.b, s.d, s.d, None) else { return };
    // Trigger Current Attack Client(DelayedSpammedAttackForceDir, DelayedSpammedAttackUnmoddedDir) @333: the attack
    // path's MoveXAxisValue = InputAxisX and RightStickInputX, the last press's.
    if e.allow_pre_attack_rotation {
        let force = s.force.map(|d| ForcedPress { dir: i32::from(d) });
        let (bx, bs) = {
            let b = &w.fighter(f).branching;
            (b.br_axis_x, b.br_stick_x)
        };
        pre_attack_rotation(w, f, force, s.u.unwrap_or(s.d), bx, bs);
    }
    add_recent_attack(w, f, s.b, Some(e.montage));
    // The row's button and direction are the kept pair (the tables match them exactly); an item row has neither.
    let button = match s.b {
        _ if e.item => -1.0,
        Btn::Attack => 0.0,
        Btn::Special => 1.0,
        _ => -1.0,
    };
    branch_attack_tutorial(w, f, button, crate::real(dir_num(s.d)));
    trigger_air_special_cost(w, f, s.b == Btn::Special && !e.item, e.conditions.as_deref());
    attack_begin(w, f, true);
    w.fighter_mut(f).abilities.attack_slot = e.ability_slot.unwrap_or(-1);
    // Comp_Pawn_BranchManager @8712
    stop_active_grouped(w, f);
    // Trigger Current Attack Client's play @12224
    start_montage(w, f, e.montage, &PROXY_BRANCH_ATTACK, None);
    // the trigger's exit (@5768-6036)
    clear_foreign_windows(w, f, false);
}

/// What `pickAttack` answers: the row's montage, AllowPreAttackRotation, Conditions and AbilitySlot, and whether it is
/// an item row.
#[derive(Clone, Debug)]
pub struct PickedAttack {
    pub montage: Name,
    pub allow_pre_attack_rotation: bool,
    pub conditions: Option<Vec<Name>>,
    pub ability_slot: Option<i32>,
    pub item: bool,
}

/// An attack row as `pickAttack` answers it.
fn picked(w: &World, a: &AttackEntry, item: bool) -> PickedAttack {
    PickedAttack {
        montage: data_name(w, &a.attack_montage),
        allow_pre_attack_rotation: a.allow_pre_attack_rotation,
        // (an empty slot is skipped by condsOk: it is left out)
        conditions: Some(a.conditions.iter().flatten().map(|c| data_name(w, c)).collect()),
        ability_slot: a.ability_slot,
        item,
    }
}

/// The first row of `table` with the button `btn` and the direction `d` whose `rowOk` passes.
fn first_row(w: &mut World, f: FighterId, table: &[AttackEntry], btn: i32, d: Dir) -> Option<AttackEntry> {
    for t in table {
        if t.required_button_input != btn || t.required_direction_input != dir_num(d) {
            continue;
        }
        let r = RowOkR {
            conditions: Some(t.conditions.iter().flatten().map(|c| data_name(w, c)).collect()),
            ability_slot: t.ability_slot,
        };
        if row_ok(w, f, &r) {
            return Some(t.clone());
        }
    }
    None
}

/// `pickAttack`: BranchManagerGetAttack: CheckForOverride (the item pickup), else the first
/// matching entry in table order (Rage / perk entries fail their conditions). `raw`: the press's unmodified direction
/// (the default is `raw_press_dir`); `rows`: the table BFGetAttack was handed (BPI_ForceAttack's one row,
/// `force_attack`), else the pawn's for the button and GetIsInAir.
pub fn pick_attack(
    w: &mut World,
    f: FighterId,
    b: Btn,
    d: Dir,
    raw: Dir,
    rows: Option<&[AttackEntry]>,
) -> Option<PickedAttack> {
    if let Some(ov) = item_override(w, f, b, raw) {
        return Some(PickedAttack {
            montage: ov,
            allow_pre_attack_rotation: true,
            conditions: None,
            ability_slot: None,
            item: true,
        });
    }
    // Comp_AbilityOverrideManager's other packs (`fighter::anim` overrides). BFGetAttack @126 and
    // DoDelayedSpammedAttackLogic @147 pass CanBranchToSameAtk true and no excludes; a row that the pack names and that
    // fails the second match is no attack (the table is not read).
    if w.fighter(f).branching.override_packs.is_some() {
        match override_attack(w, f, Some(b), raw, None, true, None) {
            Override::Row(a) => return Some(picked(w, &a, a.required_button_input == 5)),
            Override::Refused => return None,
            Override::None => {}
        }
    }
    // A handed table is matched on any button, the Item one (5) too (BPI_ForceAttack's row); the pawn's tables only
    // answer the attack and special buttons.
    let btn = match b {
        Btn::Attack => 0,
        Btn::Special => 1,
        Btn::Item if rows.is_some() => 5,
        _ => return None,
    };
    if btn == 5 {
        let r = first_row(w, f, rows.expect("a handed table"), 5, d)?;
        return Some(picked(w, &r, true));
    }
    let r = match rows {
        Some(rows) => first_row(w, f, rows, btn, d),
        None => {
            let key = match (b == Btn::Attack, in_air(w, f)) {
                (true, false) => "AttacksNormalGround",
                (true, true) => "AttacksNormalAir",
                (false, false) => "AttacksSpecialGround",
                (false, true) => "AttacksSpecialAir",
            };
            let data = w.data.clone();
            let table = pawn_table(&data, w, f, key);
            first_row(w, f, &table, btn, d)
        }
    }?;
    Some(picked(w, &r, false))
}

/// `airSpecialCue`: Comp_Pawn_BranchManager InputTriggered_Event @2801-4975 (every special
/// ButtonPress of a locally controlled fighter, with or without a montage): ui_ability_cooldown_alt while
/// AirSpecialNumber is not under AirSpecialMax. The AirAttackCoolDown flag (AirAttackAvailable?) gates no attack: only
/// the AnimBPs read it.
pub fn air_special_cue(w: &mut World, f: FighterId) {
    if crate::real(w.fighter(f).air.air_specials) >= air_special_max(w, f) {
        sfx(w, f, Some(crate::name!("ui_ability_cooldown_alt")));
    }
}

// ---- the call surface of evade: the dodge, invulnerability and the evade meter

/// `evadeForce`: EvadeButtonPressed(Direction): Evade passes 5 (Default), Neutral Evade 4.
/// Every evade path (EvadeFromBranch, the air and ground evades, DoPreAttackRotation, Evade Server) takes Direction ==
/// 4 ? 4 : Get8WayDirection(), so only a neutral press overrides the stick, and a buffered evade keeps it (ButtonPress
/// stores it). A pawn that overrides EvadeButtonPressed and calls the parent with another Direction
/// (`CharHooks::evade_direction`: Pawn_C015 passes 5) gives the parent that one.
pub fn evade_force(w: &mut World, f: FighterId) -> Option<ForcedPress> {
    let pressed = dir_num(if w.fighter(f).controls.inp.neutral_evade { Dir::Neutral } else { Dir::Default });
    let over = w.fighter(f).hooks.char.evade_direction;
    let d = match over {
        Some(h) => h(w, f, pressed),
        None => pressed,
    };
    (d == dir_num(Dir::Neutral)).then_some(ForcedPress { dir: dir_num(Dir::Neutral) })
}

/// A direction's EActionDirection byte.
pub fn dir_num(d: Dir) -> i32 {
    i32::from(d as u8)
}

/// The direction of an EActionDirection byte (`None`: no direction of the enum).
pub fn dir_of(n: i32) -> Option<Dir> {
    Some(match n {
        0 => Dir::Up,
        1 => Dir::Down,
        2 => Dir::Fwd,
        3 => Dir::Back,
        4 => Dir::Neutral,
        5 => Dir::Default,
        6 => Dir::FwdUp,
        7 => Dir::FwdDown,
        8 => Dir::BackUp,
        9 => Dir::BackDown,
        _ => return None,
    })
}

/// `enableInvulnerability`: EnableInvulnerability / Enable Invulnerability Local (Enable,
/// IsFlashingEvadeBlink): AddInvulnerability or RemoveInvulnerability, IsFlashingEvadeBlink, DodgedSuccessfully =
/// false, then UpdateEvadeBlinkParameter.
pub fn enable_invulnerability(w: &mut World, f: FighterId, on: bool, flashing: bool) {
    let me = w.fighter_mut(f);
    let n = me.invuln.invuln_count;
    me.invuln.invuln_count = if on { n + 1 } else { (n - 1).max(0) };
    me.evade.dodged_successfully = false;
    update_evade_blink(w, f, flashing);
}

/// `updateEvadeBlink`: UpdateEvadeBlinkParameter: EvadeBlink = IsInvulnerable or
/// IsEvadeBlinkEnabled (never written) ? 1 : 0, UseEvadeSine = IsFlashingEvadeBlink ? 1 : -1, EvadeFresnel =
/// EvadeBlinkFresnelIntensity.
pub fn update_evade_blink(w: &mut World, f: FighterId, flashing: bool) {
    let on = if w.fighter(f).invuln.invuln_count > 0 { 1.0 } else { 0.0 };
    mat_scalar(w, f, crate::name!("EvadeBlink"), on);
    mat_scalar(w, f, crate::name!("UseEvadeSine"), if flashing { 1.0 } else { -1.0 });
    mat_scalar(w, f, crate::name!("EvadeFresnel"), EVADE_BLINK_FRESNEL);
}

/// `matScalar`.
pub fn mat_scalar(w: &mut World, f: FighterId, name: Name, value: Real) {
    emit(w, f, |fighter| Event::MatScalar { fighter, name, value });
}

/// `endInvulnLocal`: CancelInvuln's end (ubergraph 138135): Enable Invulnerability Local(false,
/// true), then EvadeFresnel 0.5 and UseEvadeSine 0.
pub fn end_invuln_local(w: &mut World, f: FighterId) {
    enable_invulnerability(w, f, false, true);
    mat_scalar(w, f, crate::name!("EvadeFresnel"), 0.5);
    mat_scalar(w, f, crate::name!("UseEvadeSine"), 0.0);
}

/// `cancelInvuln`: CancelInvuln(IsJump): with HasSpawnInvulnerability, a jump keeps it and
/// anything else ends it (the pending Delay still resumes here later). Otherwise, unless DodgedSuccessfully, an
/// invulnerable fighter with the EvadeInvulnerableTimer running loses the evade window.
pub fn cancel_invuln(w: &mut World, f: FighterId, is_jump: bool) {
    let me = w.fighter_mut(f);
    if me.invuln.has_spawn_invuln {
        if is_jump {
            return;
        }
        me.invuln.has_spawn_invuln = false;
        // DestroySpawnVFX
        emit(w, f, |fighter| Event::SpawnFx { fighter, on: false });
        end_invuln_local(w, f);
        return;
    }
    if me.evade.dodged_successfully || me.invuln.invuln_count <= 0 || me.evade.evade_invuln <= 0.0 {
        return;
    }
    me.evade.evade_invuln = 0.0;
    end_invuln_local(w, f);
}

/// The navigation set the fighter plays (`f.nav`).
pub fn nav_set<'a>(data: &'a GameData, w: &World, f: FighterId) -> &'a NavSet {
    let def = &char_data_in(data, w, f).def;
    match w.fighter(f).anim.nav {
        SetRef::Base => &def.nav,
        SetRef::Named(n) => def.nav_sets.get(w.names.str(n)).unwrap_or(&def.nav),
    }
}

/// An attack table entry's conditions as `condsOk` takes them (an empty slot `None`).
fn entry_conds(w: &World, e: &AttackEntry) -> Vec<Option<Name>> {
    e.conditions.iter().map(|c| c.as_deref().map(|c| data_name(w, c))).collect()
}

/// The first entry of `list` for direction `d` whose conditions hold (`list.find`), cloned.
fn find_entry(w: &mut World, f: FighterId, list: &[AttackEntry], d: Dir) -> Option<AttackEntry> {
    for e in list {
        if e.required_direction_input != dir_num(d) {
            continue;
        }
        let conds = entry_conds(w, e);
        if conds_ok(w, f, Some(&conds), false) {
            return Some(e.clone());
        }
    }
    None
}

/// BFGetAttack(Evades / EvadesGroundToAir, button 3, the direction) (BaseFighter @39603, @44555, @48049):
/// BranchManagerGetAttack @126 asks Comp_AbilityOverrideManager.CheckForOverride first (@1658-1900, CanBranchToSameAtk
/// true, no excludes), as `pick_attack` does for an attack; a row a pack names and that fails the second match is no
/// entry (the table is not read). Else the table's first entry for the direction (`find_entry`).
// Unverified: no pack of the build's data has an evade (button 3) row (Arya's stolen tables answer the attack buttons,
// HeartBuff's HeartBeamOverride the special, Tom and Jerry's NoJerryTauntOverride the taunt, the hold pack the item
// and the attack), so no match runs the override side.
fn evade_entry(w: &mut World, f: FighterId, list: &[AttackEntry], d: Dir) -> Option<AttackEntry> {
    match override_attack(w, f, Some(Btn::Dodge), d, None, true, None) {
        Override::Row(a) => Some(a),
        Override::Refused => None,
        Override::None => find_entry(w, f, list, d),
    }
}

/// The pawn's EvadeGroundToAir? (a class default only Pawn_BatmanV2 and Pawn_SuperMan set).
fn evade_ground_to_air(w: &World, f: FighterId) -> bool {
    char_data(w, f).fighter.pawn.get("EvadeGroundToAir?").is_some_and(js_truthy)
}

/// What the evade's tutorial events name it (`evadeTutorial`'s kind).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EvadeKind {
    Drop,
    Air,
    Gta,
    Ground,
}

/// `tryDodge`. neutral: Direction 4 (Neutral Evade), which replaces the stick's direction
/// (the turn to MoveX before the evade, SetRotation at ubergraph 116695, still happens).
/// The evade's own turn: DoPreAttackRotation(Direction == 4 ? 4 : Get8WayDirection, false, StickAttackX) after the
/// entry is looked up, on the air path (@39914), the platform drop (@44866) and the ground-to-air path (@48360); the
/// plain ground evade has none. It reads no AllowPreAttackRotation. By then MoveX has turned him already, so it turns
/// him only with MoveX 0: an Up / Down direction to the right stick past StickAttackXDeadzone (`preAttackRotation`).
pub fn try_dodge(w: &mut World, f: FighterId, neutral: bool) -> bool {
    // EvadeFromBranch @116642-116729, out of hitstun: a MoveX that is not 0 turns him to it (SetRotation, which writes
    // FacingLeft at once in a standalone game, @61066 -> @138216), ahead of the isEvading test @114740, the EndJump
    // and the ground / air split @39171, whatever the path below does next. Every Get8WayDirection below reads the
    // turned facing, so a stick to his back is Forward there: the tables' Back, BackUp and BackDown rows are never
    // picked (Pawn_SuperMan's EvadesGroundToAir BackUp row, GroundAir_U, is left for GroundAir_FU).
    // BPI_EvadeFromBranch's DoPreAttackRotation(Direction, false, 0) @89 on the branch path is the same turn
    // (`executeBranch`).
    let x = w.fighter(f).controls.inp.x;
    if x != 0.0 {
        set_facing(w, f, if x < 0.0 { -1.0 } else { 1.0 });
    }
    // Evade press / EvadeFromBranch: EndJump(0, false, false) when not already evading, before the air evade count
    // is checked. The ground / air choice (39171) is GetIsInAir after that EndJump: a ground jump still scheduled is
    // dropped and the evade is the ground one; once the jump is issued, the jump stop launch is pending and the evade
    // is the air one, started on the ground (memory captures of the build).
    if !w.fighter(f).evade.evading {
        end_jump_at_start(w, f);
    }
    let on_ground = !in_air(w, f);
    // GroundEvadeAvailable? false (`noGroundEvade`): the ground path pops at @43669, ahead of its StopAnimMontage.
    // Unverified: taken as no evade and the press's other handling as the port has it for a refused evade;
    // BPI_IsEvadeAvailable's reader (Comp_Pawn_HitstunHandler) does not read it in the port.
    if on_ground && w.fighter(f).evade.no_ground_evade {
        return false;
    }
    // The ground evade's StopAnimMontage(null) (BaseFighter @43679) with an instance of another slot group as the
    // active montage: that one stops with its BlendOut, the DefaultGroup montage is left to the play.
    let g_stop = on_ground && stop_active_grouped(w, f);
    let (ix, iy, facing) = {
        let me = w.fighter(f);
        (me.controls.inp.x, me.controls.inp.y, me.body.facing_dir)
    };
    let d = if neutral { Dir::Neutral } else { get8_way(ix, iy, facing) };
    if w.fighter(f).evade.block_evade_montage {
        return blocked_evade(w, f, on_ground, g_stop, d, neutral);
    }
    // Evades, FWD_Evade and SpotDodgeAnim are read here, at the evade's start (NavSet).
    let data = w.data.clone();
    let nav = nav_set(&data, w, f);
    let ev: &[AttackEntry] = nav.evades.as_deref().unwrap_or(&[]);
    let mut name: Option<Name> = None;
    // The ground evade's drop (@43772-44146, then 44160): the direction (Direction == 4 ? 4 : Get8WayDirection, so
    // never for the neutral evade's Direction 4, whatever the stick says) is Down (1), ForwardDown (7) or BackDown (9)
    // and the movement base's owner is a BasePlatformBP, or a SpawnedPlatformParent whose CanPassThrough(TeamIndex) is
    // true (@47620-47873; dropBase): EvadingThroughPlatform, HandlePlatformCollision, EvadeGate off,
    // AirAttackAvailable, and the montage of the air evade set.
    // Unverified: the air evade count is taken as untouched.
    let mut drop = false;
    if on_ground
        && matches!(d, Dir::Down | Dir::FwdDown | Dir::BackDown)
        && drop_base(w, f)
        && let Some(e) = evade_entry(w, f, ev, d)
    {
        let rsx = w.fighter(f).controls.inp.rsx.unwrap_or(0.0);
        // @44866
        pre_attack_rotation(w, f, None, d, ix, rsx);
        name = Some(data_name(w, &e.attack_montage));
        w.fighter_mut(f).air.air_attack_available = true;
        drop = true;
    }
    let mut to_air = false;
    if drop {
        // the montage is the air set's, chosen above
    } else if on_ground && evade_ground_to_air(w, f) {
        // BaseFighter @47878 (a pawn with EvadeGroundToAir?, a class default only Pawn_BatmanV2 and Pawn_SuperMan set;
        // the pawn data is the whole class default object, so theirs carries it and the EvadesGroundToAir table): in
        // place of the plain ground evade, the air evade's steps over that table without its count:
        // BFGetAttack(EvadesGroundToAir, no conditions array, button 3, Direction == 4 ? 4 : Get8WayDirection) @48049,
        // BFCurrentAttack and AddRecentAttackToQueue with the entry @48215-48242, DoPreAttackRotation(the direction,
        // false, StickAttackX) @48360, AirControlGate / DashFallGate / CheckLateralFriction @48437-48459, the
        // constant-false BeginAttack @48473, Evade Server(montage, direction, EvadingThroughPlatform) @49891 and the
        // play at GetEvadeAnimMontagePlayRate @50749-50772.
        // No entry for the direction (@48201 -> @51242: Evade Server without a montage, then isEvading = false) is not
        // ported and warns. BlockEvadeMontage (@47921 -> @51264) is blockedEvade.
        let table = pawn_table(&data, w, f, "EvadesGroundToAir");
        let Some(e) = evade_entry(w, f, &table, d) else {
            let ch = w.names.str(w.fighter(f).ch);
            let msg = format!(
                "EvadesGroundToAir: no entry for direction {} ({ch}): the evade without a montage is not ported",
                dir_num(d)
            );
            warn_text(w, &msg);
            return false;
        };
        let rsx = w.fighter(f).controls.inp.rsx.unwrap_or(0.0);
        // @48360
        pre_attack_rotation(w, f, None, d, ix, rsx);
        name = Some(data_name(w, &e.attack_montage));
        to_air = true;
    } else if on_ground {
        // The plain ground evade (BaseFighter @51390-53112): SpotDodgeAnim when the direction is Neutral, Up or Down and
        // not DoingEscape, FWD_Evade otherwise (the up diagonals too).
        // The direction is Get8WayDirection's sector: a stick more up or down than 67.5 degrees off the side is Up /
        // Down, the spot dodge, whatever its X. He has turned to MoveX above (@116642-116729).
        let s = if matches!(d, Dir::Neutral | Dir::Up | Dir::Down) { &nav.spot_dodge } else { &nav.fwd_evade };
        name = s.as_deref().filter(|s| !s.is_empty()).map(|s| data_name(w, s));
    } else {
        if crate::real(w.fighter(f).air.air_evades) >= AIR_EVADE_MAX {
            return false;
        }
        // the air evade's StopAnimMontage(null), BaseFighter @39420
        stop_active_grouped(w, f);
        let Some(e) = evade_entry(w, f, ev, d) else { return false };
        let rsx = w.fighter(f).controls.inp.rsx.unwrap_or(0.0);
        // @39914
        pre_attack_rotation(w, f, None, d, ix, rsx);
        name = Some(data_name(w, &e.attack_montage));
        let air = &mut w.fighter_mut(f).air;
        air.air_evades += 1;
        air.air_attack_available = true;
    }
    let Some(name) = name.filter(|n| !n.is_none()) else { return false };
    add_recent_attack(w, f, Btn::Dodge, if on_ground && !drop && !to_air { None } else { Some(name) });
    // Evade Server runs before the local UpdateEvadeValues (the multicast skips it when locally controlled), so this
    // evade gets the window computed by the previous one. A new window replaces a running one. The bot's pawn is
    // locally controlled too (AController::IsLocalController 1429e48e0, standalone).
    // Evade Server: EnableInvulnerability(!BlockEvadeInvulnerability, never set for Shaggy); a running
    // EvadeInvulnerableTimer is cleared with EnableInvulnerability(false); then the timer, or with no time
    // EnableInvulnerability(false).
    // Set in the input event, so pending until this frame's timer tick (timerFrames): 0.225 s protects 15 frames.
    evade_server(w, f, crate::real(dir_num(d)));
    let me = w.fighter_mut(f);
    me.evade.last_evade_time = me.clock.time;
    let kind = if drop {
        EvadeKind::Drop
    } else if !on_ground {
        EvadeKind::Air
    } else if to_air {
        EvadeKind::Gta
    } else {
        EvadeKind::Ground
    };
    evade_tutorial(w, f, kind, neutral);
    update_evade_values(w, f);
    evade_sounds(w, f);
    // @42463
    tutorial_event_of(w, f, TutorialEvent::Evade);
    if !on_ground {
        // the pushed @43472: AirEvadeNumber + 1, HandleAirMoveDesaturation @43591
        handle_air_move_desaturation(w, f);
    }
    // The evade plays' proxies (BFA2269F ground-to-air, D7E07901 plain ground, 433D65E4 air, 2AA56D73 the drop) clear
    // BeginAttack and isEvading at their blend-out and interrupt (proxyClear). The ground evade
    // stops the montage (@43679) before isEvading = true (@43705), the air evade after it (@39314, @39420): the
    // replaced instance's handlers run after this evade's isEvading there.
    let mut owed: Vec<MontageProxy> = Vec::new();
    let proxy: MontageProxy = if drop { PROXY_EVADE_DROP } else { PROXY_EVADE };
    start_montage(w, f, name, &proxy, if on_ground { None } else { Some(&mut owed) });
    // PlayAnimMontage(montage, GetEvadeAnimMontagePlayRate): AnimMontagePlayRate (1) x
    // EvadeAnimMontagePlayRateModifier (the evade fold); the notifies follow the montage clock at that
    // rate.
    let rate = evade_play_rate(w, f);
    let me = w.fighter_mut(f);
    if rate != 1.0
        && let Some(m) = &mut me.montages.montage
    {
        m.play_rate = f64::from(rate);
    }
    // BaseFighter isEvading
    me.evade.evading = me.montages.montage.is_some();
    if drop && me.evade.evading {
        me.platforms.evading_through_platform = true;
        let vy = me.body.vy;
        handle_platform_collision(w, f, vy);
    }
    for o in owed {
        proxy_clear(w, f, o);
    }
    // Every evade start guards its BeginAttack = true with a constant false branch (40027, 44979, 48473, 51469), so an
    // evade counts as attacking only for its montage's isAttackingFlag (Batman_Nav_EvadeN_Montage 0-0.134 s,
    // Shaggy_Ground_Evade_N_Montage 0-0.152 s); after it the stick moves and turns the fighter (memory: air control from
    // 9 f into a down air dodge).
    w.fighter_mut(f).charge.begin_attack = false;
    lateral_friction_event(w, f, true, false);
    true
}

/// SFX_Evade: the container's SFX_Evade (class default) and, unless Comp_Actor_VO GetIsSpeaking (false for every
/// pawn offline: Ak_VO_Effort_C), VO_Evade.
fn evade_sounds(w: &mut World, f: FighterId) {
    sfx(w, f, Some(crate::name!("sfx_shared_combat_evade")));
    let vo = char_data(w, f).def.sfx.vo_evade.as_deref().filter(|s| !s.is_empty()).map(|s| data_name(w, s));
    sfx(w, f, vo);
}

/// An attack table of the pawn's class default (`f.data.pawn[key]`), read from the data (empty, with a warning,
/// without one or when it does not read as attack rows).
pub fn pawn_table(data: &GameData, w: &World, f: FighterId, key: &str) -> Vec<AttackEntry> {
    let Some(v) = char_data_in(data, w, f).fighter.pawn.get(key) else {
        let msg = format!("pawn table {key} of {} is not in the data", w.names.str(w.fighter(f).ch));
        warn_text(w, &msg);
        return Vec::new();
    };
    match mvs_data::value::from_value::<Vec<AttackEntry>>(v.clone()) {
        Ok(t) => t,
        Err(e) => {
            let msg = format!("pawn table {key} of {} does not read as attack rows: {e}", w.names.str(w.fighter(f).ch));
            warn_text(w, &msg);
            Vec::new()
        }
    }
}

/// `evadeTutorial`: the evade's tutorial events behind Evade Server, ahead of
/// UpdateEvadeValues: the platform drop's EvadeThroughPlatform (@46596), which goes on to the air evade's; the air
/// evade's direction byte (@41988) and EvadeAir (@42300); the ground to air evade's byte (@50429); the plain ground
/// evade's (@54837). The byte's index: Get8WayDirectionNoFacing, or 4 for the neutral evade's Direction 4
/// (@41844-41894).
fn evade_tutorial(w: &mut World, f: FighterId, kind: EvadeKind, neutral: bool) {
    if !tutorial_world(w, f) {
        return;
    }
    let inp = &w.fighter(f).controls.inp;
    let d = crate::real(dir_num(if neutral { Dir::Neutral } else { get8_way_no_facing(inp.x, inp.y) }));
    match kind {
        EvadeKind::Gta => return tutorial_event_of(w, f, evade_tutorial_byte(EvadeTutorialByteKind::Gta, d)),
        EvadeKind::Ground => return tutorial_event_of(w, f, evade_tutorial_byte(EvadeTutorialByteKind::Ground, d)),
        EvadeKind::Drop => tutorial_event_of(w, f, TutorialEvent::EvadeThroughPlatform),
        EvadeKind::Air => {}
    }
    tutorial_event_of(w, f, evade_tutorial_byte(EvadeTutorialByteKind::Air, d));
    tutorial_event_of(w, f, TutorialEvent::EvadeAir);
}

/// `evadeServer`: Evade Server's invulnerability (see tryDodge) and its Evade Multicast's
/// broadcast EvadeTriggered(Direction, SelectFloat(0, EvadeInvulnerableTime, BlockEvadeInvulnerability)) (BaseFighter
/// @149437; the duration is the invulnerable time: BlockEvadeInvulnerability has no writer the port reaches). Its one
/// listener is Buff_PolymorphParent.
/// Deviation: Evade Server's refusal (@148604: hitstun, dead, a failed branch validation) is not modelled here or in
/// tryDodge.
fn evade_server(w: &mut World, f: FighterId, dir: Real) {
    enable_invulnerability(w, f, true, false);
    if w.fighter(f).evade.evade_invuln > 0.0 {
        w.fighter_mut(f).evade.evade_invuln = 0.0;
        enable_invulnerability(w, f, false, false);
    }
    let t = w.fighter(f).evade.evade_invuln_time;
    if t > 0.0 {
        w.fighter_mut(f).evade.evade_invuln = timer_frames(Some(t));
    } else {
        enable_invulnerability(w, f, false, false);
    }
    evade_triggered(w, f, dir);
}

/// `evadeTriggered`.
pub fn evade_triggered(w: &mut World, f: FighterId, dir: Real) {
    let me = w.fighter(f);
    if me.buffs.buffs.is_empty() {
        return;
    }
    let duration = me.evade.evade_invuln_time;
    let arg = BuffSignalArg { dir: Some(dir), duration: Some(duration), ..BuffSignalArg::default() };
    buff_signal(w, f, BuffSignal::EvadeTriggered, arg);
}

/// `dropBase`: the ground evade's platform drop test (BaseFighter @44160-44309, else
/// @47620-47873): GetMovementBase's owner a BasePlatformBP (a stage platform), or a SpawnedPlatformParent whose
/// CanPassThrough(Comp_Actor_Destructible.TeamIndex) is true; any other base takes the plain evade (@47878). The jump
/// press's drop has the same test.
pub fn drop_base(w: &World, f: FighterId) -> bool {
    let me = w.fighter(f);
    match me.platforms.base {
        Some(PlatformRef::Stage(_)) => true,
        Some(b) => platform_can_pass_through(w, b, me.team.team),
        None => false,
    }
}

/// `blockedEvade`: BlockEvadeMontage (BaseFighter; written true by Buff_PolymorphParent @4536
/// alone, false at its EndPlay): the evade's own steps run and its montage does not play; the buff's EvadeTriggered
/// handler plays the creature's.
/// - Air (@39171-39475): AirEvadeNumber < AirEvadeMax, isEvading @39314, LastEvadeTime, AirAttackAvailable @39381,
///   StopAnimMontage(null) @39420, then the test @39475 -> @43346 BPI_IncrementPlayerAttackCount -> @41372 Evade Server,
///   and the pushed @43472 AirEvadeTime = 10, AirEvadeNumber + 1. No Evades lookup (BFGetAttack @39603 is on the
///   unblocked side): no entry needed, no pre-attack turn, nothing added to the recent attacks.
/// - Ground (@43669-43745): StopAnimMontage(null) @43679, isEvading @43705, LastEvadeTime. The platform drop
///   (@44160-44370: EvadingThroughPlatform, HandlePlatformCollision, AirAttackAvailable) and then @44427 -> @47494 ->
///   @46324 Evade Server; EvadeGroundToAir? @47921 -> @51264 -> @49818 Evade Server; the plain one @51419 -> @56481 ->
///   @53804 Evade Server (AirControlGate, DashFallGate, CheckLateralFriction @51433-51455 skipped).
///
/// Each Evade Server gets the montage of a ubergraph local the blocked path did not write (used only when the server
/// refuses the evade, which the port does not model). SFX_Evade and UpdateEvadeValues follow Evade Server as on the
/// unblocked path. The turn to MoveX (@116642-116729) is tryDodge's, ahead of this, on every path. The drop's base
/// test is the unblocked one's (dropBase: the same @44160-44309 / @47620-47873).
/// Unverified: the blocked drop runs without the Evades entry the port's unblocked drop needs.
fn blocked_evade(w: &mut World, f: FighterId, on_ground: bool, g_stop: bool, d: Dir, neutral: bool) -> bool {
    if !on_ground && crate::real(w.fighter(f).air.air_evades) >= AIR_EVADE_MAX {
        return false;
    }
    let me = w.fighter_mut(f);
    me.evade.evading = true;
    me.evade.last_evade_time = me.clock.time;
    let drop = on_ground && matches!(d, Dir::Down | Dir::FwdDown | Dir::BackDown) && drop_base(w, f);
    if !on_ground {
        w.fighter_mut(f).air.air_attack_available = true;
    }
    if !g_stop {
        stop_anim_montage(w, f);
    }
    if drop {
        let me = w.fighter_mut(f);
        me.platforms.evading_through_platform = true;
        let vy = me.body.vy;
        handle_platform_collision(w, f, vy);
        w.fighter_mut(f).air.air_attack_available = true;
    }
    evade_server(w, f, crate::real(dir_num(d)));
    let kind = if drop {
        EvadeKind::Drop
    } else if !on_ground {
        EvadeKind::Air
    } else if evade_ground_to_air(w, f) {
        EvadeKind::Gta
    } else {
        EvadeKind::Ground
    };
    evade_tutorial(w, f, kind, neutral);
    update_evade_values(w, f);
    evade_sounds(w, f);
    // @42463
    tutorial_event_of(w, f, TutorialEvent::Evade);
    if !on_ground {
        w.fighter_mut(f).air.air_evades += 1;
        // HandleAirMoveDesaturation @43591
        handle_air_move_desaturation(w, f);
    }
    w.fighter_mut(f).charge.begin_attack = false;
    true
}

/// `enableInvulnerabilityRaw`: AddInvulnerability alone (SpawnInvulnerability).
pub fn enable_invulnerability_raw(w: &mut World, f: FighterId) {
    w.fighter_mut(f).invuln.invuln_count += 1;
}

/// `clearInvuln`: match start: no invulnerability or pending spawn Delays. TrainGameMode's
/// spawn calls EnableInvulnerability(false, false) on the new pawn (count already 0), which runs the blink update.
pub fn clear_invuln(w: &mut World, f: FighterId) {
    let me = w.fighter_mut(f);
    me.invuln.invuln = 0.0;
    me.invuln.invuln_count = 0;
    me.invuln.has_spawn_invuln = false;
    me.evade.evade_invuln = 0.0;
    update_evade_blink(w, f, false);
}

/// `updateEvadeValues`: UpdateEvadeValues (also IncrementEvadeCount).
pub fn update_evade_values(w: &mut World, f: FighterId) {
    let e = &mut w.fighter_mut(f).evade;
    if e.evade_count < MAX_EVADE_COUNT {
        e.last_evade_spent_meter = true;
        e.evade_count = js::min(e.evade_count + EVADE_METER_COST, MAX_EVADE_COUNT);
    } else {
        e.last_evade_spent_meter = false;
        // @612
        tutorial_event_of(w, f, TutorialEvent::DodgeMeterEmpty);
    }
    let r = w.fighter(f).evade.evade_count / MAX_EVADE_COUNT;
    let base = evade_base_invuln(w, f);
    w.fighter_mut(f).evade.evade_invuln_time = base * (1.0 - r * r);
}

/// `replenishEvade`: ReplenishDodgeMeter (BPI_DecrementEvadeCount, DecrementEvadeCount).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison `!(count > 0)`, which a NaN count passes")]
pub fn replenish_evade(w: &mut World, f: FighterId, amount: Real) {
    let e = &mut w.fighter_mut(f).evade;
    if !(e.evade_count > 0.0) {
        return;
    }
    e.evade_count = js::max(e.evade_count - amount, 0.0);
    if e.evade_count == 0.0 {
        // @311
        tutorial_event_of(w, f, TutorialEvent::DodgeMeterFull);
    }
}

/// `tickEvadeRegen`: HandleEvadeUse runs every tick: the first time EvadeCount >= 1 a DoOnce
/// (never reset) restarts the looping timer that calls DecrementEvadeCount every EvadeInvulnerabilityRechargeTime.
/// World timers keep running through hitpause and while dead.
pub fn tick_evade_regen(w: &mut World, f: FighterId) {
    let me = w.fighter_mut(f);
    let tick = crate::real(me.clock.tick);
    // The DoOnce's K2_SetTimerDelegate (68693) replaces a timer ResetEvadeRegenTimer started before it.
    if !me.evade.evade_regen_once && me.evade.evade_count >= 1.0 {
        me.evade.evade_regen_once = true;
        me.evade.evade_regen_start = tick;
        return;
    }
    if me.evade.evade_regen_start < 0.0 {
        return;
    }
    let start = me.evade.evade_regen_start;
    // EvadeInvulnerabilityRechargeTime
    let t = evade_recharge(w, f);
    let period = js::round(t * 60.0);
    let n = tick - start;
    if n > 0.0 && n % period == 0.0 {
        replenish_evade(w, f, EVADE_RECHARGE_AMOUNT);
    }
}

/// `crouchEvadeRate`: Pawn_C019 K2_OnStartCrouch (1434-1457):
/// EvadeInvulnerabilityRechargeTime = 0.5, ResetEvadeRegenTimer; K2_OnEndCrouch (1476-1503): the value saved at
/// BeginPlay (1227), ResetEvadeRegenTimer. ACharacter calls them from the CMC's Crouch / UnCrouch.
/// ResetEvadeRegenTimer (159394 -> 68670) sets the looping timer again at the new rate past the DoOnce, whatever the
/// meter holds. Set in the CMC tick, it is pending for this frame, so its first period counts from the next frame
/// (tickEvadeRegen runs the timer tick at the next beginFrame).
pub fn crouch_evade_rate(w: &mut World, f: FighterId, on: bool) {
    w.fighter_mut(f).evade.evade_crouch_rate = on;
    // The write replaces what the evade fold wrote : 0.5, or the unbuffed BeginPlay value at the end.
    clear_evade_recharge_fold(w, f);
    let me = w.fighter_mut(f);
    me.evade.evade_regen_start = crate::real(me.clock.tick + 1);
}

// ---- the dodged attack (Comp_Pawn_HitBoxHandler pfgDoDamage on an invulnerable victim)
//
// pfgDoDamage @410-782, at the entry of every call for a fighter HitActor: with an entry of the handler's
// DodgingFighters map for it (HitActor -> the AttackID it dodged), a call of the same AttackID on a victim still
// invulnerable stops there @772; any other call removes the entry @782 and goes on.
// Then, behind IgnoreActor @2826, the hit record @2906 and HitOnlyOneFighter @3236 and ahead of the owner's hitstun
// test @4922, an invulnerable victim @4608-4682 -> @7381: CanBeHitBy(the owner's TeamIndex) @7452-7528 (another team,
// not DamageDisabled: the caller's test), then (the owner's BPI_CanMarkFighter, false when the owner is no BPI_Marker
// @8356, or LocalHitInformation.BaseKnockBack > 0) and the victim's TeamIndex != -23 (@7623-7858; no decoded class
// sets that team). BaseKnockBack is the hit box's own there: ahead of the dealt mods @5822 and the vacuum @13721. Then
// Map_Add(DodgingFighters, victim, AttackID) @7868 and the victim's BPI_SetDodgedSuccessfully(true, the handler's
// owner) @8035 -> BaseFighter @108271: DodgedSuccessfully = true (@176451), broadcast EvadedAttack(AttackingActor)
// @108294 (CharHooks.evadedAttack) and AfterEvadedAttack @108322.
// The handler's ReceiveTick (ubergraph @8263-8396 -> @4921-5267) removes every entry whose fighter overlaps none of
// HitBoxesToDamage (CheckDodgingFighterOverlap): dodgeTick below, once a frame per host behind the frame's overlap
// tests. The world tests an invulnerable fighter under a hit box at every overlap update of every frame (the melee,
// projectile and item tests), so an entry not confirmed in a frame (`at`) has seen a handler tick without the overlap.
// (attackDodged's own `!over` test is the same rule for a host no pass runs on.)
// Unverified: where the handler's tick falls among the frame's overlap updates is not read; an overlap that ends and
// begins again inside two frames is taken as kept. The build makes a call per begin-overlap event; the port is called
// per update while the overlap lasts and takes the first update of an AttackID as that event (`seen`).
// Then SpawnDodgeEffectRatelimited(HitActor) @8082 (spawnDodgeEffect) and BPI_ProcessTutorialEvent(DodgeSuccess)
// @8205. The handler's HitDodged broadcast @8264 is not ported: its one binder, BuffParentActor's OwnerHitDodged
// (@6033), calls BPI On Fighter Owner Hit Dodged, whose body is BuffParentActor's empty @3477 in every class (no child
// overrides it).
// The map is the handler's, so the fighter's for its own hit boxes and the spawned actor's for its own: an optional
// field of the host, made at its first dodged attack.
// Two hit boxes of one handler with different AttackIDs over one fighter: each box's begin-overlap is one call. The
// second finds the first's entry with another id, removes it @782 and adds its own @7868 (one more dodge); no call
// follows while both stay, so the map keeps the id of the box that came last. The world picks a box per update and can
// pick the other id from one update to the next: `seen` holds the frame each AttackID was last over the fighter, and
// only an id not seen in this frame or the last one is a begin-overlap (a call). Boxes that share an AttackID are one
// here: the second's call would return @410 all the same.

/// A dodge host's `dodge` field (a holder of a Comp_Pawn_HitBoxHandler); `None` for a reference to no such holder or
/// to one that is gone.
pub fn dodge_slot(w: &mut World, host: ActorRef) -> Option<&mut Option<DodgeState>> {
    match host {
        ActorRef::Fighter(f) => w.fighters.get_mut(f).map(|me| &mut me.evade.dodge),
        ActorRef::Projectile(p) => w.actors.projectiles.get_mut(p).map(|a| &mut a.dodge),
        ActorRef::Item(i) => w.actors.items.get_mut(i).map(|a| &mut a.dodge),
        ActorRef::Marker(m) => w.actors.markers.get_mut(m).map(|a| &mut a.dodge),
        ActorRef::Mortar(m) => w.actors.mortars.get_mut(m).map(|a| &mut a.dodge),
        ActorRef::Actor(a) => world_actor_dodge(w, a),
        ActorRef::Stage(s) => crate::stages::the_court::hoop::dodge_slot(w, s),
        ActorRef::Ship(_) | ActorRef::AimPunch(_) | ActorRef::Accelerator(_) => None,
        // The buff's class keeps its handler's dodge variables in its ext.
        ActorRef::Buff(b) => crate::combat::buffs::buff_mut(w, b).and_then(|e| e.ext.dodge_mut()),
    }
}

/// `dodgeOf`: the host's dodge variables, made at its first dodge or burst.
fn dodge_of(w: &mut World, host: ActorRef) -> Option<&mut DodgeState> {
    Some(dodge_slot(w, host)?.get_or_insert_with(DodgeState::default))
}

/// `host.useDelayedDodgeText`: the handler template's UseDelayedDodgeText, false only on HarleyJackInTheBox's and
/// HarleyBomb's (world actors); the class default true elsewhere.
fn uses_delayed_dodge_text(w: &World, host: ActorRef) -> bool {
    match host {
        ActorRef::Actor(a) => world_actor_delayed_dodge_text(w, a),
        _ => true,
    }
}

/// `attackDodged`: `host`'s hit box with `attack_id` is over the invulnerable enemy fighter
/// `vic` in this update. tick: the world's frame count. attacker: the handler's owner when it is a fighter. can_mark:
/// the owner's BPI_CanMarkFighter.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn attack_dodged(
    w: &mut World,
    host: ActorRef,
    vic: FighterId,
    attack_id: i32,
    base_knock_back: Real,
    tick: i32,
    attacker: Option<FighterId>,
    can_mark: bool,
) {
    // @7452-7528: CanBeHitBy(the owner's team) pops ahead of DodgingFighters @7868 and DodgeQuest_Update @8332 (no
    // dodge, no entry, so no DODGED text): DamageDisabled (HandleWinLoss) here, the team test at every caller.
    if w.fighter(vic).invuln.damage_disabled {
        return;
    }
    // The port's own spare switch: an opponent spared the hit has nothing to dodge (an item the stage dropped is
    // not spared: the host's own dealing fighter, before the attacker the caller names).
    let spared = if matches!(host, ActorRef::Item(_)) {
        crate::combat::phases::spared_from(w, host, vic)
    } else {
        attacker.is_some_and(|a| crate::combat::phases::spares(w, a, vic))
            || crate::combat::phases::spared_from(w, host, vic)
    };
    if spared {
        return;
    }
    let vid = w.fighter(vic).id;
    let Some(d) = dodge_slot(w, host) else { return };
    if !dodge_entry(d, vid, attack_id, tick, can_mark || base_knock_back > 0.0) {
        return;
    }
    dodge_tail(w, vic, attacker.map_or(host, ActorRef::Fighter), attacker);
    spawn_dodge_effect(w, host, vic, tick);
    // @8205
    tutorial_event_of(w, vic, TutorialEvent::DodgeSuccess);
    // pfgDoDamage @8332
    dodge_quest(w, host, vic, tick);
}

/// `attackDodged` with a dealer no list holds as the host (the perks'
/// BlastBoxExitKnockbackExplosion, checkForBlastBoxBounce, and Buff_C016_DistanceShot's explosion,
/// Lebron's perks), no attacker: its dodge record is the dealer's own, made new for each explosion and shared
/// by the explosion's loop. Its handler has UseDelayedDodgeText (the class default).
/// Deviation: the build hands the dealer actor itself to EvadedAttack and AfterEvadedAttack (AttackingActor, and
/// `actorBy` of it for the buffs); the dealer is in no list here, so the port's broadcasts carry no actor
/// (`dodge_tail_of`).
pub fn dealer_dodged(
    w: &mut World,
    d: &crate::combat::perks::PerkDealer,
    vic: FighterId,
    attack_id: i32,
    base_knock_back: Real,
    tick: i32,
) {
    if w.fighter(vic).invuln.damage_disabled || crate::combat::phases::spares(w, d.owner, vic) {
        return; // (and the port's own spare switch: an opponent spared the hit has nothing to dodge)
    }
    let vid = w.fighter(vic).id;
    if !dodge_entry(&mut d.dodge.borrow_mut(), vid, attack_id, tick, base_knock_back > 0.0) {
        return;
    }
    dodge_tail_of(w, vic, None, None);
    let fx = d.dodge.borrow_mut().as_mut().is_some_and(|s| fx_gate(s, tick));
    if fx {
        push_dodge_effect(w, vic, true);
    }
    // @8205
    tutorial_event_of(w, vic, TutorialEvent::DodgeSuccess);
    // pfgDoDamage @8332
    if !w.fighter(vic).life.is_minion && d.dodge.borrow_mut().as_mut().is_some_and(|s| quest_gate(s, tick)) {
        w.fighter_mut(vic).stats.attacks_dodged += 1;
    }
}

/// `dodgeOver` of a dealer no list holds (`dealer_dodged`).
pub fn dealer_dodge_over(w: &World, d: &crate::combat::perks::PerkDealer, vic: FighterId) {
    let vid = w.fighter(vic).id;
    if let Some(s) = d.dodge.borrow_mut().as_mut() {
        s.fighters.shift_remove(&vid);
    }
}

/// `attackDodged`'s record of the dodging fighter `vid` in the host's variables `d`; true: the dodge goes on to its
/// tail (`mark`: the caller's `canMark || baseKnockBack > 0`).
fn dodge_entry(d: &mut Option<DodgeState>, vid: Fid, attack_id: i32, tick: i32, mark: bool) -> bool {
    if let Some(e) = d.as_mut().and_then(|d| d.fighters.get_mut(&vid)) {
        let (mut over, mut had) = (false, false);
        e.seen.retain(|&id, &mut t| {
            if tick - t > 1 {
                return false;
            }
            over = true;
            if id == attack_id {
                had = true;
            }
            true
        });
        if !over {
            // the handler's tick found the fighter over no hit box
            e.id = None;
        }
        e.seen.insert(attack_id, tick);
        e.at = tick;
        if had {
            // the overlap goes on: no call
            return false;
        }
        if e.id == Some(attack_id) {
            // @410: the same AttackID
            return false;
        }
        // @782
        e.id = None;
    } else {
        let mut seen = IndexMap::new();
        seen.insert(attack_id, tick);
        d.get_or_insert_with(DodgeState::default).fighters.insert(vid, DodgeEntry { id: None, seen, at: tick });
    }
    if !mark {
        return false;
    }
    if let Some(e) = d.get_or_insert_with(DodgeState::default).fighters.get_mut(&vid) {
        e.id = Some(attack_id);
    }
    true
}

/// `dodgeQuest`: Comp_Pawn_HitBoxHandler DodgeQuest_Update(HitActor) @12471, the stat
/// trackers' part (the quests are not ported): GetByFighter(HitActor) has to succeed @12611, then the handler's gate
/// (Temp_bool_IsClosed_Variable_4, a DoOnce whoever dodged) closes, the dodger's account gets TotalAttacksDodged @13687
/// (no bot test there) and a 0.8 s timer @13757 opens it again. The gate is the host's (the handler's holder: a buff
/// actor's state made per explosion has a new one each time). The build runs it on a dedicated server only (@12492);
/// the sim counts it on every page, for every fighter, and the online server adds it to the dodger's account.
/// Unverified: GetByFighter of a minion is taken to fail (a stock controller, no player data); the timer's frame
/// (K2_SetTimerDelegate 0.8, not looping) is counted as a timer tick from the dodge's tick, and where
/// the timer manager's tick falls against the overlap updates is not read.
fn dodge_quest(w: &mut World, host: ActorRef, vic: FighterId, tick: i32) {
    if w.fighter(vic).life.is_minion {
        return;
    }
    let Some(d) = dodge_of(w, host) else { return };
    if quest_gate(d, tick) {
        w.fighter_mut(vic).stats.attacks_dodged += 1;
    }
}

/// The handler's DodgeQuest_Update gate (DODGE_QUEST_GATE): true when it was open (it closes for 0.8 s).
fn quest_gate(d: &mut DodgeState, tick: i32) -> bool {
    let gate = timer_ticks(Some(0.8));
    let tick = crate::real(tick);
    if tick < d.quest_gate {
        return false;
    }
    d.quest_gate = tick + gate;
    true
}

/// `dodgeTail`: BaseFighter BPI_SetDodgedSuccessfully(true, AttackingActor) (pfgDoDamage
/// @8035 on an invulnerable victim, its one caller): @108271 DodgedSuccessfully = true
/// (SetDodgedSuccessfullyMulticast), then the broadcasts EvadedAttack(AttackingActor) @108294 and
/// AfterEvadedAttack(AttackingActor) @108322, ahead of the caller's dodge burst. The pawn's own handler
/// (`CharHooks::evaded_attack`, bound at its BeginPlay) comes before a buff's (bound at the buff's BeginPlay). by: the
/// handler's owner (a fighter, a spawned actor or an item); attacker: the same when it is a fighter.
pub fn dodge_tail(w: &mut World, vic: FighterId, by: ActorRef, attacker: Option<FighterId>) {
    dodge_tail_of(w, vic, Some(by), attacker);
}

/// `dodgeTail` with `by` none for a host no list holds (`dealer_dodged`): the broadcasts carry no actor.
fn dodge_tail_of(w: &mut World, vic: FighterId, by: Option<ActorRef>, attacker: Option<FighterId>) {
    w.fighter_mut(vic).evade.dodged_successfully = true;
    let h = w.fighter(vic).hooks.char.evaded_attack;
    if let Some(h) = h {
        h(w, vic, attacker);
    }
    // a run-time binder's broadcast
    match by {
        Some(by) => {
            broadcast_bound(w, DelegateList::Pawn(vic, PawnList::EvadedAttack), &DelegateCall::EvadedAttack { by })
        }
        None if crate::world::flow::delegate_list_mut(w, DelegateList::Pawn(vic, PawnList::EvadedAttack))
            .is_some_and(|l| !l.is_empty()) =>
        {
            crate::fighter::common::warn_once(w, crate::name!("EvadedAttack of a PerkDealer to a run-time binder"));
        }
        None => {}
    }
    if w.fighter(vic).buffs.buffs.is_empty() {
        return;
    }
    let by = by.map(|a| actor_by(w, a, attacker));
    let arg = BuffSignalArg { by, ..BuffSignalArg::default() };
    buff_signal(w, vic, BuffSignal::EvadedAttack, arg.clone());
    buff_signal(w, vic, BuffSignal::AfterEvadedAttack, arg);
}

// ---- the dodge burst
//
// Comp_Pawn_HitBoxHandler SpawnDodgeEffectRatelimited(HitActor) U@16445 -> U@3891: a gate (IsClosed_2) that closes,
// calls SpawnDodgeEffectMulticast @3953 and opens behind Delay(0.2) @3976 (resumes at U@38); the Multicast U@8048 a
// second gate of the same kind (IsClosed_3) around SpawnDodgeEffect(HitActor) @8119, its Delay(0.2) @8208 (U@15). Both
// are the handler's own variables: one burst per attacking handler per 0.2 s, whoever dodged. As for the DODGED text's
// two gates (DODGE_TEXT_GATE below) they close in one call and open on one frame: one frame number.
// SpawnDodgeEffect (the client side): PS_EvadeConfirm on the dodger's Capsule and PF_Post_Event(ui_ability_evade, the
// dodger) @940, posted here as the dodger's sound.
// Unverified: the Delay is made in pfgDoDamage, inside an overlap update; it is counted from that frame as the text's
// (whether the frame it is made in counts, with the handler's tick ahead of or behind the overlap, not read).
// Its caller: attackDodged's success. Behind the Multicast's gate, a handler with UseDelayedDodgeText false raises
// SpawnDodgePopText(HitActor) at once @8142-8185, the DODGED text the others raise at the overlap's end (dodgeTick).

/// `spawnDodgeEffect`.
pub fn spawn_dodge_effect(w: &mut World, host: ActorRef, vic: FighterId, tick: i32) {
    let delayed = uses_delayed_dodge_text(w, host);
    let Some(d) = dodge_of(w, host) else { return };
    if fx_gate(d, tick) {
        push_dodge_effect(w, vic, delayed);
    }
}

/// The handler's burst gates (DODGE_FX_GATE): true when they were open (they close for 0.2 s).
fn fx_gate(d: &mut DodgeState, tick: i32) -> bool {
    let gate = latent_frames(0.2);
    let tick = crate::real(tick);
    if tick < d.fx_gate {
        return false;
    }
    d.fx_gate = tick + gate;
    true
}

/// SpawnDodgeEffect on the dodger `vic`, and the DODGED text at once for a handler without UseDelayedDodgeText.
fn push_dodge_effect(w: &mut World, vic: FighterId, delayed: bool) {
    let me = w.fighter_mut(vic);
    let fighter = me.id;
    me.events.push(Event::EvadeFx { fighter });
    me.events.push(Event::Sound {
        fighter,
        ev: crate::name!("ui_ability_evade"),
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    });
    if !delayed {
        // @8185
        me.events.push(dodged_text(fighter));
    }
}

/// The DODGED pop text of fighter `fighter`.
fn dodged_text(fighter: Fid) -> Event {
    Event::PopText { fighter, kind: PopKind::Dodged, stacks: 0.0, gain: None, params: None }
}

/// `dodgeOver`: a call of `host`'s pfgDoDamage on a fighter that is not invulnerable: its
/// entry goes @782 (and the hit box's next update over it, should it be invulnerable again, is taken as a new
/// overlap).
pub fn dodge_over(w: &mut World, host: ActorRef, vic: FighterId) {
    let vid = w.fighter(vic).id;
    if let Some(Some(d)) = dodge_slot(w, host) {
        d.fighters.shift_remove(&vid);
    }
}

// ---- the DODGED text
//
// Comp_Pawn_HitBoxHandler ReceiveTick U@8263 (authority U@8325): for each key of DodgingFighters (Map_Keys U@8346)
// CheckDodgingFighterOverlap U@4985 (@0-551: GetOverlappingActors(BaseFighter_C) of every key of HitBoxesToDamage); a
// fighter over none of them: Map_Remove U@5115 and, with UseDelayedDodgeText U@5175 (class default true; false on the
// handler templates of HarleyJackInTheBox and HarleyBomb, whose text comes at the dodge itself through
// SpawnDodgeEffectRatelimited U@8093-8208: spawnDodgeEffect), SpawnDodgeTextRateLimited(fighter) U@5244. Whether he is
// still invulnerable is not asked, and the removal at pfgDoDamage @782 (dodgeOver, and attackDodged's `e.id = null`:
// the dodge ended in a hit, or a box with another AttackID came) raises nothing.
// SpawnDodgeTextRateLimited U@5681-5730: a gate (IsClosed_1) that closes, calls SpawnDodgeTextMulticast and opens
// behind Delay(0.2) (resumes at U@566). SpawnDodgeTextMulticast U@5527-5576: a second gate (IsClosed) of the same kind
// (U@543) around SpawnDodgePopText U@5553. Both are the handler's own variables, so one text per attacking handler per
// 0.2 s whoever dodged; a fighter removed while a gate is closed has lost his text (the entry is gone). Where every
// machine runs the whole sim (the port offline and online) the two close in one call and their Delays, made in one
// call, end in one latent pass: one frame number holds both.
// The Delay is latentFrames(0.2) frames from the tick that closed the gate to the first tick that finds it open: a
// component runs ReceiveTick and then its latent actions with the world delta, so the frame the Delay is made in counts
// (the component tick, then its latent actions).
// Unverified: the pass runs once at the end of the frame, behind every overlap update; where the handler's tick falls
// among them is not read (the text's frame is within one of the build's).
// Deviation: "overlaps a hit box" is what the world's tests see (the host's active hit boxes over the fighter's hurt
// shapes, behind their own gates: a fighter's HitOnlyOneFighter box that has hit somebody no longer counts); the
// build's GetOverlappingActors also sees any other component of the pawn that answers the hit box's channel, so its
// text can come later.
// A dead host gets no pass: a destroyed actor's handler never ticks again and nothing on its destroy raises the text.
// Map_Keys order is taken as this map's insertion order (it decides who gets the text when two fighters leave one
// host's boxes in the same frame).

/// `dodgeTick`: once a frame for every live host that can hold the map, behind the frame's
/// overlap tests. Nothing is made here but the event of a text, pushed to `events`.
pub fn dodge_tick(w: &mut World, host: ActorRef, tick: i32, events: &mut Vec<Event>) {
    // DODGE_TEXT_GATE
    let gate = latent_frames(0.2);
    if !matches!(dodge_slot(w, host), Some(Some(h)) if !h.fighters.is_empty()) {
        return;
    }
    let text = uses_delayed_dodge_text(w, host);
    let Some(Some(h)) = dodge_slot(w, host) else { return };
    let t = crate::real(tick);
    for (&vic, e) in &mut h.fighters {
        // dodgePass
        if e.id.is_none() || e.at == tick {
            continue;
        }
        // U@5115
        e.id = None;
        // U@5175, U@5681 / U@5527
        if !text || t < h.gate {
            continue;
        }
        h.gate = t + gate;
        // SpawnDodgePopText U@5553
        events.push(dodged_text(vic));
    }
}

// ---- the call surface of abilities: Comp_Pawn_AbilityManager's cooldowns and ammo

/// An ability row as the manager reads it (`AbilityDef`): the template's (`ch.abilities[slot]`) or the one a graph
/// wrote in place and passed to OverrideAbility ([`AbilityOver`]). `AlternateCooldowns` is read by
/// [`alternate_cooldown`].
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct AbilityRow {
    pub cooldown: Real,
    pub icon: Option<Name>,
    pub max_count: Real,
    pub start_count: Real,
    pub count_cooldown: Real,
    pub count_spent_per_trigger: Real,
    pub count_refreshed_per_cooldown: Real,
    pub cooldown_always_visible: bool,
    pub ally_cooldown_visible: bool,
    pub foe_cooldown_visible: bool,
    pub static_ability_cooldown: bool,
    pub static_ammo_cooldown: bool,
    pub refresh_cooldown_on_respawn: bool,
}

impl AbilityRow {
    /// The row of a data definition; its icon is a name of the data, interned at bind.
    pub fn of(names: &crate::names::Names, a: &AbilityDef) -> AbilityRow {
        AbilityRow {
            cooldown: crate::math::narrow(a.cooldown),
            icon: a.icon.as_deref().map(|s| data_name_in(names, s)),
            max_count: crate::math::narrow(a.max_count),
            start_count: crate::math::narrow(a.start_count),
            count_cooldown: crate::math::narrow(a.count_cooldown),
            count_spent_per_trigger: crate::math::narrow(a.count_spent_per_trigger),
            count_refreshed_per_cooldown: crate::math::narrow(a.count_refreshed_per_cooldown),
            cooldown_always_visible: a.cooldown_always_visible,
            ally_cooldown_visible: a.ally_cooldown_visible,
            foe_cooldown_visible: a.foe_cooldown_visible,
            static_ability_cooldown: a.static_ability_cooldown,
            static_ammo_cooldown: a.static_ammo_cooldown,
            refresh_cooldown_on_respawn: a.refresh_cooldown_on_respawn,
        }
    }
}

/// `abilityOver[slot]`: the row a graph wrote over the template's and passed to OverrideAbility.
#[derive(State, Debug, Default)]
pub struct AbilityOver {
    pub row: AbilityRow,
    pub alternate_cooldowns: Option<Vec<Real>>,
}

/// `abilityRow`: the row of an ability slot: Comp_Pawn_AbilityManager.Abilities[slot],
/// which is also its container's Ability (CreateAbilities hands each row to its AbilityCooldownContainer). The pawn's
/// template row until a graph writes the array's row in place and calls OverrideAbility with it
/// ([`override_ability`]): from then on both read the written row (`abilityOver`), and nothing writes the template's
/// back.
pub fn ability_row(w: &World, f: FighterId, slot: i32) -> Option<AbilityRow> {
    if let Some(o) = w.fighter(f).abilities.ability_over.as_ref().and_then(|o| o.get(&slot)) {
        return Some(o.row);
    }
    let a = char_data(w, f).def.abilities.get(usize::try_from(slot).ok()?)?;
    Some(AbilityRow::of(&w.names, a))
}

/// The row's AlternateCooldowns[alt].
pub fn alternate_cooldown(w: &World, f: FighterId, slot: i32, alt: i32) -> Option<Real> {
    let alt = usize::try_from(alt).ok()?;
    if let Some(o) = w.fighter(f).abilities.ability_over.as_ref().and_then(|o| o.get(&slot)) {
        return o.alternate_cooldowns.as_ref()?.get(alt).copied();
    }
    let a = char_data(w, f).def.abilities.get(usize::try_from(slot).ok()?)?;
    a.alternate_cooldowns.as_ref()?.get(alt).copied().map(crate::math::narrow)
}

/// The slot's container.
fn slot_of(w: &World, f: FighterId, slot: i32) -> Option<&AbilityState> {
    w.fighter(f).abilities.abilities.get(usize::try_from(slot).ok()?)
}

/// [`slot_of`], to write.
fn slot_mut(w: &mut World, f: FighterId, slot: i32) -> Option<&mut AbilityState> {
    w.fighter_mut(f).abilities.abilities.get_mut(usize::try_from(slot).ok()?)
}

/// The slots' count, as a slot number.
fn slot_count(w: &World, f: FighterId) -> i32 {
    i32::try_from(w.fighter(f).abilities.abilities.len()).expect("fewer than i32::MAX ability slots")
}

/// `overrideAbility`: Comp_Pawn_AbilityManager OverrideAbility(Ability, slot)
/// (ubergraph @10363-10467): the slot's container SetAbility(Ability) (AbilityCooldownContainer SetAbility @0: the row
/// alone, no cooldown value is read again), then ResetAbility(slot). A slot without a container: nothing. `row` is the
/// manager's own row after the caller's in-place writes (Buff_Disguise U@7978-8809 writes Abilities[0] and passes it).
pub fn override_ability(w: &mut World, f: FighterId, slot: i32, row: AbilityOver) {
    if slot_of(w, f, slot).is_none() {
        return;
    }
    w.fighter_mut(f).abilities.ability_over.get_or_insert_with(IndexMap::new).insert(slot, row);
    reset_ability(w, f, slot);
}

/// `resetAbility`: Comp_Pawn_AbilityManager ResetAbility(slot) (ubergraph @10730): with the
/// ability or the ammo cooldown running the widget's SetCooldownExitForPlayer (@11151); the container's ResetAbility
/// (its ubergraph @235, as [`reset_abilities`]: only with the row's RefreshCooldownOnRespawn the start state from the
/// row, both timers cleared); then CreateAbilityUI(player, slot, the container's Ability) (@11533).
pub fn reset_ability(w: &mut World, f: FighterId, slot: i32) {
    let Some(a) = ability_row(w, f, slot) else { return };
    let Some(s) = slot_of(w, f, slot) else { return };
    if s.cooldown || s.ammo_cooldown {
        emit(w, f, |fighter| Event::UiAbility { fighter, n: slot, op: UiAbilityOp::Exit });
    }
    if a.refresh_cooldown_on_respawn
        && let Some(s) = slot_mut(w, f, slot)
    {
        refresh(s, &a);
    }
    let max_count = js::to_int32(a.max_count);
    emit(w, f, |fighter| Event::AbilityUiCreate { fighter, slot, icon: a.icon.unwrap_or(Name::NONE), max_count });
}

/// The container's ResetAbility with RefreshCooldownOnRespawn: the start state from the row, both timers cleared.
fn refresh(s: &mut AbilityState, a: &AbilityRow) {
    s.cooldown = false;
    s.ammo_cooldown = false;
    s.cd_frames = 0.0;
    s.ammo_frames = 0.0;
    s.ammo = a.start_count;
    s.default_cd = a.cooldown;
    s.active_cd = a.cooldown;
    s.ammo_max = a.count_cooldown;
}

/// `hasAbilityResources`: Comp_Pawn_AbilityManager.HasAbilityResources: no cooldown
/// running, and MaxCount 0 or enough ammo for a trigger. A slot past the array reads Array_Get's default entry
/// (MaxCount 0, no container: taken as ready).
pub fn has_ability_resources(w: &World, f: FighterId, slot: i32) -> bool {
    let (Some(a), Some(s)) = (ability_row(w, f, slot), slot_of(w, f, slot)) else { return true };
    !s.cooldown && (s.ammo >= a.count_spent_per_trigger || a.max_count == 0.0)
}

/// `gameModeCooldownDisabled`: IsGameModeCooldownDisabled (Comp_Pawn_AbilityManager
/// @0..754): false unless the owner's controller is a player controller; then the Panda game mode's
/// IsInstantCooldownsEnabled (a client without the game mode reads PlayerArray[0]'s
/// TrainingModeConfig.bInstantCooldowns, the same value offline). A bot keeps its cooldowns with the Lab toggle on.
pub fn game_mode_cooldown_disabled(w: &World, f: FighterId) -> bool {
    let me = w.fighter(f);
    !me.controls.bot_controlled && me.abilities.instant_cooldowns
}

/// `cooldownsBlocked`: Comp_Pawn_AbilityManager.BlockCooldownCount < 1 (`blockCooldowns`:
/// absent at 0). TriggerAbilityCheck, TriggerAbilityActual and TriggerAbilityServer each read it.
pub fn cooldowns_blocked(w: &World, f: FighterId) -> bool {
    w.fighter(f).abilities.block_cooldowns.unwrap_or(0.0) >= 1.0
}

/// `adjustBlockCooldowns`: AdjustBlockCooldowns(Block) (@0..329): BlockCooldownCount + 1,
/// or Max(BlockCooldownCount - 1, 0); a PrintString of the count either way (a development print). Callers:
/// C023A_Buff_Silenced's BeginPlay (true, ubergraph @786) and EndPlay (false, @939), AnimNotify_BlockCooldown's
/// NotifyBegin (true) and NotifyEnd (false).
pub fn adjust_block_cooldowns(w: &mut World, f: FighterId, block: bool) {
    let a = &mut w.fighter_mut(f).abilities;
    let n = a.block_cooldowns.unwrap_or(0.0);
    a.block_cooldowns = Some(if block { n + 1.0 } else { js::max(n - 1.0, 0.0) });
}

/// `triggerAbilityCheck`: TriggerAbilityCheck (@0..1002): (HasAbilityResources ||
/// DisableCD || IsGameModeCooldownDisabled) && BlockCooldownCount < 1 (cooldownsBlocked). DisableCD is false in the
/// CDO; the tutorial actor writes it on its pawns (`disableCD`); the debug cheat that toggles it
/// (Comp_GameState_Debug @5048) is not ported.
/// A failed check of a pawn whose controller is a player controller (@233): PF_Post_VO_Event(owner, "VO Cooldown
/// Ability 1..3" for slot 0..2, no context actor, probability 1), PF_Post_Global_Event("SFX Cooldown 0..2" for slot
/// 0..2; the Select's default for a later slot is no event), ShowAbilityUINotReady(player, slot) ->
/// UI_Broker.SetCooldownErrorForPlayer (the HUD's 'error' op). The three VO strings are empty in the CDO and no ported
/// pawn sets them: the empty alias goes through the row search as any other (RandomFloat included).
/// The Ability<n>NotReady conditions negate this same check, so they give the feedback too.
pub fn trigger_ability_check(w: &mut World, f: FighterId, slot: i32) -> bool {
    let disable_cd = w.fighter(f).abilities.disable_cd;
    if (has_ability_resources(w, f, slot) || disable_cd || game_mode_cooldown_disabled(w, f))
        && !cooldowns_blocked(w, f)
    {
        return true;
    }
    if !w.fighter(f).controls.bot_controlled {
        let def = &char_data(w, f).def;
        let at = usize::try_from(slot).ok();
        let vo = at.and_then(|i| def.ability_cooldown_vo.get(i)).map_or(Name::NONE, |s| data_name(w, s));
        let ev = at.and_then(|i| def.ability_cooldown_sfx.get(i)).filter(|s| !s.is_empty()).map(|s| data_name(w, s));
        post_vo(w, f, vo, None, 1.0);
        if ev.is_some() {
            sfx(w, f, ev);
        }
        emit(w, f, |fighter| Event::UiAbility { fighter, n: slot, op: UiAbilityOp::Error });
    }
    false
}

/// `triggerAbility`: TriggerAbilityActual / TriggerAbilityServer (locally controlled /
/// authority: both hold offline): with the game mode's cooldowns on (IsGameModeCooldownDisabled false: with the Lab's
/// instant cooldowns a player's trigger starts nothing and spends no ammo), DisableCD false and BlockCooldownCount < 1,
/// UseAbility -> TriggerAbilityTimers. No resource check: a trigger on a running cooldown restarts it. A slot outside
/// the array reaches no container (nothing happens).
/// TriggerAbilityTimers: SetAbilityCooldown(ActiveAbilityCooldown); CurrentAmmo - CountSpentPerTrigger (clamped to
/// 0..MaxCount); the ammo cooldown (GetMaxAmmoCooldown) starts when it is negative, or positive and not running. With
/// BlockCooldownCount >= 1 (TriggerAbilityActual @99 / @336, TriggerAbilityServer @175 / @328) nothing starts either.
pub fn trigger_ability(w: &mut World, f: FighterId, slot: i32) {
    if w.fighter(f).abilities.disable_cd || game_mode_cooldown_disabled(w, f) || cooldowns_blocked(w, f) {
        return;
    }
    use_ability(w, f, slot);
}

/// `useAbility`: UseAbility (ubergraph @5350; UseAbilityServer @5851 adds an authority
/// test, true offline): TriggerAbilityTimers, then UpdateAbilityMulticast with the cooldown pulse. No guard of its
/// own: the game mode's instant cooldowns and BlockCooldownCount are TriggerAbilityActual's / TriggerAbilityServer's
/// (Comp_StevenShield calls UseAbilityServer straight).
pub fn use_ability(w: &mut World, f: FighterId, slot: i32) {
    let Some(a) = ability_row(w, f, slot) else { return };
    let Some(s) = slot_of(w, f, slot) else { return };
    let active = s.active_cd;
    set_ability_cooldown(w, f, slot, active);
    // TriggerAbilityTimers @159
    cooldown_updated(w, f, slot, false);
    let Some(s) = slot_mut(w, f, slot) else { return };
    s.ammo = js::min(js::max(s.ammo - a.count_spent_per_trigger, 0.0), a.max_count);
    let ammo = s.ammo;
    // @507
    ammo_updated(w, f, slot, ammo, false);
    let Some(s) = slot_of(w, f, slot) else { return };
    let (ammo_max, ammo_cd) = (s.ammo_max, s.ammo_cooldown);
    if ammo_max < 0.0 || (ammo_max > 0.0 && !ammo_cd) {
        set_ammo_cooldown(w, f, slot, ammo_max);
    }
    // UseAbility's UpdateAbilityMulticast
    ability_ui(w, f, slot, true, false);
}

/// `cooldownUpdated`: the AbilityCooldownUpdated(AbilitySlot, Complete) broadcast: what
/// the pawn binds (`CharHooks::ability_cooldown_updated`), then the actors bound to the fighter (FighterSignal), then a
/// run-time binder's.
fn cooldown_updated(w: &mut World, f: FighterId, slot: i32, complete: bool) {
    let h = w.fighter(f).hooks.char.ability_cooldown_updated;
    if let Some(h) = h {
        h(w, f, slot, complete);
    }
    signal_bound(w, f, FighterSignal::AbilityCooldownUpdated, complete, crate::real(slot), Name::NONE, None);
    let call = DelegateCall::AbilityCooldownUpdated { slot, complete };
    broadcast_bound(w, DelegateList::Pawn(f, PawnList::AbilityCooldownUpdated), &call);
}

/// `ammoUpdated`: the AbilityAmmoUpdated(Index, NewAmmo, Increase) broadcast: the
/// fighter's event and what the pawn binds (`CharHooks::ability_ammo_updated`).
fn ammo_updated(w: &mut World, f: FighterId, slot: i32, ammo: Real, increase: bool) {
    emit(w, f, |fighter| Event::AbilityAmmo { fighter, slot, ammo, increase });
    let h = w.fighter(f).hooks.char.ability_ammo_updated;
    if let Some(h) = h {
        h(w, f, slot, ammo, increase);
    }
}

/// `setAmmoCooldownOnAuthority`: SetAmmoCooldownOnAuthority(slot, MaxCooldown,
/// RemainingCooldown) (ubergraph @6920-7671, authority): the container's SetMaxAmmoCooldown(max) @7051,
/// SetAmmoCooldown(remaining) when it is above 0 @7203, then UpdateAbilityMulticast with no cooldown pulse and the ammo
/// pulse when remaining > 0 @7671. A slot past the containers reaches none.
pub fn set_ammo_cooldown_on_authority(w: &mut World, f: FighterId, slot: i32, max: Real, remaining: Real) {
    let Some(s) = slot_mut(w, f, slot) else { return };
    s.ammo_max = max;
    if remaining > 0.0 {
        set_ammo_cooldown(w, f, slot, remaining);
    }
    ability_ui(w, f, slot, false, remaining > 0.0);
}

/// `applyAbilityBuffs`: Comp_State_Manager.ApplyAbilityBuffs(AbilityMods) (@0..790), from
/// CalculateBuffValues' LocalAbilityMods (@6948: 1 times each buff's GetAbilityMods().AbilityCooldownMultiplier that
/// is not nearly 1; a stacking buff gives AbilityMods + stacks x AbilityModsPerStack): when it differs from the
/// fighter's Cooldown Multiplier, BPI_SetCooldownMultiplier(new) and the manager's
/// UpdateAbilityCooldownMultiplier(new / old), which runs HandleAbilityCooldownUpdate(slot, that, UpdateUI, the
/// entry's StaticAbilityCooldown, StaticAmmoCooldown) for every container.
/// No buff of the ported set has a multiplier other than 1 (the perks that do are not ported), so this has no caller
/// with another value yet.
pub fn apply_ability_buffs(w: &mut World, f: FighterId, mult: Real) {
    let a = &mut w.fighter_mut(f).abilities;
    if a.cooldown_multiplier == mult {
        return;
    }
    let rel = mult / a.cooldown_multiplier;
    a.cooldown_multiplier = mult;
    for k in 0..slot_count(w, f) {
        let row = ability_row(w, f, k).expect("a container's row");
        handle_ability_cooldown_update(w, f, k, rel, true, row.static_ability_cooldown, row.static_ammo_cooldown);
    }
}

/// `handleAbilityCooldownUpdate`: HandleAbilityCooldownUpdate (@0..3386). The ability
/// cooldown, unless static: a DefaultAbilityCooldown other than 0 and -1 is multiplied and written with
/// SetMaxAbilityCooldown (default and active); otherwise an ActiveAbilityCooldown other than 0 and -1 is multiplied and
/// written with SetTemporaryMaxAbilityCooldown; either way a running timer restarts at remaining / old x new. The ammo
/// cooldown, unless static and unless -1: the same on GetMaxAmmoCooldown. With UpdateUI, UpdateAbilityMulticast
/// without a pulse.
/// Deviation: the timer's remaining time is whole timer ticks (the build reads the timer's own float).
/// The slot keeps the whole ticks left, not the expire time on the timer manager's double clock: ticks x
/// DeltaSeconds in float has the bits of that clock's read while the timer's rate is whole ticks; a restarted rate
/// (remaining / old x new) is rounded to ticks.
pub fn handle_ability_cooldown_update(
    w: &mut World,
    f: FighterId,
    slot: i32,
    mult: Real,
    update_ui: bool,
    static_cd: bool,
    static_ammo: bool,
) {
    let Some(s) = slot_mut(w, f, slot) else { return };
    let mut restart_cd = None;
    if !static_cd {
        let temp = s.default_cd == 0.0 || s.default_cd == -1.0;
        let prev = if temp { s.active_cd } else { s.default_cd };
        let next = prev * mult;
        if prev != 0.0 && prev != -1.0 && next != prev {
            s.active_cd = next;
            if !temp {
                s.default_cd = next;
            }
            if s.cd_frames > 0.0 {
                restart_cd = Some(((s.cd_frames * DT) / prev) * next);
            }
        }
    }
    if let Some(sec) = restart_cd {
        set_ability_cooldown(w, f, slot, sec);
    }
    let Some(s) = slot_mut(w, f, slot) else { return };
    if !static_ammo && s.ammo_max != -1.0 {
        let prev = s.ammo_max;
        let next = prev * mult;
        if next != prev {
            s.ammo_max = next;
            if s.ammo_frames > 0.0 {
                let sec = ((s.ammo_frames * DT) / prev) * next;
                set_ammo_cooldown(w, f, slot, sec);
            }
        }
    }
    if update_ui {
        ability_ui(w, f, slot, false, false);
    }
}

/// `beginAlternateCooldown`: BeginAlternateCooldownOnAuthority(AbilitySlot,
/// AlternateCooldownSlot, ApplyCooldownMultipliers) (ubergraph @14088-14447): with authority, a valid AbilitySlot of the
/// manager's Abilities and a valid AlternateCooldownSlot of that row's AlternateCooldowns,
/// SetTemporaryAbilityCooldownOnAuthority(slot, the entry, the entry, Apply) @14397.
pub fn begin_alternate_cooldown(w: &mut World, f: FighterId, slot: i32, alt: i32, apply_mult: bool) {
    let Some(v) = alternate_cooldown(w, f, slot, alt) else { return };
    set_temporary_ability_cooldown(w, f, slot, v, v, apply_mult);
}

/// `setTemporaryAbilityCooldown`: SetTemporaryAbilityCooldownOnAuthority(AbilitySlot,
/// TemporaryMaxCooldown, TemporaryRemainingCooldown, ApplyCooldownMultipliers) (ubergraph @13018-14083), with
/// authority: the container's SetTemporaryMaxAbilityCooldown (ActiveAbilityCooldown = max; the default stays, so
/// OnAbilityCooldownComplete puts the default back) @13149; a remaining time > 0: its SetAbilityCooldown(remaining)
/// @13301; with Apply, HandleAbilityCooldownUpdate(slot, the owner's BPI_GetCooldownMultiplier (Cooldown Multiplier,
/// absolute: not relative to a 1 the temporary time had), UpdateUI false, StaticAbilityCooldown false,
/// StaticAmmoCooldown false) @13505, whatever the row's Static flags say; then UpdateAbilityMulticast with
/// PulseAbilityCooldown = remaining > 0 and no ammo pulse @13963.
pub fn set_temporary_ability_cooldown(
    w: &mut World,
    f: FighterId,
    slot: i32,
    max: Real,
    remaining: Real,
    apply_mult: bool,
) {
    // Array_Get past AbilityContainers: calls on None
    let Some(s) = slot_mut(w, f, slot) else { return };
    s.active_cd = max;
    finish_set_cooldown(w, f, slot, remaining, apply_mult);
}

/// The tail of SetTemporaryAbilityCooldownOnAuthority and SetAbilityCooldownOnAuthority.
fn finish_set_cooldown(w: &mut World, f: FighterId, slot: i32, remaining: Real, apply_mult: bool) {
    if remaining > 0.0 {
        set_ability_cooldown(w, f, slot, remaining);
    }
    if apply_mult {
        let m = w.fighter(f).abilities.cooldown_multiplier;
        handle_ability_cooldown_update(w, f, slot, m, false, false, false);
    }
    ability_ui(w, f, slot, remaining > 0.0, false);
}

/// `setAbilityCooldownOnAuthority`: SetAbilityCooldownOnAuthority(slot, MaxCooldown,
/// RemainingCooldown, ApplyCooldownMultiplier) (ubergraph @9288-10329, authority): the container's
/// SetMaxAbilityCooldown(max) @9419 (DefaultAbilityCooldown and ActiveAbilityCooldown both),
/// SetAbilityCooldown(remaining) when it is above 0 @9571, with Apply HandleAbilityCooldownUpdate(slot,
/// BPI_GetCooldownMultiplier, false, false, false) @9775, then UpdateAbilityMulticast with PulseAbilityCooldown =
/// remaining > 0 and no ammo pulse @10233. A slot past the containers reaches none.
pub fn set_ability_cooldown_on_authority(
    w: &mut World,
    f: FighterId,
    slot: i32,
    max: Real,
    remaining: Real,
    apply_mult: bool,
) {
    let Some(s) = slot_mut(w, f, slot) else { return };
    s.default_cd = max;
    s.active_cd = s.default_cd;
    finish_set_cooldown(w, f, slot, remaining, apply_mult);
}

/// `abilityUi`: UpdateAbilityMulticast(slot, the container's GetCooldownData, server
/// time, PulseAbilityCooldown, PulseAmmoCooldown) -> UpdateLocalUI, as the HUD's 'update' op: the remaining and full
/// times in seconds (a flag set without a timer reads -1). Offline the server time is the sender's, so no time is
/// taken off.
pub fn ability_ui(w: &mut World, f: FighterId, slot: i32, pulse: bool, pulse_ammo: bool) {
    let s = slot_of(w, f, slot).expect("an ability container");
    let left = |on: bool, frames: Real, max: Real| {
        if frames > 0.0 {
            js::min(frames * DT, max)
        } else if on {
            -1.0
        } else {
            0.0
        }
    };
    let op = UiAbilityOp::Update {
        ammo: s.ammo,
        cd: left(s.cooldown, s.cd_frames, s.active_cd),
        cd_max: s.active_cd,
        ammo_cd: left(s.ammo_cooldown, s.ammo_frames, s.ammo_max),
        ammo_max: s.ammo_max,
        pulse,
        pulse_ammo,
    };
    emit(w, f, |fighter| Event::UiAbility { fighter, n: slot, op });
}

/// `actorSpawnAbility`: SpawnedActorParent's spawn (ubergraph 13212):
/// TriggerAbilityOnSpawn (deprecated) triggers the instigator's TriggerAbilityIndex with TriggerAbilityOnSpawnIndex,
/// else its current attack's AbilitySlot; TriggerAbilityOnSpawnIndex alone triggers TriggerAbilityIndex (0 in the
/// parent's CDO).
pub fn actor_spawn_ability(w: &mut World, f: FighterId, p: &Props) {
    let truthy = |k: &str| p.get(k).is_some_and(js_truthy);
    if truthy("TriggerAbilityOnSpawnIndex") {
        let k = p.get("TriggerAbilityIndex").and_then(Value::as_f64).unwrap_or(0.0);
        trigger_ability(w, f, crate::math::dbl::to_int32(k));
    } else if truthy("TriggerAbilityOnSpawn") {
        let k = w.fighter(f).abilities.attack_slot;
        trigger_ability(w, f, k);
    }
}

/// A data value's JavaScript truthiness.
pub fn js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Num(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        _ => true,
    }
}

/// `cooldownFrames`: AbilityCooldownContainer SetAbilityCooldown / SetAmmoCooldown
/// (HandleCooldownByReference): the timer is cleared, the flag is seconds != 0 and a timer starts for seconds > 0 (a
/// negative time leaves the flag set with no timer). One set in the tick groups is pending for that frame's timer tick
/// (timerFrames).
pub fn cooldown_frames(w: &World, f: FighterId, sec: Real) -> Real {
    if sec > 0.0 {
        timer_ticks(Some(sec)) + if w.fighter(f).abilities.in_ability_timers { 0.0 } else { 1.0 }
    } else {
        0.0
    }
}

/// `setAbilityCooldown`.
pub fn set_ability_cooldown(w: &mut World, f: FighterId, slot: i32, sec: Real) {
    let frames = cooldown_frames(w, f, sec);
    if let Some(s) = slot_mut(w, f, slot) {
        s.cooldown = sec != 0.0;
        s.cd_frames = frames;
    }
}

/// `setAmmoCooldown`.
pub fn set_ammo_cooldown(w: &mut World, f: FighterId, slot: i32, sec: Real) {
    let frames = cooldown_frames(w, f, sec);
    if let Some(s) = slot_mut(w, f, slot) {
        s.ammo_cooldown = sec != 0.0;
        s.ammo_frames = frames;
    }
}

/// `tickAbilities`: the containers' timers (world timers: they run through hitpause and a
/// ring-out). OnAbilityCooldownComplete: AbilityOnCooldown false, ActiveAbilityCooldown = DefaultAbilityCooldown.
/// OnAmmoCooldownComplete: AmmoOnCooldown false, then the manager's UpdateAbilityAmmo(slot,
/// CountRefreshedPerCooldown, true): the ammo (clamped), ui_ability_ammo (PF_Post_Global_Event), and with a positive
/// ammo cooldown (GetMaxAmmoCooldown) SetAmmoCooldown(it while below MaxCount, else 0).
pub fn tick_abilities(w: &mut World, f: FighterId) {
    w.fighter_mut(f).abilities.in_ability_timers = true;
    // (forEach visits the slots there at its start.)
    for k in 0..slot_count(w, f) {
        let Some(s) = slot_mut(w, f, k) else { continue };
        if s.cd_frames > 0.0 {
            s.cd_frames -= 1.0;
            if s.cd_frames == 0.0 {
                ability_cooldown_complete(w, f, k);
            }
        }
        let Some(s) = slot_mut(w, f, k) else { continue };
        if s.ammo_frames > 0.0 {
            s.ammo_frames -= 1.0;
            if s.ammo_frames == 0.0 {
                ammo_cooldown_complete(w, f, k);
            }
        }
    }
    w.fighter_mut(f).abilities.in_ability_timers = false;
}

/// `abilityCooldownComplete`.
pub fn ability_cooldown_complete(w: &mut World, f: FighterId, k: i32) {
    let Some(s) = slot_mut(w, f, k) else { return };
    s.cooldown = false;
    s.active_cd = s.default_cd;
    // the manager's OnAbilityCooldownComplete @3439, ahead of its UpdateAbilityMulticast
    cooldown_updated(w, f, k, true);
    ability_ui(w, f, k, true, false);
}

/// `ammoCooldownComplete`.
pub fn ammo_cooldown_complete(w: &mut World, f: FighterId, k: i32) {
    let a = ability_row(w, f, k).expect("a container's row");
    let Some(s) = slot_mut(w, f, k) else { return };
    s.ammo_cooldown = false;
    s.ammo = js::min(js::max(s.ammo + a.count_refreshed_per_cooldown, 0.0), a.max_count);
    let ammo = s.ammo;
    let ev = crate::name!("ui_ability_ammo");
    sfx(w, f, Some(ev));
    // UpdateAbilityAmmo(slot, CountRefreshedPerCooldown, true) (@2327) -> @4334
    ammo_updated(w, f, k, ammo, a.count_refreshed_per_cooldown > 0.0);
    restart_ammo(w, f, k, &a);
    ability_ui(w, f, k, false, true);
}

/// With a positive ammo cooldown, SetAmmoCooldown(it while below MaxCount, else 0).
fn restart_ammo(w: &mut World, f: FighterId, k: i32, a: &AbilityRow) {
    let Some(s) = slot_of(w, f, k) else { return };
    if s.ammo_max > 0.0 {
        let sec = if s.ammo < a.max_count { s.ammo_max } else { 0.0 };
        set_ammo_cooldown(w, f, k, sec);
    }
}

/// `updateAbilityAmmo`: Comp_Pawn_AbilityManager.UpdateAbilityAmmo(slot, change, restart)
/// (ubergraph 3946-5261): CurrentAmmo + change, clamped to 0..MaxCount (AbilityCooldownContainer SetCurrentAmmo @725);
/// ui_ability_ammo; with restart and a positive GetMaxAmmoCooldown, SetAmmoCooldown(it while below MaxCount, else 0)
/// (@4556-4827); the UI update.
pub fn update_ability_ammo(w: &mut World, f: FighterId, slot: i32, change: Real, restart: bool) {
    let Some(a) = ability_row(w, f, slot) else { return };
    let Some(s) = slot_mut(w, f, slot) else { return };
    s.ammo = js::min(js::max(s.ammo + change, 0.0), a.max_count);
    let ammo = s.ammo;
    let ev = crate::name!("ui_ability_ammo");
    sfx(w, f, Some(ev));
    // @4334
    ammo_updated(w, f, slot, ammo, change > 0.0);
    if restart {
        restart_ammo(w, f, slot, &a);
    }
    ability_ui(w, f, slot, false, change > 0.0);
}

/// `updateAbilityCooldown`: UpdateAbilityCooldownServer(slot, Increase, Seconds,
/// PercentOfTotalCD) (Comp_Pawn_AbilityManager ubergraph 2380-3438, authority) -> the container's UpdateAbilityCooldown
/// (AbilityCooldownContainer @0-1194) for the ability cooldown when ActiveAbilityCooldown > 0 and the entry has no
/// StaticAbilityCooldown, and for the ammo cooldown when GetMaxAmmoCooldown > 0 without StaticAmmoCooldown:
/// remaining + (full x Percent + Seconds) x (Increase ? 1 : -1), clamped to 0..full (full = ActiveAbilityCooldown /
/// AmmoCooldown), written with SetAbilityCooldown / SetAmmoCooldown (0 clears the timer and the flag); a time that
/// reaches 0 from above 0 runs the timer's own completion (OnAbilityCooldownComplete / OnAmmoCooldownComplete).
/// GetRemaining reads -1 for a flag set without a timer. Then UpdateAbilityMulticast without a pulse.
/// Callers in the build: Buff_C019_ApplybuffOnOverlap AllowApplication (@2706: (TriggerAbilityIndex, false, 0, 1), 25 s
/// after an ally took the buff; teams only), MarkTeleportProjectile, the AbilityRefund perks, the batarang, C015 /
/// C016. Not ported: the calls of Buff_C019_ApplybuffOnOverlap and MarkTeleportProjectile.
/// Deviation: the remaining time is whole timer ticks (the build reads the timer's own float).
/// The slot keeps the whole ticks left, not the expire time on the timer manager's double clock: ticks x
/// DeltaSeconds in float has the bits of that clock's read while the timer's rate is whole ticks; a restarted rate
/// (remaining / old x new) is rounded to ticks.
pub fn update_ability_cooldown(w: &mut World, f: FighterId, slot: i32, increase: bool, seconds: Real, pct: Real) {
    let Some(a) = ability_row(w, f, slot) else { return };
    if slot_of(w, f, slot).is_none() {
        return;
    }
    let sign = if increase { 1.0 } else { -1.0 };
    let next = |on: bool, frames: Real, full: Real| {
        let left = if frames > 0.0 {
            frames * DT
        } else if on {
            -1.0
        } else {
            0.0
        };
        let add = ((full * pct) * sign) + (seconds * sign);
        js::min(js::max(left + add, 0.0), full)
    };
    let s = slot_of(w, f, slot).expect("checked");
    if s.active_cd > 0.0 && !a.static_ability_cooldown {
        let prev = if s.cd_frames > 0.0 {
            1
        } else if s.cooldown {
            -1
        } else {
            0
        };
        let v = next(s.cooldown, s.cd_frames, s.active_cd);
        set_ability_cooldown(w, f, slot, v);
        if v == 0.0 && prev > 0 {
            ability_cooldown_complete(w, f, slot);
        }
    }
    let s = slot_of(w, f, slot).expect("checked");
    if s.ammo_max > 0.0 && !a.static_ammo_cooldown {
        let prev = if s.ammo_frames > 0.0 {
            1
        } else if s.ammo_cooldown {
            -1
        } else {
            0
        };
        let v = next(s.ammo_cooldown, s.ammo_frames, s.ammo_max);
        set_ammo_cooldown(w, f, slot, v);
        if v == 0.0 && prev > 0 {
            ammo_cooldown_complete(w, f, slot);
        }
    }
    ability_ui(w, f, slot, false, false);
}

/// `resetAbilities`: ResetAbilities (ResetFighterLogic): each container's ResetAbility,
/// which only acts with RefreshCooldownOnRespawn (start state, both timers cleared, the full times back at the entry's
/// Cooldown and CountCooldown whatever the cooldown multiplier is; Morty's marker slot keeps its cooldown), then
/// HandlePreSpawnCooldowns: for an entry with CooldownAlwaysVisible or without RefreshCooldownOnRespawn
/// SetAbilityCooldownOnAuthority(slot, ActiveAbilityCooldown, 0, false) (SetMaxAbilityCooldown: the default becomes the
/// active time; a remaining time of 0 leaves the timer alone) and the ammo cooldown starts when MaxCount > StartCount.
/// HandlePreSpawnCooldowns runs 1.0 s after CreateAbilities (Comp_Pawn_AbilityManager Delay @9077, from
/// PlayerDataReadyMulticast @142469). Unverified: PlayerDataReady's time against the match clock is not decoded.
/// The UI calls: ClearCooldownClockMulticast per slot (the widget's exit; ResetAbility's own exit for a running
/// cooldown finds the exit animation playing), and HandlePreSpawnCooldowns' UpdateAbilityMulticast(.., true, true) for
/// its entries (its two authority calls before that send the same data without a pulse).
pub fn reset_abilities(w: &mut World, f: FighterId) {
    for k in 0..slot_count(w, f) {
        let a = ability_row(w, f, k).expect("a container's row");
        emit(w, f, |fighter| Event::UiAbility { fighter, n: k, op: UiAbilityOp::Exit });
        if a.refresh_cooldown_on_respawn
            && let Some(s) = slot_mut(w, f, k)
        {
            refresh(s, &a);
        }
        pre_spawn_cooldown(w, f, k);
    }
}

/// `preSpawnCooldown`.
pub fn pre_spawn_cooldown(w: &mut World, f: FighterId, k: i32) {
    let a = ability_row(w, f, k).expect("a container's row");
    if a.cooldown_always_visible || !a.refresh_cooldown_on_respawn {
        let Some(s) = slot_mut(w, f, k) else { return };
        s.default_cd = s.active_cd;
        let ammo_max = s.ammo_max;
        if a.max_count > a.start_count && ammo_max > 0.0 {
            set_ammo_cooldown(w, f, k, ammo_max);
        }
        ability_ui(w, f, k, true, true);
    }
}

// ---- the call surface of charge: Comp_Pawn_ChargeAttack

/// `chargePlayRate`: AnimNotify_ChargingAttack Received_NotifyBegin (@1064-1427: LocalAnimation
/// is the active montage either way, Montage_SetPlayRate(it, 1 / LocalChargeMultiplier), LocalChargeMultiplier 1
/// without ModifyMontageWithChargeMultiplier) and Received_NotifyEnd (@152-258: Montage_SetPlayRate(
/// GetCurrentActiveMontage, 1.0)): the active instance plays at the rate from there, a dash montage (GetDashPlayRate)
/// or the ally shield montage ('Montage Play Rate') that is the active one when a late NotifyEnd comes included.
/// Deviation: a NotifyEnd the port runs inside the stop comes ahead of the montage played next; the build's comes at
/// the dispatch after it and sets that montage's rate.
pub fn charge_play_rate(w: &mut World, f: FighterId, rate: Real) {
    if let Some(a) = active_instance(w, f)
        && let Some(s) = slot_state_mut(w.fighter_mut(f), a)
    {
        s.play_rate = f64::from(rate);
    }
}

/// `applyChargeBuffs`: Comp_State_Manager ApplyChargeBuffs(ChargeModifier) (@0-826, with
/// authority): a value other than the owner's Comp_Pawn_ChargeAttack.ChargeMultiplier (exact NotEqual @261) goes to its
/// MulticastSetChargeMultiplier (@406 -> @8627: the field only), then to Comp_HoldCharge's when that differs too
/// (@471-781, setHoldChargeMultiplier); an equal one returns at @321 without looking at Comp_HoldCharge. Every fighter
/// has the first component; both start at the class default 1 on a new pawn (chargeAttackBeginPlay beside
/// holdChargeBeginPlay), so the early return leaves them equal.
pub fn apply_charge_buffs(w: &mut World, f: FighterId, m: Real) {
    let me = w.fighter_mut(f);
    if m == me.charge.charge_attack_multiplier.unwrap_or(1.0) {
        return;
    }
    me.charge.charge_attack_multiplier = Some(m);
    if me.charge.hold_charge.as_ref().is_some_and(|h| m != h.multiplier) {
        set_hold_charge_multiplier(w, f, m);
    }
}

/// `chargeAttackBeginPlay`: a new pawn's Comp_Pawn_ChargeAttack (the World's match reset,
/// beside holdChargeBeginPlay): ChargeMultiplier the class default 1 (no template sets
/// UseInstigatorsChargeMultiplier, ReceiveBeginPlay @8837), no ChargeTimer. ChargeTotalTime goes with the multiplier.
pub fn charge_attack_begin_play(w: &mut World, f: FighterId) {
    let c = &mut w.fighter_mut(f).charge;
    c.charge_attack_multiplier = None;
    c.charge_total_time = None;
    c.charge_timer = None;
}

/// `chargeTotal`: Comp_Pawn_ChargeAttack ChargeTotalTime: TotalDuration x ChargeMultiplier of
/// the last begun charge (StartAttackChargeMultipliersServer @1279), kept on the fighter once the multiplier has been
/// written; until then it is the TotalDuration of `dur`'s notify, the charge the caller knows to be the last begun.
pub fn charge_total(w: &World, f: FighterId, dur: Real) -> Real {
    js::max(w.fighter(f).charge.charge_total_time.unwrap_or(dur), 1e-6)
}

/// `activeGrouped(f)?.m` as the slot it plays in: the grouped entry at index `i`.
fn grouped_slot(i: usize) -> MontageSlot {
    MontageSlot::Grouped(u32::try_from(i).expect("a grouped index"))
}

/// `startChargeTotals`: StartAttackChargeMultipliersServer (@1252-1352, then @15-390) for the
/// notify `n` beginning: ChargeTotalTime = ChargeMultiplier x TotalDuration (every charge, flagged or not),
/// TotalChargedTime = it, and the ChargeTimer (K2_SetTimerDelegate(EndChargeAttackServerFromTimer, ChargeTotalTime,
/// false) @333, a running one restarted) when OnCompleteMontageSectionOverride is a name and ChargeMultiplier != 1
/// (@186-300). The component's On Complete Section Override is written by every start (@121): a running timer fires
/// with the newest.
pub fn start_charge_totals(w: &mut World, f: FighterId, n: NotifyRef) {
    let data = w.data.clone();
    let Some(nd) = notify_def(&data, w, f, n) else {
        let msg = format!("startChargeTotals: notify {} of {} is not in the data", n.index, w.names.str(n.montage));
        warn_text(w, &msg);
        return;
    };
    let dur = nd.dur;
    let sec = match nd.p.get("OnCompleteMontageSectionOverride").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() && s != "None" => {
            let sec = w.names.get(s);
            if sec.is_none() {
                // (the names are interned at bind: a section no montage of the data holds)
                let msg = format!("startChargeTotals: section {s} of {} is not a known name", w.names.str(n.montage));
                warn_text(w, &msg);
            }
            sec
        }
        _ => None,
    };
    let c = &mut w.fighter_mut(f).charge;
    let m = c.charge_attack_multiplier;
    if let Some(m) = m {
        c.charge_total_time = Some(m * crate::math::narrow(dur));
    }
    c.total_charged_time = c.charge_total_time.unwrap_or(crate::math::narrow(dur));
    if let Some(t) = &mut c.charge_timer {
        t.sec = sec;
    }
    let Some(m) = m else { return };
    if sec.is_none() || m == 1.0 {
        return;
    }
    let ticks = timer_ticks(c.charge_total_time);
    w.fighter_mut(f).charge.charge_timer = Some(ChargeTimer { left: -1.0, ticks, sec });
}

/// `chargeTimerTick`: the ChargeTimer in the frame's timer pass (World.step, behind
/// holdChargeTimer): pending in the frame it is set, then it fires after `ticks` passes (the timer rule of
/// the hold charge). EndChargeAttackServerFromTimer (@8655-8762): CalcChargeMultipliersServer(On Complete
/// Section Override, now), then UnbindChargeAttackInputClient. UpdateChargePercentMulticast(1.0) (@8807) is a
/// dedicated server's only.
/// Unverified: the order against Comp_HoldCharge's timer in the same pass.
pub fn charge_timer_tick(w: &mut World, f: FighterId) {
    let c = &mut w.fighter_mut(f).charge;
    let Some(t) = &mut c.charge_timer else { return };
    if t.left < 0.0 {
        t.left = t.ticks;
        return;
    }
    t.left -= 1.0;
    if t.left > 0.0 {
        return;
    }
    let sec = t.sec;
    c.charge_timer = None;
    release_charge(w, f, sec);
    // UnbindChargeAttackInputClient @8762
    w.fighter_mut(f).charge.charge_released = true;
}

/// `chargeFiled`: Comp_Pawn_ChargeAttack's AttackMontage (AssignChargeAttackValuesMulticast
/// @944): the montage a ChargingAttack passed at its begin, which is the active one of that moment whatever the
/// notify's own montage is (AnimNotify_ChargingAttack @1064-1158, @1888-1994). Kept only where it is an instance of
/// another slot group: instance 0's reset is `startMontage`'s (the Deviation of [`charge_blend_out`]).
pub fn charge_filed(w: &mut World, f: FighterId) {
    if w.fighter(f).montages.grouped.is_none() {
        return;
    }
    let a = active_instance(w, f);
    let Some((list, _)) = &mut w.fighter_mut(f).montages.grouped else { return };
    for (i, e) in list.iter_mut().enumerate() {
        let me = u32::try_from(i).ok().map(MontageSlot::Grouped);
        e.charge = me.is_some() && me == a && e.fade.is_none();
    }
}

/// `chargeBlendOut`: Comp_Pawn_ChargeAttack OnMontageBlendOut_Event (@5618-5781, then
/// @588-836), bound to the anim instance's OnMontageBlendingOut: only for AttackMontage,
/// ResetChargeAttackMultipliersServer (the multipliers 1, PercentCharged 0), ActiveAnim = null (no release from here
/// on) and the delegate removed. TotalChargedTime stays.
/// Deviation: for a DefaultGroup AttackMontage the port resets at the next `startMontage` instead; a fighter's own
/// read between that blend-out and the next montage start (a weapon attack's Hitframe, the HitFrame and Spawner
/// multipliers) takes the old values where the build's GetChargeValue gives 1, 1, 1, 0. A spawned actor's copy and a
/// move command's multiplier do not: instigatorCharge and chargeMovement make
/// GetChargeValue's AttackMontage test themselves.
pub fn charge_blend_out(w: &mut World, f: FighterId, e: &mut GroupedEntry) {
    if !e.charge {
        return;
    }
    e.charge = false;
    let c = &mut w.fighter_mut(f).charge;
    c.charge_mult = 1.0;
    c.charge_dmg_mult = 1.0;
    c.charge_buff_mult = 1.0;
    c.charge_pct = 0.0;
    c.charge_notify = -1;
}

/// `endChargeLoopSound`: AnimNotify_ChargingAttack Received_NotifyEnd ->
/// CompCharge.EndChargeLoopSound(Ak Stop Duration): ExecuteActionOnPlayingID(Stop, the last posted ID, duration ms,
/// linear), whether the notify has a sound or not. Then, while the ramp timer is valid: clear it,
/// SetRTPCValue(cha_charge_loop, ChargeValue, 0) and Delay(0.1) (a pending Delay is not restarted) ->
/// SetRTPCValue(cha_charge_loop, 0, 500 ms).
/// The key `id + ':charge' + seq` is the key's tag (`chargeSoundKey`, NONE for '') with the sequence
/// number `chargeSoundSeq` as its number.
pub fn end_charge_loop_sound(w: &mut World, f: FighterId, n: NotifyRef) {
    let stop = notify_def(&w.data, w, f, n).and_then(|nd| nd.p.get("Ak Stop Duration")?.as_f64()).unwrap_or(0.0);
    let rtpc = crate::name!("cha_charge_loop");
    let me = w.fighter_mut(f);
    let id = me.id;
    let ms = &mut me.montages;
    if !ms.charge_sound_key.is_none() {
        let key = SoundKey { tag: ms.charge_sound_key, n: ms.charge_sound_seq };
        me.events.push(Event::SoundStop {
            fighter: id,
            key,
            fade: Some(crate::math::narrow(stop / 1000.0)),
            ev: None,
            curve: None,
            cut: false,
        });
        me.montages.charge_sound_key = Name::NONE;
    }
    let ms = &mut me.montages;
    if ms.charge_ramp.is_none() {
        return;
    }
    ms.charge_ramp = None;
    let value = ms.charge_value;
    if ms.charge_ramp_delay <= 0.0 {
        ms.charge_ramp_delay = js::round(0.1 * 60.0);
    }
    me.events.push(Event::Rtpc { fighter: id, name: rtpc, value, interp_ms: 0.0 });
}

/// `tickChargeRamp`: Comp_Pawn_ChargeAttack ScaleSoundOverTimeMulticast's timer (the timer
/// rule of tickWallTimers): every 0.05 s v = NormalizeToRange(game time - SoundChargeStartTime, 0, TotalDuration -
/// 0.1); while v <= 1.1 it is stored in ChargeValue and set on the owner's cha_charge_loop (interpolation 0). Only
/// sfx_shared_combat_attack_charge_large / _medium read that parameter (pitch, make-up gain on LayerContainer
/// 921356648).
pub fn tick_charge_ramp(w: &mut World, f: FighterId) {
    let rtpc = crate::name!("cha_charge_loop");
    let me = w.fighter_mut(f);
    let id = me.id;
    let ms = &mut me.montages;
    if ms.charge_ramp_delay > 0.0 {
        ms.charge_ramp_delay -= 1.0;
        if ms.charge_ramp_delay == 0.0 {
            me.events.push(Event::Rtpc { fighter: id, name: rtpc, value: 0.0, interp_ms: 500.0 });
        }
    }
    let ms = &mut me.montages;
    let Some(r) = &mut ms.charge_ramp else { return };
    if r.pending {
        r.pending = false;
        return;
    }
    r.n += 1;
    if crate::consts::clock(crate::real(r.n)) <= r.at {
        return;
    }
    r.at += 0.05;
    let t = crate::real(r.n) * DELTA_SECONDS;
    // KismetMathLibrary::NormalizeToRange
    let v = if r.span == 0.0 { if t >= 0.0 { 1.0 } else { 0.0 } } else { t / r.span };
    if v > 1.1 {
        return;
    }
    ms.charge_value = v;
    me.events.push(Event::Rtpc { fighter: id, name: rtpc, value: v, interp_ms: 0.0 });
}

/// `chargeReleaseInput`: Comp_Pawn_ChargeAttack InputReleased_Event (@7635-7750), bound to
/// the fighter's button release, which has no gate (ButtonRelease, BaseFighter ubergraph 131535-131632): it also
/// releases under a hitpause (an armoured charge that takes a hit, a charge that runs over its own hitframe), and the
/// charge is measured in world time. StartAttackChargeClient reads the button when the notify begins (@2065-2120), so
/// the level test here is the release event.
pub fn charge_release_input(w: &mut World, f: FighterId) {
    let c = &w.fighter(f).charge;
    if c.charge_notify >= 0
        && owner_of(w, f, crate::real(c.charge_notify)).is_some()
        && !c.charge_released
        && charge_held(w, f) == Some(false)
    {
        release_charge(w, f, None);
    }
}

/// `chargeHeld`: BPI_IsButtonPressed(ActiveButton); `None` without a button the port
/// tracks.
pub fn charge_held(w: &World, f: FighterId) -> Option<bool> {
    let me = w.fighter(f);
    let inp = &me.controls.inp;
    Some(match me.branching.charge_btn? {
        ChargeBtn::Attack(b) => {
            let down = match b {
                AttackBtn::Attack => inp.attack,
                AttackBtn::Special => inp.special,
            };
            down && inp.force_release != Some(b)
        }
        ChargeBtn::Jump => inp.jump,
        ChargeBtn::Dodge => inp.dodge,
    })
}

/// `releaseCharge`. `over`: CalcChargeMultipliersServer's NameOverride (the ChargeTimer's
/// On Complete Section Override; a release's None).
/// Unverified: a ChargeTimer that fires with no charge the port resolves does nothing; the build's jumps the montage
/// then active and recomputes the stored multipliers (UpdateChargeValues puts them back at their Max). The timer
/// outlives its window with a multiplier above 1, which no class of the build gives, and also when the charge is cut
/// before it fires (a hit, a cancel: neither the NotifyEnd @5786 nor the blend-out reset @588-906 clears it), which
/// no ported montage with an OnComplete override and a charge buff reaches.
pub fn release_charge(w: &mut World, f: FighterId, over: Option<Name>) {
    // `chargeNotify` is a key: the charge of the instance that began it (the last one begun).
    let key = w.fighter(f).charge.charge_notify;
    if key < 0 {
        return;
    }
    let Some(r) = resolve(w, f, crate::real(key)) else { return };
    let (m, n) = (r.m, r.n);
    {
        let c = &mut w.fighter_mut(f).charge;
        c.charge_released = true;
        // SetChargeAttackForPlayer(idx, 0, 0, Reset, -1)
        c.set_charge_ui((0.0, 0.0));
    }
    // Comp_Pawn_ChargeAttack CalcChargeMultipliersServer: jump to NameOverride when it is a name, else to
    // MontageSectionOveride, or to the end of the current section when that is None too (@4085-4146); then
    // UpdateChargeValues = 1 + (Max - 1) * min(elapsed / total, 1), elapsed in game time since the notify began,
    // total = ChargeTotalTime (chargeTotal); then the ChargeTimer cleared (@5033).
    // The montage of both jumps is GetCurrentActiveMontage (Comp_Pawn_ChargeAttack @4438-4579 / @5424-5485 ->
    // Comp_Pawn_MontageNavigation JumpToSectionLogic @1492-1823 / JumpToEndOfSectionLogic @3795-4131): an instance of
    // another slot group played over the attack takes the jump and the attack charges on.
    // Unverified: a section name that montage has not is taken as no jump (stock Montage_JumpToSection).
    // The charge of an attack instance of another slot group: the active instance over both sets (a DefaultGroup
    // montage played after it takes the jump; none active: no jump).
    let t = if key >= KEY_STRIDE { active_instance(w, f) } else { active_grouped(w, f).map(grouped_slot).or(Some(m)) };
    let own = notify_def(&w.data, w, f, n)
        .and_then(|nd| nd.p.get("MontageSectionOveride")?.as_str().map(str::to_owned))
        .filter(|s| !s.is_empty());
    let over = over.map(|o| w.names.str(o).to_owned());
    let sec = over.or(own);
    let named = sec.as_deref().filter(|&s| s != "None");
    if let Some(t) = t {
        // (Montage_JumpToSection with no active montage: nothing.)
        with_player(w, f, t, |p| match named {
            Some(s) => {
                p.jump_to_section(s);
            }
            None => {
                let idx = p.s.section_index_at(p.m, p.s.pos);
                p.jump_to_section_end(idx);
            }
        });
    }
    // MontageNavigation's AdditionalMeshes: a Disguise's second mesh takes the same jump on its own montage
    if w.fighter(f).shapes.disguise.is_some() {
        let s = named.and_then(|s| w.names.get(s));
        mirror_section_jump(w, f, s);
    }
    update_charge_values(w, f, n);
    w.fighter_mut(f).charge.charge_timer = None;
}

/// `updateChargeValues`: UpdateChargeValues: TotalChargedTime = elapsed game time since
/// ChargeStartTime; each multiplier = 1 + (Max - 1) * min(elapsed / ChargeTotalTime, 1). `n`: the tracked charge (the
/// last begun).
pub fn update_charge_values(w: &mut World, f: FighterId, n: NotifyRef) {
    let data = w.data.clone();
    let nd = notify_def(&data, w, f, n);
    let dur = nd.map_or(0.0, |nd| nd.dur);
    let max = |k: &str| nd.and_then(|nd| nd.p.get(k)?.as_f64()).unwrap_or(1.0);
    let (kb, dmg, buff) = (max("MaxKnockBackMultiplier"), max("MaxDamageMultiplier"), max("MaxBuffStackMultiplier"));
    let total = charge_total(w, f, crate::math::narrow(dur));
    let me = w.fighter_mut(f);
    let elapsed = me.clock.time - me.charge.charge_start_time;
    let c = &mut me.charge;
    c.total_charged_time = elapsed;
    let frac = js::min(elapsed / total, 1.0);
    c.charge_pct = frac;
    c.charge_mult = crate::math::narrow(1.0 + (kb - 1.0) * f64::from(frac));
    c.charge_dmg_mult = crate::math::narrow(1.0 + (dmg - 1.0) * f64::from(frac));
    c.charge_buff_mult = crate::math::narrow(1.0 + (buff - 1.0) * f64::from(frac));
}

/// The notify `n` of the fighter's data (or of a montage its second meshes play: a Disguise copy's ChargingAttack).
pub fn notify_def<'a>(data: &'a GameData, w: &World, f: FighterId, n: NotifyRef) -> Option<&'a Notify> {
    crate::fighter::anim::montages::any_notify_in(data, w, f, n)
}

// ---- the call surface's argument and return types

/// `execute`'s 'branch' | 'jump' | 'dodge'.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecuteKind {
    Branch,
    Jump,
    Dodge,
}

#[cfg(test)]
mod tests {
    use super::{ChargeAttack, dir_num, dir_of};
    use crate::input::Dir;
    use crate::state::{StateHash, StateHasher};

    #[test]
    fn direction_bytes_round_trip() {
        for n in 0..10 {
            let d = dir_of(n).expect("a byte of EActionDirection");
            assert_eq!(dir_num(d), n);
        }
        assert_eq!(dir_of(10), None);
        assert_eq!(dir_of(-1), None);
        assert_eq!(dir_num(Dir::Neutral), 4);
    }

    /// An equal chargeUi written twice counts twice; the count is neither hashed nor restored.
    #[test]
    fn charge_ui_write_count() {
        let mut c = ChargeAttack::default();
        let mut h0 = StateHasher::default();
        c.hash_state(&mut h0);
        c.set_charge_ui((0.0, 0.0));
        c.set_charge_ui((0.0, 0.0));
        assert_eq!(c.charge_ui_writes, 2);
        let mut h1 = StateHasher::default();
        c.hash_state(&mut h1);
        assert_eq!(h0.finish(), h1.finish());
        let mut d = ChargeAttack::default();
        d.clone_from(&c);
        assert_eq!(d.charge_ui_writes, 0);
    }
}
