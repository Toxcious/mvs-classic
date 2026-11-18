//! Comp_AbilityOverrideManager: the pack list and its two readers, Comp_Pawn_BranchManager
//! BranchManagerGetAttack @1658-1881 (CheckForOverride) and @1905-2128 (CheckForBranchOverride).
//!
//! The two item packs (Comp_ItemManager PickUpOverride, Priority 1, and CurrentHoldOverridePack, Priority 0) are not
//! on this list: the item code derives them from ItemReachable and HeldItem, and the readers ask it first.
//! Deviation: that order is the build's outside a window and in a window whose bucket is empty (CheckForOverride: the
//! lowest Priority number wins, and no pack of the roster is under 0). In a window with rows for the press
//! CheckForBranchOverride appends the valid rows of every pack it reads, walking from the pack added last, and the
//! first row of the list wins: a pack added after an item pack and with a higher Priority number than it would win
//! there. The port has no add order between an item pack and a pack of this list.

// A written-out `!(a < b)` is true for a NaN where `a >= b` is not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::branches::{conds_ok, names_of};
use super::{OverridePack, OverrideRow};
use crate::fighter::FighterId;
use crate::fighter::control::trigger_ability_check;
use crate::input::{Btn, Dir, dir_name};
use crate::names::Name;
use crate::world::World;
use mvs_data::fighter::{AttackEntry, Branch};

/// Enum_InputButton.
const BTN_NORMAL: i32 = 0;
const BTN_SPECIAL: i32 = 1;
const BTN_ITEM: i32 = 5;
const BTN_NONE: i32 = 6;

/// What an override reader answers: `undefined` (no override: the caller reads its own rows), `null` (the override
/// held and no row passed: no attack) or the row.
#[derive(Clone, Debug)]
pub enum Override<T> {
    None,
    Refused,
    Row(T),
}

/// `pressButton`: a press's Enum_InputButton; `None` is the button None, what a buffered window holds until a press
/// is written into it.
pub fn press_button(b: Option<Btn>) -> i32 {
    match b {
        Some(Btn::Attack) => BTN_NORMAL,
        Some(Btn::Special) => BTN_SPECIAL,
        Some(Btn::Jump) => 2,
        Some(Btn::Dodge) => 3,
        Some(Btn::Item) => BTN_ITEM,
        None => BTN_NONE,
    }
}

/// `packAnswersNoPress`: a pack on the fighter has a row that IsOverrideValid can pass for
/// no button and direction Default (@1478-1648): a row of button None and direction Default with a montage.
pub fn pack_answers_no_press(w: &World, f: FighterId) -> bool {
    let Some(ps) = &w.fighter(f).branching.override_packs else { return false };
    ps.iter().any(|p| {
        p.overrides.iter().any(|r| {
            let a = &r.attack;
            a.required_button_input == BTN_NONE
                && a.required_direction_input == Dir::Default as i32
                && !a.attack_montage.is_empty()
        })
    })
}

fn same_attack(a: &AttackEntry, b: &AttackEntry) -> bool {
    a.required_button_input == b.required_button_input
        && a.required_direction_input == b.required_direction_input
        && a.attack_montage == b.attack_montage
        && a.ability_slot.unwrap_or(-1) == b.ability_slot.unwrap_or(-1)
        && a.allow_pre_attack_rotation == b.allow_pre_attack_rotation
        && a.conditions == b.conditions
}

/// `samePack`: two packs as Array_AddUnique / Array_RemoveItem compare them, the structs'
/// values (the row's DisplayName, an FText, is not kept in the data and is left out).
pub fn same_pack(a: &OverridePack, b: &OverridePack) -> bool {
    a.priority == b.priority
        && a.overrides.len() == b.overrides.len()
        && a.overrides
            .iter()
            .zip(&b.overrides)
            .all(|(x, y)| x.aerial_move == y.aerial_move && same_attack(&x.attack, &y.attack))
}

/// `addOverridePack`: AddOverridePack @0, Array_AddUnique.
pub fn add_override_pack(w: &mut World, f: FighterId, pack: OverridePack) {
    let ps = w.fighter_mut(f).branching.override_packs.get_or_insert_with(Vec::new);
    if ps.iter().any(|p| same_pack(p, &pack)) {
        return;
    }
    ps.push(pack);
}

/// `removeOverridePack`: RemoveOverridePack @0, Array_RemoveItem (every equal entry).
pub fn remove_override_pack(w: &mut World, f: FighterId, pack: &OverridePack) {
    let b = &mut w.fighter_mut(f).branching;
    let Some(ps) = &mut b.override_packs else { return };
    ps.retain(|p| !same_pack(p, pack));
    if ps.is_empty() {
        b.override_packs = None;
    }
}

/// `clearOverridePacks`: ClearOverridePacks @0, Array_Clear.
pub fn clear_override_packs(w: &mut World, f: FighterId) {
    w.fighter_mut(f).branching.override_packs = None;
}

fn excluded(exclude: Option<&[Name]>, w: &World, montage: &str) -> bool {
    exclude.is_some_and(|e| e.iter().any(|&n| w.names.str(n) == montage))
}

fn is_cur(w: &World, cur: Option<Name>, montage: &str) -> bool {
    cur.is_some_and(|c| w.names.str(c) == montage)
}

/// `isOverrideValid`: IsOverrideValid (@0-2375) for one row: 0 invalid, 1 valid, 2 valid
/// as an item-button override. `btn`: the press's Enum_InputButton; `dir`: its unmodified direction.
/// Unverified: GetByFighter's Success (@721, a pawn without player data) is taken as true, as the item code does.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn is_override_valid(
    w: &mut World,
    f: FighterId,
    a: &AttackEntry,
    btn: i32,
    dir: Dir,
    exclude: Option<&[Name]>,
    can_same: bool,
    cur: Option<Name>,
) -> u8 {
    if !can_same && is_cur(w, cur, &a.attack_montage) {
        return 0;
    }
    if excluded(exclude, w, &a.attack_montage) {
        return 0;
    }
    let mut item = false;
    if a.required_button_input == BTN_ITEM {
        let fi = w.fighter(f);
        let held = fi.items.held_item.is_some();
        if btn == BTN_NORMAL {
            item = if held {
                dir as i32 == a.required_direction_input
            } else {
                dir == Dir::Neutral && fi.items.item_reachable && fi.controls.prefs.attack_picks_up_items
            };
        } else if btn == BTN_ITEM {
            item = !held && fi.items.item_reachable;
        }
    }
    if !item && !(btn == a.required_button_input && dir as i32 == a.required_direction_input) {
        return 0;
    }
    let conds = names_of(w, a.conditions.iter().map(|c| c.as_deref()));
    if !conds_ok(w, f, Some(&conds), false) {
        return 0;
    }
    if let Some(s) = a.ability_slot
        && s >= 0
        && !trigger_ability_check(w, f, s)
    {
        return 0;
    }
    if item { 2 } else { 1 }
}

/// The packs' rows in the readers' walk: the packs from the last added to the first, a pack read only with
/// Priority < the best so far; each valid row of the fighter's ground / air state goes to `take`.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn walk(
    w: &mut World,
    f: FighterId,
    btn: i32,
    dir: Dir,
    air: bool,
    exclude: Option<&[Name]>,
    can_same: bool,
    cur: Option<Name>,
    take: &mut dyn FnMut(&AttackEntry, bool),
) {
    let packs: Vec<(crate::Real, Vec<OverrideRow>)> = match &w.fighter(f).branching.override_packs {
        Some(ps) => ps.iter().map(|p| (p.priority, p.overrides.clone())).collect(),
        None => return,
    };
    let mut best = 100.0;
    for (pr, rows) in packs.iter().rev() {
        if !(*pr < best) {
            continue;
        }
        for r in rows {
            // ahead of the AerialMove test (@1142, @1364)
            let v = is_override_valid(w, f, &r.attack, btn, dir, exclude, can_same, cur);
            if v == 0 || r.aerial_move != air {
                continue;
            }
            take(&r.attack, v == 2);
            best = *pr;
        }
    }
}

/// `checkForOverride`: CheckForOverride (@5-1911): the lowest Priority number, of equal
/// ones the pack added last, and in a pack its LAST valid row (no break, @1142-1643); with its IsItemButtonOverride.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn check_for_override(
    w: &mut World,
    f: FighterId,
    btn: i32,
    dir: Dir,
    air: bool,
    exclude: Option<&[Name]>,
    can_same: bool,
    cur: Option<Name>,
) -> Option<(AttackEntry, bool)> {
    let mut out: Option<(AttackEntry, bool)> = None;
    walk(w, f, btn, dir, air, exclude, can_same, cur, &mut |a, item| out = Some((a.clone(), item)));
    out
}

/// `checkForBranchOverride`: CheckForBranchOverride (@5-2367): the same walk, every valid
/// row appended (@2135); the IsItemButtonOverride of the last valid row met (@1432).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn check_for_branch_override(
    w: &mut World,
    f: FighterId,
    btn: i32,
    dir: Dir,
    air: bool,
    exclude: Option<&[Name]>,
    can_same: bool,
    cur: Option<Name>,
) -> Option<(Vec<AttackEntry>, bool)> {
    let mut rows: Option<Vec<AttackEntry>> = None;
    let mut item = false;
    walk(w, f, btn, dir, air, exclude, can_same, cur, &mut |a, it| {
        rows.get_or_insert_with(Vec::new).push(a.clone());
        item = it;
    });
    rows.map(|r| (r, item))
}

fn row_conds_ok(w: &mut World, f: FighterId, a: &AttackEntry) -> bool {
    let conds = names_of(w, a.conditions.iter().map(|c| c.as_deref()));
    conds_ok(w, f, Some(&conds), false) && a.ability_slot.is_none_or(|s| s < 0 || trigger_ability_check(w, f, s))
}

/// `overrideAttack`: the rest of BranchManagerGetAttack behind a CheckForOverride that
/// found a row (@1835-1900): an item-button override as it stands (@1487-1598); any other row matched again,
/// direction Default or the press's and button None or the press's (@2732-3079), then its conditions (@3241) and its
/// ability check (@3541) a second time.
pub fn override_attack(
    w: &mut World,
    f: FighterId,
    b: Option<Btn>,
    dir: Dir,
    exclude: Option<&[Name]>,
    can_same: bool,
    cur: Option<Name>,
) -> Override<AttackEntry> {
    if w.fighter(f).branching.override_packs.is_none() {
        return Override::None;
    }
    let btn = press_button(b);
    let air = crate::fighter::in_air(w, f);
    let Some((a, item)) = check_for_override(w, f, btn, dir, air, exclude, can_same, cur) else {
        return Override::None;
    };
    if item {
        return Override::Row(a);
    }
    if a.required_direction_input != Dir::Default as i32 && a.required_direction_input != dir as i32 {
        return Override::Refused;
    }
    if a.required_button_input != BTN_NONE && a.required_button_input != btn {
        return Override::Refused;
    }
    if !row_conds_ok(w, f, &a) {
        return Override::Refused;
    }
    Override::Row(a)
}

/// `overrideWindowBranch`: the rest of BranchManagerGetAttack behind a
/// CheckForBranchOverride that replaced the window's branches (@2082-2147): of the list the highest GetBranchPriority
/// above -2 (a Default row -1, any other 0), the first of equals; an item-button override takes a row as it stands,
/// else IsBranchValid (@2525) again.
pub fn override_window_branch(
    w: &mut World,
    f: FighterId,
    b: Option<Btn>,
    dir: Dir,
    exclude: Option<&[Name]>,
    can_same: bool,
    cur: Option<Name>,
) -> Override<Branch> {
    if w.fighter(f).branching.override_packs.is_none() {
        return Override::None;
    }
    let btn = press_button(b);
    let air = crate::fighter::in_air(w, f);
    let Some((rows, item)) = check_for_branch_override(w, f, btn, dir, air, exclude, can_same, cur) else {
        return Override::None;
    };
    let mut pick: Option<&AttackEntry> = None;
    let mut pr = -2;
    for a in &rows {
        let p = if a.required_direction_input == Dir::Default as i32 { -1 } else { 0 };
        if p <= pr {
            continue;
        }
        if !item {
            if !can_same && is_cur(w, cur, &a.attack_montage) {
                continue;
            }
            if excluded(exclude, w, &a.attack_montage) {
                continue;
            }
            if a.required_button_input != BTN_NONE && a.required_button_input != btn {
                continue;
            }
            if a.required_direction_input != Dir::Default as i32 && a.required_direction_input != dir as i32 {
                continue;
            }
            if !row_conds_ok(w, f, a) {
                continue;
            }
        }
        pick = Some(a);
        pr = p;
    }
    match pick {
        Some(a) => Override::Row(row_branch(w, a)),
        None => Override::Refused,
    }
}

/// `rowBranch`: a pack row as the branch CheckForBranchOverride makes of it (@1616-2074):
/// the row's own button and direction again; the port's Branch names the two attack buttons only.
pub fn row_branch(w: &World, a: &AttackEntry) -> Branch {
    let button = match a.required_button_input {
        BTN_NORMAL => "EActionButton::AB_HeavyAttack",
        BTN_SPECIAL => "EActionButton::AB_SpecialAttack",
        _ => "",
    };
    let d = if a.required_direction_input == Dir::Default as i32 {
        "AD_Default".to_string()
    } else {
        w.names.str(dir_name(dir_of(a.required_direction_input))).to_string()
    };
    Branch {
        attack: a.attack_montage.clone(),
        button: button.to_string(),
        direction: format!("EActionDirection::{d}"),
        conditions: a.conditions.clone(),
        priority: 0,
        ability_slot: a.ability_slot,
        allow_pre_attack_rotation: a.allow_pre_attack_rotation,
        display_name: None,
        unique_id: String::new(),
    }
}

/// A direction byte as a [`Dir`] (`dirName`'s table: an unknown byte names Neutral).
pub fn dir_of(b: i32) -> Dir {
    match b {
        0 => Dir::Up,
        1 => Dir::Down,
        2 => Dir::Fwd,
        3 => Dir::Back,
        5 => Dir::Default,
        6 => Dir::FwdUp,
        7 => Dir::FwdDown,
        8 => Dir::BackUp,
        9 => Dir::BackDown,
        _ => Dir::Neutral,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(montage: &str) -> OverrideRow {
        OverrideRow {
            aerial_move: false,
            attack: AttackEntry {
                required_button_input: 0,
                required_direction_input: 5,
                attack_montage: montage.to_string(),
                ability_slot: None,
                display_name: Some("a name the comparison leaves out".to_string()),
                conditions: Vec::new(),
                allow_pre_attack_rotation: true,
            },
        }
    }

    #[test]
    fn packs_compare_by_value_without_the_display_name() {
        let a = OverridePack { priority: 2.0, overrides: vec![row("M_A")], from_data: false };
        let mut b = OverridePack { priority: 2.0, overrides: vec![row("M_A")], from_data: false };
        b.overrides[0].attack.display_name = None;
        assert!(same_pack(&a, &b));
        b.overrides[0].attack.attack_montage = "M_B".to_string();
        assert!(!same_pack(&a, &b));
    }

    #[test]
    fn button_and_direction_bytes() {
        assert_eq!(press_button(None), BTN_NONE);
        assert_eq!(press_button(Some(Btn::Item)), BTN_ITEM);
        assert_eq!(dir_of(5), Dir::Default);
        assert_eq!(dir_of(4), Dir::Neutral);
        assert_eq!(dir_of(9), Dir::BackDown);
    }
}
