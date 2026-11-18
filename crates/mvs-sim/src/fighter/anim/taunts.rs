//! Comp_Pawn_TauntManager: the four taunt slots of a fighter (TauntDataMap keys 0-3),
//! GetTauntBranchStruct, and the reads of other classes (Buff_Disguise GetTauntInfo, AnimNotify_PlayEmoteVFX). A slot
//! holds a TauntData or an EmoteData (a sticker) asset of the fighter's data (`FighterData::taunt_table`), by name.

use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::names::Name;
use crate::world::World;
use mvs_data::fighter::{Branch, FighterData, TauntEntry};
use mvs_data::value::{Props, Value};

/// The taunt manager's class defaults (the pawn's component, else the base's).
fn taunt_manager(d: &FighterData) -> Option<&Props> {
    let c = d
        .components
        .pawn
        .get("Comp_Pawn_TauntManager_GEN_VARIABLE")
        .or_else(|| d.components.base.get("Comp_Pawn_TauntManager_GEN_VARIABLE"))?;
    match &c.p {
        Value::Obj(p) => Some(p),
        _ => None,
    }
}

fn manager_str<'d>(d: &'d FighterData, k: &str) -> Option<&'d str> {
    taunt_manager(d).and_then(|p| p.get(k)).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// `tauntSlotNames`: the slot names (4, '' empty) of a fighter of this data whose player names
/// `taunts` (MatchPlayer.taunts).
/// - A player who names a taunt: the player's slots, by index; a name the data lacks is refused with a warning.
/// - Else OnSyncedPreferencesLoaded's forced-offline path (IsForcedOfflineMode, always true in this build): with a
///   valid TestTaunt (@622, Taz's Emote_Batman_Neutral) every slot 0-3 gets it (@665-911).
///   Deviation: the build's forced-offline path takes the TestTaunt over the player's slots too; the port keeps the
///   player's picks (the online game's path), so a TestTaunt only fills a loadout that names none.
/// - Nothing loaded (@4065): slot 0 = the table's first entry that is not a sticker (@4908-5089).
///
/// `w`: the World the warnings go to.
pub fn taunt_slot_names(w: &World, d: &FighterData, taunts: Option<&[&str]>) -> [String; 4] {
    let empty = Default::default();
    let table = d.taunt_table.as_ref().unwrap_or(&empty);
    let named = taunts.is_some_and(|t| t.iter().take(4).any(|n| !n.is_empty()));
    if !named && let Some(test) = manager_str(d, "TestTaunt") {
        if table.contains_key(test) {
            return std::array::from_fn(|_| test.to_string());
        }
        warn_text(w, &format!("Comp_Pawn_TauntManager: TestTaunt {test} is not in the taunt table"));
    }
    let mut out: [String; 4] = Default::default();
    for (i, n) in taunts.unwrap_or(&[]).iter().take(4).enumerate() {
        if n.is_empty() {
            continue;
        }
        if table.contains_key(*n) {
            out[i] = (*n).to_string();
        } else {
            warn_text(
                w,
                &format!("taunt slot {i}: {n} is not a taunt or sticker of this character, the slot is empty"),
            );
        }
    }
    if out.iter().all(String::is_empty) {
        out[0] = table.iter().find(|(_, t)| t.sticker.is_none()).map(|(k, _)| k.clone()).unwrap_or_default();
    }
    out
}

/// `setTauntSlots`.
pub fn set_taunt_slots(w: &mut World, f: FighterId, taunts: Option<&[Name]>) {
    let names = w.names.clone();
    let list: Option<Vec<&str>> = taunts.map(|t| t.iter().map(|&n| names.str(n)).collect());
    let slots = taunt_slot_names(w, super::montages::fdata(w, f), list.as_deref());
    let slots: Vec<Name> = slots.iter().map(|s| names.get(s).unwrap_or_default()).collect();
    w.fighter_mut(f).anim.taunt_slots = Some(slots);
}

/// `tauntSlotName`: TauntDataMap[slot]'s asset name, '' for an empty slot. A fighter whose
/// slots were not set reads the fresh-profile slots (they are set at this first read; they are a function of the
/// data alone).
pub fn taunt_slot_name(w: &World, f: FighterId, slot: i32) -> String {
    let Ok(i) = usize::try_from(slot) else { return String::new() };
    match &w.fighter(f).anim.taunt_slots {
        Some(s) => s.get(i).map(|&n| w.names.str(n).to_string()).unwrap_or_default(),
        None => taunt_slot_names(w, super::montages::fdata(w, f), None).get(i).cloned().unwrap_or_default(),
    }
}

/// `tauntSlotEntry`.
pub fn taunt_slot_entry(w: &World, f: FighterId, slot: i32) -> Option<&TauntEntry> {
    let n = taunt_slot_name(w, f, slot);
    if n.is_empty() {
        return None;
    }
    super::montages::fdata(w, f).taunt_table.as_ref()?.get(&n)
}

/// `tauntBranch`: GetTauntBranchStruct for a slot: an EmoteData plays its Montage when the soft
/// reference is valid, else the component's EmoteMontage (@2000); a TauntData its montage. A valid montage: Button 5,
/// Direction 9, the data's Conditions and Priority, AllowPreAttackRotation true; else the empty struct (None).
pub fn taunt_branch(w: &World, f: FighterId, slot: i32) -> Option<Branch> {
    let t = taunt_slot_entry(w, f, slot)?;
    let montage = if t.sticker.is_some() {
        t.montage.as_deref().or_else(|| manager_str(super::montages::fdata(w, f), "EmoteMontage"))
    } else {
        t.montage.as_deref()
    };
    let montage = montage.filter(|m| !m.is_empty())?;
    Some(Branch {
        attack: montage.to_string(),
        button: String::new(),
        direction: "AD_Default".to_string(),
        conditions: t.conds.iter().map(|c| Some(c.clone())).collect(),
        priority: crate::math::dbl::to_int32(t.priority),
        ability_slot: None,
        allow_pre_attack_rotation: true,
        display_name: None,
        unique_id: String::new(),
    })
}

/// `tauntInfoMontage`: Buff_Disguise GetTauntInfo (@0-1716): the first slot whose entry is a
/// cached TauntData. Unverified: map order (the order the slots' loads completed) is taken as slot order.
pub fn taunt_info_montage(w: &World, f: FighterId) -> Option<Name> {
    (0..4).find_map(|i| {
        let t = taunt_slot_entry(w, f, i)?;
        if t.sticker.is_some() {
            return None;
        }
        let m = t.montage.as_deref().filter(|m| !m.is_empty())?;
        Some(w.names.get(m).unwrap_or_default())
    })
}
