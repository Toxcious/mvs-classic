//! Armor, invulnerability and rage (the fighter's fields, written by Comp_Armor, Comp_Pawn_Rage and
//! the hit code).

use crate::Real;
use crate::State;
use crate::chars::CharState;
use crate::combat::buffs::{buff_of, buff_signal, end_buff};
use crate::combat::hit::timer_ticks;
use crate::consts::{
    DELTA_SECONDS, EXTRA_HUNGRY_RAGE_THRESHOLD, RAGE_BUFF_DURATION, RAGE_CHARGE_DECREMENT, RAGE_CHARGE_INCREMENT,
    RAGE_RESIDUAL_SCALE, RAGE_THRESHOLD,
};
use crate::events::{Event, UiStateOp, UiStateParams};
use crate::fighter::FighterId;
use crate::fighter::anim::{active_instance, cond, start_montage};
use crate::fighter::common::timer_frames;
use crate::fighter::control::{base_num, data_name, emit, pawn_comp_in, pawn_num, with_player};
use crate::fighter::match_side::{TempHealth, TutorialEvent, sfx, tutorial_event_of};
use crate::fighter::moves::{V2, armor_pack_buffed};
use crate::input::AttackBtn;
use crate::math as js;
use crate::names::Name;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::world::World;
use crate::world::pawns::match_fighters_iter;

/// `ArmorPack`.
#[derive(State, Debug, Default)]
pub struct ArmorPack {
    /// `type`.
    pub kind: Name,
    pub can_override: bool,
    pub amount: Real,
    pub max_hits: Real,
    pub threshold: Real,
    pub kb_res: Real,
    pub dmg_res: Real,
    pub hs_res: Real,
}

/// `timedArmor`: the armor packs with an expiry.
#[derive(State, Debug, Default)]
pub struct TimedArmor {
    /// Per slot: the pack's type and expiry.
    pub slots: Vec<Option<(Name, Real)>>,
    pub free: Vec<i32>,
    pub fire: Real,
    pub at: Real,
    pub dur: Real,
}

/// Armor.
#[derive(State, Debug, Default)]
pub struct Armor {
    pub armor_packs: Vec<ArmorPack>,
    pub armor_vis: Option<Real>,
    pub armor_hid: bool,
    pub timed_armor: Option<TimedArmor>,
}

/// Invulnerability and the damage gates.
#[derive(State, Debug, Default)]
pub struct Invuln {
    pub invuln_count: i32,
    pub invuln: Real,
    pub has_spawn_invuln: bool,
    pub damage_disabled: bool,
    pub projectile_armor: bool,
    pub light_projectile_armor: Option<bool>,
}

/// Rage (Comp_Pawn_Rage).
#[derive(State, Debug, Default)]
pub struct Rage {
    pub rage: Real,
    pub rage_enabled: bool,
    pub rage_charging: bool,
    /// `rageInput`: the main montage instance the charge began on (the release input bound while it is set).
    pub rage_input: Option<crate::fighter::anim::MontageKey>,
    pub rage_timer: Real,
    pub rage_filled: bool,
    pub rage_started: bool,
    pub rage_fx: bool,
    pub rage_residual: bool,
    pub rage_glow_shown: bool,
}

// ---- rage: Comp_Pawn_Rage

const RAGE_COMP: &str = "Comp_Pawn_Rage_GEN_VARIABLE";

/// GetRageThreshold (Comp_Pawn_Rage @0-286): BPI_CheckConditions([IsExtraHungryPerk_C, RockEquippedCondition_C])
/// selects ExtraHungryRageThreshold (1.5) over RageThreshold (2); read at every use. The perk's condition is asked
/// first, so a fighter with no Shaggy state never reaches the sandwich's.
fn rage_threshold(w: &mut World, f: FighterId) -> Real {
    let (perk, rock) = (crate::name!("IsExtraHungryPerk_C"), crate::name!("RockEquippedCondition_C"));
    if cond(w, f, perk) && cond(w, f, rock) { EXTRA_HUNGRY_RAGE_THRESHOLD } else { RAGE_THRESHOLD }
}

/// `rageReleaseInput`: Comp_Pawn_Rage InputReleased_Event (bound to the fighter's button
/// release; ubergraph 6126-6241: the released button is ActiveButton and ActiveAnim is valid, nothing else), so a
/// release under a hitpause counts too.
pub fn rage_release_input(w: &mut World, f: FighterId) {
    let me = w.fighter(f);
    let c = &me.controls;
    if me.rage.rage_input.is_some()
        && ((c.prev.special && !c.inp.special) || c.inp.force_release == Some(AttackBtn::Special))
    {
        rage_release(w, f);
    }
}

/// `dealtRage`: HitRageCalculation, the owner's Comp_Pawn_HitBoxHandler HitAnything, for a
/// fighter of another team.
pub fn dealt_rage(w: &mut World, f: FighterId, vic: FighterId, hit_box_damage: Real) {
    if crate::chars::iron_giant::giant_of(w, f).is_some() && w.fighter(vic).team.team != w.fighter(f).team.team {
        crate::chars::iron_giant::hover_rage_own_hit(w, f, hit_box_damage);
    }
}

/// `tickRage`: Comp_Pawn_Rage ReceiveTick. Charge and decay use GetTimeSeconds deltas
/// (NewTickTime - PreviousTickTime, in float32), so they run through hitpause. PreviousTickTime is stamped at the
/// charge start and end and on every charge or decay tick, so each tick's delta is the world clock's last frame step.
pub fn tick_rage(w: &mut World, f: FighterId) {
    let thr = rage_threshold(w, f);
    let me = w.fighter(f);
    let dt = me.clock.time - me.clock.prev_time;
    if me.rage.rage_charging {
        let inc = pawn_num(w, f, RAGE_COMP, "RageChargeIncrement").unwrap_or(RAGE_CHARGE_INCREMENT);
        let r = &mut w.fighter_mut(f).rage;
        r.rage = js::min(js::max(r.rage + (dt * inc), 0.0), thr);
    } else if me.rage.rage != thr && me.rage.rage > 0.0 {
        let r = &mut w.fighter_mut(f).rage;
        r.rage = js::min(js::max(r.rage - (dt * RAGE_CHARGE_DECREMENT), 0.0), thr);
        // Every decay tick with rage off: Enable Vfx Multicast -> EnableVFX(false).
        if !r.rage_enabled {
            rage_vfx(w, f, false);
        }
    }
    // The residual follows the meter, and goes once the meter is empty outside a charge or rage is on
    // (DestroyRageResidualMulticast).
    if w.fighter(f).rage.rage_residual {
        let r = &w.fighter(f).rage;
        if (r.rage <= 0.0 && !r.rage_charging) || r.rage_enabled {
            w.fighter_mut(f).rage.rage_residual = false;
            emit(w, f, |fighter| Event::RageResidual { fighter, scale: None });
        } else {
            let scale = residual_scale(w, f);
            emit(w, f, |fighter| Event::RageResidual { fighter, scale: Some(scale) });
        }
    }
    // AfterRageTimerExpired
    let r = &mut w.fighter_mut(f).rage;
    if r.rage_timer > 0.0 {
        r.rage_timer -= 1.0;
        if r.rage_timer == 0.0 && r.rage_enabled {
            end_rage(w, f);
        }
    }
    // Authority: a full meter runs EnableRage(true) once; dropping below the threshold re-arms it.
    if w.fighter(f).rage.rage == thr {
        if !w.fighter(f).rage.rage_filled {
            w.fighter_mut(f).rage.rage_filled = true;
            enable_rage(w, f, true);
            // @2115-2497: RockEquippedCondition and IsExtraHungryPerk again -> Comp_Spawner.UnequipObject(RockMesh).
            let (perk, rock) = (crate::name!("IsExtraHungryPerk_C"), crate::name!("RockEquippedCondition_C"));
            if cond(w, f, rock) && cond(w, f, perk) {
                let mesh = crate::name!("RockMesh");
                crate::fighter::unequip_object(w, f, mesh);
            }
        }
    } else {
        w.fighter_mut(f).rage.rage_filled = false;
    }
    // Last in the tick (authority): ShouldShowMaterialGlow follows RageCharging || RageEnabled, and each change runs
    // Enable Rage Material Effect Multicast (Emissive_Mult 1 / 0 on every skeletal mesh), so the glow of a charge goes
    // with the charge.
    let r = &mut w.fighter_mut(f).rage;
    let glow = r.rage_charging || r.rage_enabled;
    if glow != r.rage_glow_shown {
        r.rage_glow_shown = glow;
        emit(w, f, |fighter| Event::RageGlow { fighter, on: glow, shirt: false });
    }
}

/// `startRageCharge`: StartRageCharge (AnimNotify_RageCharge begin; PressedInput is
/// SpecialAttack on every Shaggy builder). Also sets the shirt's Emissive_Mult to 1 (rageGlow) and, with authority
/// and rage off, Create Rage Residual Multicast: unless one is still valid, PS_BruteRageAura_Hold on Spine3 at the
/// meter's residual scale.
pub fn start_rage_charge(w: &mut World, f: FighterId) {
    w.fighter_mut(f).rage.rage_charging = true;
    emit(w, f, |fighter| Event::RageGlow { fighter, on: true, shirt: true });
    let r = &w.fighter(f).rage;
    if !r.rage_enabled && !r.rage_residual {
        w.fighter_mut(f).rage.rage_residual = true;
        let scale = residual_scale(w, f);
        emit(w, f, |fighter| Event::RageResidual { fighter, scale: Some(scale) });
    }
    let me = w.fighter_mut(f);
    me.rage.rage_input = me.montages.montage.as_ref().map(|m| m.montage);
    me.rage.rage_started = true;
}

/// `residualScale`: VLerp(MinResidualScale, MaxResidualScale, RageChargeCurrent / threshold).
pub fn residual_scale(w: &mut World, f: FighterId) -> Real {
    let [lo, hi] = RAGE_RESIDUAL_SCALE;
    let thr = rage_threshold(w, f);
    lo + (hi - lo) * (w.fighter(f).rage.rage / thr)
}

/// `rageVfx`: EnableVFX: true posts the loop and spawns the eye and aura emitters (render
/// side); false destroys them, posting the stop for each. Its EnableRageMaterialEffect runs first either way: on every
/// skeletal mesh component's materials Emissive_Mult = 1 / 0, Team (unchanged from the skin's value) and HairSpeed (no
/// such parameter in Shaggy's materials).
pub fn rage_vfx(w: &mut World, f: FighterId, on: bool) {
    emit(w, f, |fighter| Event::RageGlow { fighter, on, shirt: false });
    if on {
        let ev = crate::name!("sfx_cha_shaggy_charge_loop");
        sfx(w, f, Some(ev));
        w.fighter_mut(f).rage.rage_fx = true;
        emit(w, f, |fighter| Event::RageVfx { fighter, on });
    } else if w.fighter(f).rage.rage_fx {
        let ev = crate::name!("sfx_cha_shaggy_charge_loop_stop");
        for _ in 0..3 {
            sfx(w, f, Some(ev));
        }
        w.fighter_mut(f).rage.rage_fx = false;
        emit(w, f, |fighter| Event::RageVfx { fighter, on });
    }
}

/// `endRageCharge`: EndRageChargeMulticast (notify end, or the AttackMontage blending out):
/// unbinds the release input.
pub fn end_rage_charge(w: &mut World, f: FighterId) {
    let r = &mut w.fighter_mut(f).rage;
    if !r.rage_charging {
        return;
    }
    r.rage_charging = false;
    r.rage_input = None;
}

/// `rageRelease`: InputReleased_Event -> JumpMontageServer/Multicast:
/// Montage_JumpToSectionsEnd on the current section. RageBuilderCancelMontage is proxied with no mesh, so nothing
/// plays. The SkipToTime(montage position) call goes to the fighter's ActiveNotifiesRollbackComponent (native,
/// FUN_1410e3310): it only rebuilds that component's list of active notify states and its QueuedBegin / QueuedEnd
/// arrays, which BaseFighter reads in GetMovementImpulsesForCurrentFrameCmd alone (the new movement / rollback tick,
/// off in this build's offline play), so it has no effect here.
/// The montage is GetCurrentActiveMontage (Comp_Pawn_Rage @7395; its current section @7437, the jump @7496): an
/// instance of another slot group played over the attack takes the jump, and with no active instance (the attack in
/// its own blend-out) nothing jumps.
pub fn rage_release(w: &mut World, f: FighterId) {
    let me = w.fighter(f);
    // Deviation: the instance the charge began on is not kept; the port keeps the montage's name, so a second play
    // of the builder started over the first one passes too.
    let Some(m) = me.montages.montage.as_ref() else { return };
    if Some(m.montage) != me.rage.rage_input {
        return;
    }
    if let Some(slot) = active_instance(w, f) {
        with_player(w, f, slot, |p| {
            let idx = p.s.section_index_at(p.m, p.s.pos);
            p.jump_to_section_end(idx);
        });
    }
}

/// `enableRage`: EnableRage(true): RageEnabled, OnRageEnabled, EnableRageVisuals(true),
/// ScoobySnack @206 (scoobySnack), RageBuffDuration timer. The RockMesh unequip is the tick's, behind this call
/// (tickRage). EnableRageVisuals(true) plays the success montage, then EnableVFX(true).
/// play_animation: EnableRage's PlayAnimation (true from the tick; Buff_Shaggy_DyingRage passes false @863):
/// EnableRageVisuals(false) @9046 skips the success montage and goes to EnableVFX(true) @7551.
pub fn enable_rage(w: &mut World, f: FighterId, play_animation: bool) {
    let data = w.data.clone();
    let Some(p) = pawn_comp_in(&data, w, f, RAGE_COMP) else { return };
    w.fighter_mut(f).rage.rage_enabled = true;
    // K2_SetTimerDelegate(RageBuffDuration, no loop) from the component's tick (243): pending for this frame, then it
    // fires at the timer tick of the 900th later frame (timerFrames), which tickRage runs at the next beginFrame.
    w.fighter_mut(f).rage.rage_timer = timer_frames(Some(RAGE_BUFF_DURATION));
    // The success montage replaces the builder (IsFalling picks the air one); the builder's RageCharge NotifyEnd then
    // ends the charge.
    let key =
        if w.fighter(f).body.on_floor { "RageBuilderGroundSuccessMontage" } else { "RageBuilderAirSuccessMontage" };
    let mont = if play_animation { p.get(key).and_then(|v| v.as_str()) } else { None };
    if let Some(mont) = mont.filter(|s| !s.is_empty()) {
        let n = data_name(w, mont);
        start_montage(w, f, n, &0, None);
    }
    rage_vfx(w, f, true);
    tutorial_event_of(w, f, TutorialEvent::UniqueCharacterMechanic0); // Comp_Pawn_Rage @6380, behind OnRageEnabled
    let last = p.get("LastSnackSpawnTime").and_then(|v| v.as_f64());
    scooby_snack(w, f, last.map(crate::math::narrow));
}

/// `scoobySnack`: ScoobySnack @8757 (authority): BPI_GetGameModeType != 4 (Tutorial) @8975
/// (a game mode without the interface: 0), then @2567-3322 for each player index i < GetPlayerCount,
/// GetByPlayerIndex(i)'s TeamIndex == GetByFighter(owner)'s -> NumTeammates + 1 (the owner counts; neither lookup's
/// Success is tested: no life, range or arena test), NumTeammates > 1 @3327 -> SpawnScoobySnack, NumTeammates = 0
/// @3389. SpawnScoobySnack @9424: GetGameTimeInSeconds - LastSnackSpawnTime > 1.0 @9499 -> LastSnackSpawnTime =
/// GetGameTimeInSeconds @9572, Comp_Spawner.CompSpawnActor(ScoobySnackBox_C, no attach, WorldTransforms,
/// MakeTransform(the owner's location, rot(0, 90, 0), scale 1), direction (0, 0, 0), InitialSpeed 0, MaxSpeed 0)
/// @9863. LastSnackSpawnTime starts at the component's data value (the Comp_Pawn_Rage CDO and the pawn's template:
/// -1.0). The players are matchFighters less the minions (no player data).
// Deviation: CompSpawnActor runs in the rage component's tick; the request is spawned in the world's spawn pass of
// the owner, later in the same frame (as a notify's spawn).
fn scooby_snack(w: &mut World, f: FighterId, data_last: Option<Real>) {
    let CharState::Shaggy(s) = &w.fighter(f).cs else { return };
    let last = s.last_snack_spawn;
    if crate::modes::tutorial::running(w) {
        return;
    }
    let team = w.fighter(f).team.team;
    let mut mates = 0;
    for x in match_fighters_iter(w) {
        let o = w.fighter(x);
        if !o.life.not_player_character && o.team.team == team {
            mates += 1;
        }
    }
    if mates <= 1 {
        return;
    }
    let now = w.fighter(f).clock.time;
    // The read is `s.lastSnackSpawn ?? p.LastSnackSpawnTime`: an absent data value is NaN in the subtraction.
    let from = last.or(data_last).unwrap_or(Real::NAN);
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the test !(x > 1.0) is kept: a NaN gap spawns nothing")]
    if !((now - from) > 1.0) {
        return;
    }
    if let CharState::Shaggy(s) = &mut w.fighter_mut(f).cs {
        s.last_snack_spawn = Some(now);
    }
    crate::chars::shaggy::snack_box::request_snack_box(w, f);
}

/// `endRage`: EndRageMulticast: RageEnabled false and the meter emptied.
pub fn end_rage(w: &mut World, f: FighterId) {
    rage_vfx(w, f, false);
    let r = &mut w.fighter_mut(f).rage;
    r.rage_enabled = false;
    r.rage = 0.0;
}

/// `resetRage`: OnFighterReset (ResetFighterLogic's delegate; spawn stands in for it).
pub fn reset_rage(w: &mut World, f: FighterId) {
    let r = &mut w.fighter_mut(f).rage;
    r.rage_charging = false;
    r.rage_started = false;
    r.rage_input = None;
    r.rage = 0.0;
    if r.rage_enabled {
        r.rage_timer = 0.0;
        rage_vfx(w, f, false);
    }
    w.fighter_mut(f).rage.rage_enabled = false;
}

// ---- armor: Comp_Armor's packs and the temp health packs

/// `addArmorPack`: Comp_Armor HandleAddArmorPack: a pack that cannot override replaces the
/// last pack of its type (the loop @176-558 has no break: LocalOverrideIndex @520 is written at every match,
/// Array_Set @573 at the last). HandleAddArmorPack: the first pack added (not an override) adds the Armor state.
/// Each added pack (not the override path, @319-644) then calls Comp_FighterVisibilityManager.AddArmorVisibility
/// (@1629; MeshesSet is true from the spawn in the port, so never QueuedArmorVisuals): `armor_vis` + 1, the
/// ArmorVisibilityCount that shows the armor overlay copies; its 0 -> 1 shows the copies a SetupInitialVisibility hid
/// (@106-701: `armor_hid`).
pub fn add_armor_pack(w: &mut World, f: FighterId, a: &ArmorPack) {
    let a = armor_pack_buffed(w, f, a); // @76 ApplyArmorBuffsToArmorPack
    let armor_state = armor_state();
    let me = w.fighter_mut(f);
    let ar = &mut me.armor;
    let mut i = None;
    if !a.can_override {
        for (k, p) in ar.armor_packs.iter().enumerate() {
            if p.kind == a.kind {
                i = Some(k);
            }
        }
    }
    if let Some(i) = i {
        ar.armor_packs[i] = a;
    } else {
        ar.armor_packs.push(a);
        if ar.armor_packs.len() == 1 {
            me.controls.ui_state_ops.push(UiStateOp::Add(armor_state));
        }
        let v = ar.armor_vis.unwrap_or(0.0) + 1.0;
        ar.armor_vis = Some(v);
        if v == 1.0 {
            ar.armor_hid = false;
        }
    }
}

/// `addTimedArmorPack`: Comp_Armor AddTimedArmorPack @3154 (its one caller in the build is
/// Buff_DodgeArmor): MulticastAddArmor(pack) @3226 (HandleAddArmorPack: addArmorPack), TimedArmorPack[type] =
/// GetGameTimeInSeconds + Duration @3324, then one ArmorTimer for the component: set for Duration when it is not
/// active @3488, else cleared and set again for Duration when Duration < its remaining time @3611-3679 (a later,
/// longer pack does not move it). The map is a TMap: a removed key's slot is reused by the next add, the last freed
/// first, and its walk is in slot order. A world timer on the world clock, set in a tick group: timerFrames.
pub fn add_timed_armor_pack(w: &mut World, f: FighterId, a: &ArmorPack, dur: Real) {
    add_armor_pack(w, f, a);
    let me = w.fighter_mut(f);
    let now = me.clock.time;
    let t = me.armor.timed_armor.get_or_insert_with(|| TimedArmor { fire: Real::INFINITY, ..Default::default() });
    let exp = now + dur;
    let found = t.slots.iter().position(|s| s.is_some_and(|(k, _)| k == a.kind));
    let i = match found {
        Some(i) => i,
        None => match t.free.pop() {
            Some(i) => usize::try_from(i).expect("a free slot index"),
            None => t.slots.len(),
        },
    };
    if i >= t.slots.len() {
        t.slots.resize(i + 1, None);
    }
    t.slots[i] = Some((a.kind, exp));
    if t.fire == Real::INFINITY || dur < (t.at + t.dur - now) {
        arm_armor_timer(t, now, dur, timer_frames(Some(dur)));
    }
}

fn arm_armor_timer(t: &mut TimedArmor, now: Real, dur: Real, frames: Real) {
    t.fire = frames;
    t.at = now;
    t.dur = dur;
}

/// `tickTimedArmor`: the ArmorTimer's count, per fighter tick (beside the TempHealthTimer's);
/// its GetGameTimeInSeconds is the frame's the timer tick belongs to (prevTime, as the TempHealthTimer's).
pub fn tick_timed_armor(w: &mut World, f: FighterId) {
    let me = w.fighter_mut(f);
    let prev = me.clock.prev_time;
    let Some(t) = me.armor.timed_armor.as_mut() else { return };
    if t.fire == Real::INFINITY {
        return;
    }
    t.fire -= 1.0;
    if t.fire > 0.0 {
        return;
    }
    t.fire = Real::INFINITY;
    remove_armor_timers(w, f, prev);
}

/// `removeArmorTimers`: Comp_Armor RemoveArmorTimers: the map's keys walked from the last
/// slot; an entry whose time is <= now is Map_Removed and gets MulticastRemoveArmor(type) (HandleRemoveArmorPack:
/// every pack of that type); the others give the least time left, and the timer is set again for it @1121 (inside
/// the timer tick: timerTicks).
fn remove_armor_timers(w: &mut World, f: FighterId, now: Real) {
    let mut next = Real::INFINITY;
    let n = w.fighter(f).armor.timed_armor.as_ref().map_or(0, |t| t.slots.len());
    for i in (0..n).rev() {
        let t = w.fighter_mut(f).armor.timed_armor.as_mut().expect("the timed armor");
        let Some((kind, exp)) = t.slots[i] else { continue };
        if exp <= now {
            t.slots[i] = None;
            t.free.push(i32::try_from(i).expect("a slot index"));
            remove_armor(w, f, kind);
        } else {
            next = js::min(next, exp - now);
        }
    }
    if next != Real::INFINITY {
        let t = w.fighter_mut(f).armor.timed_armor.as_mut().expect("the timed armor");
        arm_armor_timer(t, now, next, timer_ticks(Some(next)));
    }
}

/// `removeArmor`: HandleRemoveArmorPack: each removal that leaves no pack removes the Armor
/// state; each removed pack calls RemoveArmorVisibility (@1607: the count - 1 when above 0, VisibilityManager :473).
/// CalculateArmor's dropped pack never comes here, so the count can stay above 0 with no pack left: the build's
/// behaviour, kept.
pub fn remove_armor(w: &mut World, f: FighterId, kind: Name) {
    let armor = crate::name!("Armor");
    let me = w.fighter_mut(f);
    while let Some(i) = me.armor.armor_packs.iter().position(|x| x.kind == kind) {
        me.armor.armor_packs.remove(i);
        if me.armor.armor_packs.is_empty() {
            me.controls.ui_state_ops.push(UiStateOp::Remove(armor));
        }
        if let Some(v) = me.armor.armor_vis
            && v > 0.0
        {
            me.armor.armor_vis = Some(v - 1.0);
        }
    }
}

/// `calculateArmor`: Comp_Armor CalculateArmor. Inputs are the hit's charged damage,
/// BaseKnockBack and HitStunMult; the attacker's charge KB multiplier only enters the threshold check. Nothing
/// without a pack or with a dealer of the owner's team (@237-428: every caller here is an enemy's hit).
/// The loop counts from 0 over CurrentArmorPacks and reads its length again each pass (@702):
/// - a pack breaks (@1303) on a piercing hit (@845) or when a check fails: hitsLeft = HitStun == 0 ? MaxHits :
///   MaxHits - 1 with MaxHits == 0 or hitsLeft > 0 (@3076), ArmorAmount - damage > 0 (@3238), threshold > kb x the
///   charge multiplier or threshold == 0 (@3485); a pack that passes gives its resistances (the least of each,
///   @3558-4063) and breaks all the same with hitsLeft == 1 (@4298), else it goes to LocalArmorPackUpdates with
///   amount - damage and MaxHits = Max(hitsLeft, 0) (@4871-5205).
/// - a break: Array_Get(i) @1442, broadcast ArmorBreak(ArmorType, DamageDealer, ArmorPierce) @1501, Array_Get(i)
///   again @1556 and RemoveArmor(type) @1615, whose HandleRemoveArmorPack takes every pack of that type out of
///   CurrentArmorPacks in the call; then the counter + 1 (@2679). So the pack that slid into index i is never read:
///   no check, no resistance, no broadcast, and @2163 (CurrentArmorPacks = LocalArmorPackUpdates) drops it. An
///   Array_Get past the end (a bound function or the buff below took packs out) leaves a default item: type None, of
///   which RemoveArmor finds none.
/// - on a piercing hit, ahead of the break: CheckHasBuff(Buff_NullifyNextHit_C) @930 -> RemoveBuff(that class, 0)
///   @1167 (Comp_State_Manager RemoveBuff: K2_DestroyActor @986, so its EndPlay runs in the call).
///
/// After the loop the outputs x FClamp(best, 0, 1) (@1734-2067), the packs become the updates (@2163), and with none
/// left ArmorBreakMulticast @2406 and RemoveStateForPlayer(Armor) @2626 (again, after HandleRemoveArmorPack's).
/// dealer: the hit's DamageDealer when it is a fighter, else none (as hitByEnemy). With packs left
/// CallArmorHitMulticast(LocalHitLocation) @2297, else ArmorBreakMulticast @2406: the events armorHit (at: the
/// HitLocation) and armorBreak. ArmorBreakMulticast's RemoveArmor over the packs left (U@2751) finds none here.
/// A pack that holds calls the owner's BPI_DecrementShieldCharge (@4529) before its update is made from the pack read
/// again.
// Unverified: a buff actor's light hit (buffActorLightHit) passes no `at` and the victim's actor location is taken
// (the HitLocation of its pfgDoDamage with a null HitBox is not read).
#[allow(clippy::too_many_arguments, reason = "the call keeps the build function's parameters")]
pub fn calculate_armor(
    w: &mut World,
    f: FighterId,
    damage: Real,
    kb: Real,
    hs: Real,
    chrg_kb_mult: Real,
    pierce: bool,
    dealer: Option<FighterId>,
    at: Option<V2>,
) -> CalculateArmorOut {
    if w.fighter(f).armor.armor_packs.is_empty() {
        return CalculateArmorOut { damage, kb, hs };
    }
    // @237-428: the packs are used only when DamageDealerTeamIndex != own TeamIndex; else @1648 passes all through.
    if let Some(d) = dealer
        && w.fighter(d).team.team == w.fighter(f).team.team
    {
        return CalculateArmorOut { damage, kb, hs };
    }
    let none = crate::name!("None");
    let (mut dmg_res, mut kb_res, mut hs_res): (Real, Real, Real) = (1.0, 1.0, 1.0);
    let mut updates: Vec<ArmorPack> = Vec::new();
    let mut i = 0;
    while i < w.fighter(f).armor.armor_packs.len() {
        let mut holds = false;
        if pierce {
            let nb = crate::name!("Buff_NullifyNextHit_C");
            if let Some(b) = buff_of(w, f, nb) {
                end_buff(w, f, b);
            }
        } else {
            let a = &w.fighter(f).armor.armor_packs[i];
            let hits_left = if hs == 0.0 { a.max_hits } else { a.max_hits - 1.0 };
            if (a.max_hits == 0.0 || hits_left > 0.0)
                && a.amount - damage > 0.0
                && (a.threshold > kb * chrg_kb_mult || a.threshold == 0.0)
            {
                dmg_res = js::min(dmg_res, a.dmg_res);
                hs_res = js::min(hs_res, a.hs_res);
                kb_res = js::min(kb_res, a.kb_res);
                // 1: absorbs this hit, then breaks.
                if hits_left != 1.0 {
                    // @4351-4529: the owner's BPI_DecrementShieldCharge, then the pack is read again (@4600): the
                    // call can take packs out (Steven's last charge removes his shield's type), and an index past the
                    // end reads the default item (type None, every number 0).
                    let h = w.fighter(f).hooks.char.decrement_shield_charge;
                    if let Some(h) = h {
                        h(w, f);
                    }
                    let b = w
                        .fighter(f)
                        .armor
                        .armor_packs
                        .get(i)
                        .cloned()
                        .unwrap_or(ArmorPack { kind: none, ..Default::default() });
                    let left = if hs == 0.0 { b.max_hits } else { b.max_hits - 1.0 };
                    updates.push(ArmorPack { amount: b.amount - damage, max_hits: js::max(left, 0.0), ..b });
                    holds = true;
                }
            }
        }
        if !holds {
            // @1303 -> @1403: every break, a pierce (with or without Buff_NullifyNextHit, @1082) or a failed hold test.
            tutorial_event_of(w, f, TutorialEvent::ArmorBreak);
            let kind = w.fighter(f).armor.armor_packs.get(i).map_or(none, |p| p.kind);
            armor_break(w, f, kind, dealer, pierce);
            let kind = w.fighter(f).armor.armor_packs.get(i).map_or(none, |p| p.kind);
            remove_armor(w, f, kind);
        }
        i += 1;
    }
    let left = !updates.is_empty();
    w.fighter_mut(f).armor.armor_packs = updates;
    if left {
        let (x, y) = match at {
            Some(p) => (p.x, p.y),
            None => (w.fighter(f).body.x, crate::fighter::actor_y(w, f)),
        };
        emit(w, f, |fighter| Event::ArmorHit { fighter, x, y });
        armor_shake_play(w, f); // ... broadcast ArmorHit() @1632 -> BaseFighter NoHitStunHIt
    } else {
        emit(w, f, |fighter| Event::ArmorBreak { fighter });
        let armor = crate::name!("Armor");
        w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(armor)); // ArmorBreakMulticast, RemoveStateForPlayer
    }
    let c = |v: Real| js::min(js::max(v, 0.0), 1.0);
    // Multiply_FloatFloat each: float32.
    CalculateArmorOut { damage: (damage * c(dmg_res)), kb: (kb * c(kb_res)), hs: (hs * c(hs_res)) }
}

// ---- BaseFighter ArmorShake (every fighter)
// Comp_Armor's ArmorHit delegate (CallArmorHitMulticast @2297 -> ubergraph @1146, `broadcast ArmorHit()` @1632) is
// bound by BaseFighter ReceiveBeginPlay to its event NoHitStunHIt (EX_BindDelegate @124494, EX_AddMulticastDelegate
// @124517), which plays the ArmorShake timeline from the start (ubergraph @148319). Its update (@189780 ->
// @148352-148471) sets the Mesh RelativeLocation to (track, 0, the current Z): `mesh_x`, Z untouched; its Finished
// (@15-130) to (0, 0, the current Z). The timeline (ArmorShake_Template): length 0.2, the float track ArmorShake on
// CurveFloat_1, no loop / auto-play / ignore-dilation stored.
// PlayFromStart fires the update at position 0 in the call (mesh X 40; Unverified: UE 4.26 FTimeline::PlayFromStart,
// as Garnet's aim and shockwave timelines and the Batcave fans). Its tick (UTimelineComponent, TG_PrePhysics)
// advances by the owner's dilated DeltaSeconds (a hit pause: x 0.005) in float32 and finishes only past the length
// (FTimeline::TickTimeline 142e46330): the update at 0.2 (track 0), then Finished (X 0).
// Unverified: the timeline's order against the pawn's ReceiveTick (HandleHitShake) and the mesh inside TG_PrePhysics is
// not fixed by code; taken as after ReceiveTick (the pawn tick's end, the Garnet AimResetTimeline and Batcave fans'
// choice), so in a frame where both write (an armor pack with hitstun resistance above 0) the shake's X is the one
// the frame ends with; BFHitPauseEnd (the timer pass, ahead of the ticks) writes X 0 before it.
// Buff_Ice HitShakeMulticast @5614 -> NoHitStunHIt @5724 never runs (AllyBreakThreshold 1).

/// CurveFloat_1 [time, value, arrive, leave]: keys 0..2 RCIM_Cubic, key 3 RCIM_Linear (no segment after it).
const ARMOR_SHAKE_KEYS: [[Real; 4]; 4] = [
    [0.0, 40.0, 0.0, 0.0],
    [0.06454508, -25.0, -206.84854, -206.84854],
    [0.12086138, 15.0, 62.878998, 62.876312],
    [0.2, 0.0, 0.0, 0.0],
];

/// `ARMOR_SHAKE_LENGTH`: fround(0.2).
const ARMOR_SHAKE_LENGTH: Real = 0.2_f32 as Real;

/// `armorShakeCurve`: FRichCurve::Eval: a cubic segment is the Bezier of P0, P0 + Leave x
/// gap / 3, P3 - Arrive x gap / 3, P3 (no tangent weights); constant past the ends.
fn armor_shake_curve(t: Real) -> Real {
    let k = &ARMOR_SHAKE_KEYS;
    if t <= k[0][0] {
        return k[0][1];
    }
    if t >= k[k.len() - 1][0] {
        return k[k.len() - 1][1];
    }
    let mut i = 0;
    while k[i + 1][0] <= t {
        i += 1;
    }
    let (a, b) = (k[i], k[i + 1]);
    let gap = b[0] - a[0];
    let u = (t - a[0]) / gap;
    let p1 = a[1] + (a[3] * gap) / 3.0;
    let p2 = b[1] - (b[2] * gap) / 3.0;
    let v = 1.0 - u;
    v * v * v * a[1] + 3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u * b[1]
}

/// `armorShakePlay`: NoHitStunHIt: ArmorShake.PlayFromStart.
pub fn armor_shake_play(w: &mut World, f: FighterId) {
    let h = &mut w.fighter_mut(f).hit;
    h.armor_shake = Some(0.0);
    h.mesh_x = armor_shake_curve(0.0);
}

/// `armorShakeTick`: the timeline's component tick (after ReceiveTick); dilation: the hit
/// pause's (0.005) or 1.
pub fn armor_shake_tick(w: &mut World, f: FighterId, dilation: Real) {
    let h = &mut w.fighter_mut(f).hit;
    let Some(s) = h.armor_shake else { return };
    let p = s + (DELTA_SECONDS * dilation);
    if p > ARMOR_SHAKE_LENGTH {
        h.mesh_x = 0.0; // the update at the length (track 0), then ArmorShake__FinishedFunc (X 0)
        h.armor_shake = None;
        return;
    }
    h.armor_shake = Some(p);
    h.mesh_x = armor_shake_curve(p);
}

/// `armorBreak`: Comp_Armor's ArmorBreak(ArmorType, DamageDealer, ArmorPierce) delegate
/// (CalculateArmor @1501), for what binds it: the pawn (`armor_break`) and the fighter's registered buffs.
// Unverified: the order of the bound functions (pawn, then buffs) is the binding order in the build and is not read.
fn armor_break(w: &mut World, f: FighterId, kind: Name, dealer: Option<FighterId>, pierce: bool) {
    let h = w.fighter(f).hooks.char.armor_break;
    if let Some(h) = h {
        h(w, f, kind, dealer, pierce);
    }
    let arg = BuffSignalArg { pack: Some(kind), dealer, pierce: Some(pierce), ..Default::default() };
    buff_signal(w, f, BuffSignal::ArmorBreak, arg);
}

/// `addTempHealthPack`: Comp_Armor AddTempHealthPack: the first pack of the same
/// TempHealthType is removed, its HealthAmount added to the new one unless the new one has CanOverride (@514-733,
/// @1189-1348); with MaxTempHealth != 0 (75 on Pawn_C017, 0 in the component's CDO) the amount is capped to
/// MaxTempHealth - the total of the packs left (@734-939); Map_Add (the freed slot is reused, so the pack keeps its
/// place) with GetGameTimeInSeconds as its value, after the broadcast of TempHealthAdded(type) @975; then
/// TempHelthTimerFunction() @1100, called here in a tick group (the timer it sets is pending).
// Deviation: UpdateTempHealthVisuals (the HUD's grey health) is not ported.
pub fn add_temp_health_pack(w: &mut World, f: FighterId, kind: Name, amount: Real, can_override: bool, time: Real) {
    let i = w.fighter(f).stats.temp_health.iter().position(|pk| pk.kind == kind);
    let mut a = amount;
    if let Some(i) = i
        && !can_override
    {
        a += w.fighter(f).stats.temp_health[i].amount;
    }
    let max = pawn_num(w, f, "Comp_Armor_GEN_VARIABLE", "MaxTempHealth")
        .or_else(|| base_num(w, f, "Comp_Armor_GEN_VARIABLE", "MaxTempHealth"))
        .unwrap_or(0.0);
    if max != 0.0 {
        let mut total: Real = 0.0;
        for (j, pk) in w.fighter(f).stats.temp_health.iter().enumerate() {
            if Some(j) != i {
                total += pk.amount;
            }
        }
        if a > (max - total) {
            a = max - total;
        }
    }
    buff_signal(w, f, BuffSignal::TempHealthAdded, BuffSignalArg { pack: Some(kind), ..Default::default() });
    // The running timer stays as it is when the function sets none (below), so the new pack shares its count.
    let me = w.fighter_mut(f);
    let fire = me.stats.temp_health.first().map_or(Real::INFINITY, |pk| pk.fire);
    let pk = TempHealth { kind, amount: a, time, added: me.clock.time, fire };
    match i {
        Some(i) => me.stats.temp_health[i] = pk,
        None => me.stats.temp_health.push(pk),
    }
    let now = me.clock.time;
    temp_health_timer(w, f, now, false);
}

/// `tempHealthTimer`: Comp_Armor TempHelthTimerFunction (called by AddTempHealthPack and by
/// its own TempHealthTimer), now its GetGameTimeInSeconds: nothing without a pack (@170). Each pack with Time > 0 whose
/// Time <= now - its add time, or within 0.001 of it (NearlyEqual) (@677-1068), is removed (@1141) with a broadcast of
/// TempHealthPackRemoved (@1260); the others with Time > 0 give LocalNextTime = the least Time - (now - add time),
/// from 1e12 (@1563-2024). Then UpdateTempHealthVisualsServer @1298; with a pack left and LocalNextTime <= 10000,
/// K2_SetTimer(TempHelthTimerFunction, LocalNextTime) @2052 (else the timer is left as it is @1467); with none the
/// timer is cleared @1468. in_timer: set inside the timer tick, so active at once (timerTicks).
pub fn temp_health_timer(w: &mut World, f: FighterId, now: Real, in_timer: bool) {
    if w.fighter(f).stats.temp_health.is_empty() {
        return;
    }
    let mut next: Real = 1e12;
    let mut gone = Vec::new();
    w.fighter_mut(f).stats.temp_health.retain(|pk| {
        let el = now - pk.added;
        if pk.time > 0.0 && (pk.time <= el || (pk.time - el).abs() < 0.001) {
            gone.push(pk.kind);
            return false;
        }
        if pk.time > 0.0 {
            next = js::min(next, pk.time - el);
        }
        true
    });
    for t in gone {
        buff_signal(w, f, BuffSignal::TempHealthPackRemoved, BuffSignalArg { pack: Some(t), ..Default::default() });
    }
    temp_health_visuals(w, f);
    if !w.fighter(f).stats.temp_health.is_empty() && next <= 10000.0 {
        let n = if in_timer { timer_ticks(Some(next)) } else { timer_frames(Some(next)) };
        for pk in &mut w.fighter_mut(f).stats.temp_health {
            pk.fire = n;
        }
    }
}

/// `tempHealthVisuals`: Comp_Armor UpdateTempHealthVisualsServer (ubergraph @3035):
/// CaculateTempHealth, and with a total <= 0 the broadcast of TempHealthFullyRemoved @3106 (then
/// UpdateTempHealthVisuals @3125, not ported).
pub fn temp_health_visuals(w: &mut World, f: FighterId) {
    let mut total: Real = 0.0;
    for pk in &w.fighter(f).stats.temp_health {
        total += pk.amount;
    }
    if total <= 0.0 {
        buff_signal(w, f, BuffSignal::TempHealthFullyRemoved, BuffSignalArg::default());
    }
}

/// `removeTempHealthPack`: Comp_Armor RemoveTempHealthPack: the first pack of that type,
/// with a broadcast of TempHealthPackRemoved @580 ahead of its Map_Remove @676; UpdateTempHealthVisualsServer @748
/// with or without one.
pub fn remove_temp_health_pack(w: &mut World, f: FighterId, kind: Name) {
    if let Some(i) = w.fighter(f).stats.temp_health.iter().position(|pk| pk.kind == kind) {
        buff_signal(w, f, BuffSignal::TempHealthPackRemoved, BuffSignalArg { pack: Some(kind), ..Default::default() });
        w.fighter_mut(f).stats.temp_health.remove(i);
    }
    temp_health_visuals(w, f);
}

/// `absorbTempHealth`: Comp_Armor CalculateTempHealthDamage, called by Comp_Actor_Damage
/// OnTakeAnyDamage (ubergraph 546-745) on the amount of ApplyDamage: what it returns is what CurrentDamage gains and
/// what RecordDamage gets. With Damage > 0 (or no pack) each pack in turn: HealthAmount - damage <= 0 removes it and
/// leaves the absolute rest as the damage (@547-1043), else the pack keeps the difference and the damage is 0
/// (@1169-1779). A heal (negative) passes unchanged (@197). A removed pack is broadcast (TempHealthPackRemoved
/// @1006), and after the packs UpdateTempHealthVisualsServer @1076 (without a pack too: nothing listens that could
/// tell).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the test !(damage > 0) is kept: a NaN damage passes unchanged")]
pub fn absorb_temp_health(w: &mut World, f: FighterId, damage: Real) -> Real {
    if w.fighter(f).stats.temp_health.is_empty() || !(damage > 0.0) {
        return damage;
    }
    let mut d = damage;
    let mut gone = Vec::new();
    w.fighter_mut(f).stats.temp_health.retain_mut(|pk| {
        let left = pk.amount - d;
        if left <= 0.0 {
            d = left.abs();
            gone.push(pk.kind);
            false
        } else {
            pk.amount = left;
            d = 0.0;
            true
        }
    });
    for t in gone {
        buff_signal(w, f, BuffSignal::TempHealthPackRemoved, BuffSignalArg { pack: Some(t), ..Default::default() });
    }
    temp_health_visuals(w, f);
    d
}

/// `ARMOR_STATE`: Comp_Armor HandleAddArmorPack's params.
fn armor_state() -> UiStateParams {
    UiStateParams {
        id: crate::name!("Armor"),
        state_type: 2,
        display_type: 0,
        current_value: -1.0,
        max_value: -1.0,
        timer: -1.0,
        icon_material: crate::name!("MI_State_Armor"),
        active: true,
        pop_text: Name::NONE,
        auto_remove_time: -1.0,
        subicon: None,
    }
}

// ---- the call surface's argument and return types

/// What `calculateArmor` takes or gives (a struct literal).
#[derive(Clone, Debug)]
pub struct CalculateArmorOut {
    pub damage: Real,
    pub kb: Real,
    pub hs: Real,
}

#[cfg(test)]
mod tests {
    use super::{ARMOR_SHAKE_KEYS, armor_shake_curve};

    #[test]
    fn armor_shake_curve_holds_its_keys() {
        for k in ARMOR_SHAKE_KEYS {
            assert_eq!(armor_shake_curve(k[0]), k[1]);
        }
        assert_eq!(armor_shake_curve(-1.0), 40.0);
        assert_eq!(armor_shake_curve(1.0), 0.0);
        // between the keys: inside the hull of the segment's Bezier points, a float32
        let v = armor_shake_curve(0.1);
        assert!(v > -25.0 && v < 15.0, "{v}");
        assert_eq!(v, v);
    }
}
