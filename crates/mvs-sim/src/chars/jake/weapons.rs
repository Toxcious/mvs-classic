//! Comp_WeaponSwap and the Weapon Master perk: the pawn's weapon component
//! (`JakeState::weapon`, `weapon_equipped`, `weapons_hidden`), the notifies WeaponSwap and WeaponHideCurrent,
//! Buff_Jake_WeaponMaster ([`MasterExt`]) and Buff_Jake_WeaponMasterAlly. AnimNotify_PopText is the shared notifies'.
//!
//! Jake's weapons: Comp_WeaponSwap (CDO HoldSlotName R_HoldPoint, MeshTag JakeWeapon): CycleWeapon, EquipIfUnarmed /
//! UnEquipIfArmed, TempHidAllWeapons, UnhidEquippedWeapon, WeaponVisibilityMulticast, SFX_Weapon_Swap; the WeaponMaster
//! calls @2263 / @2760. AnimNotify_WeaponSwap / AnimNotify_WeaponHideCurrent. Buff_Jake_WeaponMaster /
//! Buff_Jake_WeaponMasterAlly.
//!
//! The weapons: Pawn_Jake's Weapon1-3, skeletal mesh components tagged JakeWeapon1-3 (and JakeWeapon), bVisible false:
//! sub-meshes of the skin (jakeweapon1-3, hidden at the start), shown and hidden through
//! Comp_FighterVisibilityManager.SetSubMeshVisibility (`set_sub_mesh`, what the view draws).
//! `JakeState::weapon` / `weapon_equipped` / `weapons_hidden` mirror the component's CurrentWeapon / WeaponEquiped and
//! the TempHid / Unhid pair.
//! - BeginPlay @860: MirrorWeaponSide bound to the owner's FacingChanged @955-978 (FindMirrorSocket(HoldSlotName) and a
//!   relative rotation per facing @344-746: the view's attach, not the sim's); AddedWeapons attached @15-592;
//!   WeaponEquiped = false @1075 (`JakeState`'s default).
//! - MirrorWeaponSide re-attaches at every FacingChanged (SetRotationLogic broadcasts it at every facing set): drawing
//!   only.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::buffs::{buff, buff_mut, buff_of, inst_def_of, set_buff_values, spawn_buff};
use crate::combat::hit::timer_ticks;
use crate::combat::{BuffClass, BuffExt, BuffRef};
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::set_sub_mesh;
use crate::fighter::common::buff_def;
use crate::fighter::match_side::notify_props;
use crate::fighter::{FighterId, actor_y};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::{flag, flag_or, num, num_or, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use mvs_data::value::{Props, Value};

use super::state::{jake_of, jake_w};

/// Buff_Jake_WeaponMaster's class.
pub const WEAPON_MASTER: &str = "Buff_Jake_WeaponMaster_C";

/// Buff_Jake_WeaponMasterAlly's class.
pub const WEAPON_MASTER_ALLY: &str = "Buff_Jake_WeaponMasterAlly_C";

const WEAPON_TAGS: [Name; 3] = [name!("JakeWeapon1"), name!("JakeWeapon2"), name!("JakeWeapon3")];

/// Buff_Jake_WeaponMaster_C's extension (`MasterState`).
#[derive(State, Debug)]
pub struct MasterExt {
    /// DeactivateBuff bound to HitBoxesDeactivated (never unbound once bound).
    pub bound: bool,
    /// ActivationTimer: the fighter tick it fires at, -1 none.
    pub at: Real,
    /// SetVFXVisibility's last value (the hands' VFX: the view's).
    pub active: bool,
}

impl Default for MasterExt {
    fn default() -> Self {
        MasterExt { bound: false, at: -1.0, active: false }
    }
}

fn sound(w: &mut World, f: FighterId, ev: Name) {
    let fighter = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: None });
}

/// @1087-1648 / @2820-3381: SetSubMeshVisibility(JakeWeaponN, CurrentWeapon == N - 1) for the three (with a valid
/// Comp_FighterVisibilityManager).
fn show_current(w: &mut World, f: FighterId) {
    let j = jake_w(w, f);
    j.weapons_hidden = false;
    let cur = j.weapon;
    for (i, tag) in WEAPON_TAGS.iter().enumerate() {
        set_sub_mesh(w, f, *tag, i32::try_from(i).expect("a weapon index") == cur);
    }
}

fn hide_all(w: &mut World, f: FighterId) {
    for tag in WEAPON_TAGS {
        set_sub_mesh(w, f, tag, false);
    }
}

/// SFX_Weapon_Swap @5048-5925: per CurrentWeapon 0 / 1 / 2, SetSwitch(character_jake_weapon_switch = axe / hammer /
/// mace) on the owner, then sfx_cha_jake_weapon_swap posted on it.
fn sfx_weapon_swap(w: &mut World, f: FighterId) {
    let value = match jake_w(w, f).weapon {
        0 => name!("character_jake_weapon_axe"),
        1 => name!("character_jake_weapon_hammer"),
        2 => name!("character_jake_weapon_mace"),
        _ => return,
    };
    let fighter = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Switch { fighter, group: name!("character_jake_weapon_switch"), value });
    sound(w, f, name!("sfx_cha_jake_weapon_swap"));
}

/// WeaponVisibilityMulticast(Equip) @4250: Equip -> WeaponEquiped = true, CurrentWeapon += 1 (past 2: 0 @2797), the
/// current one shown @2820, sfx_cha_jake_weapon_stretch_on @3469, SFX_Weapon_Swap @3542. Not Equip @4432 ->
/// WeaponEquiped = false, the three hidden @4632-4894, sfx_cha_jake_weapon_stretch_off @4974.
fn weapon_visibility(w: &mut World, f: FighterId, equip: bool) {
    let j = jake_w(w, f);
    j.weapon_equipped = equip;
    if equip {
        j.weapon = if j.weapon + 1 > 2 { 0 } else { j.weapon + 1 };
        show_current(w, f);
        sound(w, f, name!("sfx_cha_jake_weapon_stretch_on"));
        sfx_weapon_swap(w, f);
    } else {
        hide_all(w, f);
        sound(w, f, name!("sfx_cha_jake_weapon_stretch_off"));
    }
}

/// @1803: WeaponVisibilityMulticast(false), then the owner's WeaponMaster (CheckHasBuff) DeactivateBuff @2263.
fn unequip(w: &mut World, f: FighterId) {
    weapon_visibility(w, f, false);
    deactivate_weapon_master(w, f);
}

/// @2300: WeaponVisibilityMulticast(true), then its ActivateBuff @2760.
fn equip(w: &mut World, f: FighterId) {
    weapon_visibility(w, f, true);
    activate_weapon_master(w, f);
}

/// CycleWeapon @1717 (with authority): WeaponEquiped ? @1803 : @2300.
pub fn cycle_weapon(w: &mut World, f: FighterId) {
    if jake_w(w, f).weapon_equipped {
        unequip(w, f);
    } else {
        equip(w, f);
    }
}

/// EquipIfUnarmed @3557: not WeaponEquiped -> @2300.
pub fn equip_if_unarmed(w: &mut World, f: FighterId) {
    if !jake_w(w, f).weapon_equipped {
        equip(w, f);
    }
}

/// UnEquipIfArmed @3644: WeaponEquiped -> @1803.
pub fn un_equip_if_armed(w: &mut World, f: FighterId) {
    if jake_w(w, f).weapon_equipped {
        unequip(w, f);
    }
}

/// TempHidAllWeapons @3731: the three hidden (WeaponEquiped kept).
pub fn temp_hid_all_weapons(w: &mut World, f: FighterId) {
    jake_w(w, f).weapons_hidden = true;
    hide_all(w, f);
}

/// `unhidEquippedWeapon` (UnhidEquippedWeapon @4235): WeaponEquiped -> @1087 (the current one shown). Callers:
/// AnimNotify_WeaponHideCurrent's end, JakeHorse's EndPlay (horse.rs), JakeHouse_Actor's EndPlay @12995 (house.rs),
/// JakeStretch's BeginPlay @5862 (stretch.rs).
pub fn unhid_equipped_weapon(w: &mut World, f: FighterId) {
    if jake_w(w, f).weapon_equipped {
        show_current(w, f);
    }
}

/// AnimNotify_WeaponSwap Received_NotifyBegin: Cycle (CDO true) -> CycleWeapon @107, else Equip -> EquipIfUnarmed
/// @266, else UnEquip -> UnEquipIfArmed @414, on the owner's Comp_WeaponSwap (none on another pawn: nothing).
/// NotifyEnd: nothing. Jake_WeaponSwap(_Air) Cycle; Attack_Combo_Weapon_02 / 03 Equip; Fist_Combo_Attack_02 / 03
/// UnEquip.
fn swap_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if jake_of(w, f).is_none() {
        return;
    }
    let p = notify_props(w, f, n);
    let (cycle, eq, un) = (flag_or(p, "Cycle", true), flag(p, "Equip"), flag(p, "UnEquip"));
    if cycle {
        cycle_weapon(w, f);
    } else if eq {
        equip_if_unarmed(w, f);
    } else if un {
        un_equip_if_armed(w, f);
    }
}

/// AnimNotify_WeaponHideCurrent: Begin TempHidAllWeapons @229, End UnhidEquippedWeapon @229 (with a valid
/// Comp_WeaponSwap). The Ballet emote, Taunt_Dance, the three horse montages.
fn hide_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if jake_of(w, f).is_some() {
        temp_hid_all_weapons(w, f);
    }
}

fn hide_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if jake_of(w, f).is_some() {
        unhid_equipped_weapon(w, f);
    }
}

// ---- Buff_Jake_WeaponMaster (Perk_Jake_WeaponMaster, self only; BuffDuration 5, the perk's spawn has none) ----
// ActivationDuration 3, DamageModAddition 3, KnockBackModAddition 1.5, MaxDistanceToGiveToAlly 300; DealtDamageMods
// DamageMod / KnockBackMod 0 of type Add (NewEnumerator1): nothing until ActivateBuff writes them.
// BeginPlay @3560: PS_Jake_WeaponMaster at R_Hand and L_Hand (scale 2), hidden (VFX); an owner that is no
// BP_FighterCharacter destroys it @4170 (a fighter always is).
// ActivateBuff @2685: DealtDamageMods.DamageMod = DamageModAddition, KnockBackMod = KnockBackModAddition;
// ActivationTimer cleared and set again, K2_SetTimerDelegate(DeactivateBuff, ActivationDuration, no loop) @2800;
// DeactivateBuff bound to the owner's Comp_Pawn_HitBoxManager.HitBoxesDeactivated @2979-3002 (AddUnique); the VFX
// shown @3065; with GetAllPlayers > 2 @3230, each player whose pawn is not the owner, of the owner's TeamIndex and
// within MaxDistanceToGiveToAlly (Vector_Distance of the actor locations) @376-2504 gets SpawnBuffActor(
// Buff_Jake_WeaponMasterAlly_C, the pawn, the owner, 0, 3.0, true, false) on its own Comp_State_Manager.
// DeactivateBuff @3279: both mods 0, the timer cleared, the binding added again (it stays bound), the VFX hidden.
// Deviation: a player's pawn waiting for its respawn is left out of the share (GetAllPlayers lists it; as
// World.applyNearbyBuffs); a minion is no player.

/// `masterState`: the buff's extension, made at the first read.
fn master_mut(w: &mut World, b: BuffRef) -> Option<&mut MasterExt> {
    let x = buff_mut(w, b)?;
    if !matches!(x.ext, BuffExt::JakeWeaponMaster(_)) {
        x.ext = BuffExt::JakeWeaponMaster(MasterExt::default());
    }
    match &mut x.ext {
        BuffExt::JakeWeaponMaster(e) => Some(e),
        _ => None,
    }
}

fn master_of(w: &World, b: BuffRef) -> Option<&MasterExt> {
    match &buff(w, b)?.ext {
        BuffExt::JakeWeaponMaster(e) => Some(e),
        _ => None,
    }
}

fn set_mods(w: &mut World, b: BuffRef, dmg: Real, kb: Real) {
    let mut d = inst_def_of(w, b).and_then(|p| obj(&p, "DealtDamageMods").cloned()).unwrap_or_default();
    d.insert("DamageMod".to_owned(), Value::Num(f64::from(dmg)));
    d.insert("KnockBackMod".to_owned(), Value::Num(f64::from(kb)));
    set_buff_values(w, b, &Props::from_iter([("DealtDamageMods".to_owned(), Value::Obj(d))]));
}

/// ActivateBuff on the owner's WeaponMaster, when it has one (Comp_WeaponSwap's CheckHasBuff @2482).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated test, kept for its NaN result")]
fn activate_weapon_master(w: &mut World, f: FighterId) {
    let Some(b) = buff_of(w, f, name!("Buff_Jake_WeaponMaster_C")) else { return };
    let d = buff_def(w, ActorRef::Fighter(f), name!("Buff_Jake_WeaponMaster_C")).cloned().unwrap_or_default();
    set_mods(w, b, num_or(&d, "DamageModAddition", 0.0), num_or(&d, "KnockBackModAddition", 0.0));
    // set in the tick groups: from the next timer pass
    let at = crate::real(w.fighter(f).clock.tick) + timer_ticks(num(&d, "ActivationDuration"));
    if let Some(s) = master_mut(w, b) {
        s.at = at;
        s.bound = true;
        s.active = true;
    }
    let all = match_fighters(w);
    if all.len() <= 2 {
        return;
    }
    let max = num_or(&d, "MaxDistanceToGiveToAlly", 0.0);
    for o in all {
        let (of, ff) = (w.fighter(o), w.fighter(f));
        if o == f || of.life.dead > 0.0 || of.team.team != ff.team.team {
            continue;
        }
        let dx = of.body.x - ff.body.x;
        let dy = actor_y(w, o) - actor_y(w, f);
        if !(((dx * dx + dy * dy).sqrt()) <= max) {
            continue;
        }
        spawn_buff(w, o, name!("Buff_Jake_WeaponMasterAlly_C"), 3.0, Some(0), Some(f), true, Some(o));
    }
}

fn deactivate(w: &mut World, b: BuffRef) {
    set_mods(w, b, 0.0, 0.0);
    if let Some(s) = master_mut(w, b) {
        s.at = -1.0;
        s.bound = true;
        s.active = false;
    }
}

/// DeactivateBuff on the owner's WeaponMaster, when it has one (CheckHasBuff @1985).
fn deactivate_weapon_master(w: &mut World, f: FighterId) {
    if let Some(b) = buff_of(w, f, name!("Buff_Jake_WeaponMaster_C")) {
        deactivate(w, b);
    }
}

/// `weaponMasterActive`: whether the owner's WeaponMaster shows its hand VFX (SetVFXVisibility): for the view.
pub fn weapon_master_active(w: &World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_Jake_WeaponMaster_C")).is_some_and(|b| master_of(w, b).is_some_and(|e| e.active))
}

/// Buff_Jake_WeaponMaster BeginPlay @3560: its state.
fn master_begin(w: &mut World, _f: FighterId, b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    master_mut(w, b);
}

/// Buff_Jake_WeaponMaster: HitBoxesDeactivated, once bound -> DeactivateBuff @3279.
fn master_on(w: &mut World, _f: FighterId, b: BuffRef, s: BuffSignal, _arg: &BuffSignalArg) -> bool {
    if s == BuffSignal::HitBoxesDeactivated && master_mut(w, b).is_some_and(|e| e.bound) {
        deactivate(w, b);
    }
    false
}

/// Buff_Jake_WeaponMaster's ActivationTimer -> DeactivateBuff.
fn master_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let at = master_mut(w, b).map_or(-1.0, |s| s.at);
    if at < 0.0 || crate::real(w.fighter(f).clock.tick) < at {
        return false;
    }
    deactivate(w, b);
    false
}

// ---- Buff_Jake_WeaponMasterAlly (BuffDuration 3, CanRefresh; DealtDamageMods DamageMod 3 / KnockBackMod 1.5, Add) ----
// BeginPlay @15: with authority OnHitboxesDeactivated bound to its owner's (the ally's) Comp_Pawn_HitBoxHandler
// HitBoxesDeactivated @234-257 -> K2_DestroyActor @2368; PS_Jake_WeaponMaster at R_Hand / L_Hand (scale 2.5) through
// the owner's Comp_FighterVisibilityManager.AddVFX (the view's). EndPlay @1349: unbound, RemoveVFX and the emitters
// destroyed. Its mods are the class data's, taken by the dealt pass as they are (`dealt_mods_over`).

/// Buff_Jake_WeaponMasterAlly: HitBoxesDeactivated -> K2_DestroyActor @2368.
fn ally_on(_w: &mut World, _f: FighterId, _b: BuffRef, s: BuffSignal, _arg: &BuffSignalArg) -> bool {
    s == BuffSignal::HitBoxesDeactivated
}

/// This file's registry entries (the two notifies, the two buffs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies
        .enter("AnimNotify_WeaponSwap_C", NotifyHandler { begin: Some(swap_begin), ..NotifyHandler::default() })?;
    r.notifies.enter(
        "AnimNotify_WeaponHideCurrent_C",
        NotifyHandler { begin: Some(hide_begin), end: Some(hide_end), ..NotifyHandler::default() },
    )?;
    r.buffs.enter(
        WEAPON_MASTER,
        BuffEntry::new(
            BuffClass::BuffJakeWeaponMaster,
            BuffHooks {
                begin: Some(master_begin),
                on: Some(master_on),
                timer: Some(master_timer),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.buffs.enter(
        WEAPON_MASTER_ALLY,
        BuffEntry::new(BuffClass::BuffJakeWeaponMasterAlly, BuffHooks { on: Some(ally_on), ..BuffHooks::default() }),
    )
}
