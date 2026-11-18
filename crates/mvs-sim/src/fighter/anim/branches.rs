//! The branch manager: Comp_Pawn_BranchManager's branch windows, the buffered input and the
//! branch conditions. The build's offsets for each rule are in the comment of the function of the same name.

// A written-out `!(a > b)` is true for a NaN where `a <= b` is not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::montages::{KEY_STRIDE, active_keys, active_of, any_notify, ix, montage_data, start_montage, truthy};
use super::overrides::{Override, override_attack, override_window_branch, pack_answers_no_press, row_branch};
use super::*;
use crate::Real;
use crate::combat::BuffRef;
use crate::combat::conditions;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::fighter::control::{
    ExecuteKind, add_recent_attack, attack_begin, evade_force, execute, forced_press, pre_attack_rotation, press_dir,
    swapped_dir, trigger_ability_check, trigger_air_special_cost,
};
use crate::fighter::match_side::{branch_row_tutorial, item_override};
use crate::fighter::{has_attack_inst, in_air, in_hitstun};
use crate::input::{Btn, Dir, dir_name, get8_way};
use crate::names::Name;
use crate::registry::entries::{Condition, NotifyHandler};
use crate::registry::signals::CondSignal;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::fighter::Branch;
use mvs_data::value::{Props, Value};

/// A window: its notify and its key (`{ n, i }`).
pub type Window = OpenWindowsOpenWindows;

const BRANCH_WINDOW: Name = crate::name!("AnimNotify_BranchWindow_C");
const BRANCH_WINDOW_CLS: &str = "AnimNotify_BranchWindow_C";

// ---- the window's data

fn props(w: &World, f: FighterId, n: NotifyRef) -> Option<&Props> {
    any_notify(w, f, n).map(|x| &x.p)
}

fn flag(w: &World, f: FighterId, n: NotifyRef, k: &str) -> bool {
    props(w, f, n).is_some_and(|p| truthy(p.get(k)))
}

/// A notify's list of strings (`BranchesToExclude`, `BranchGroups`); a null entry is `None`.
fn strs(p: &Props, k: &str) -> Vec<Option<String>> {
    p.get(k)
        .and_then(Value::as_arr)
        .map(|a| a.iter().map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn exclude_of(w: &World, f: FighterId, n: NotifyRef) -> Vec<String> {
    props(w, f, n).map(|p| strs(p, "BranchesToExclude").into_iter().flatten().collect()).unwrap_or_default()
}

fn exclude_names(w: &World, ex: &[String]) -> Vec<Name> {
    ex.iter().map(|s| w.names.get(s).unwrap_or_default()).collect()
}

/// Condition class names as [`Name`]s, a null entry `None` (a string the match did not intern names no condition).
pub fn names_of<'a>(w: &World, it: impl Iterator<Item = Option<&'a str>>) -> Vec<Option<Name>> {
    it.map(|c| c.map(|s| w.names.get(s).unwrap_or_default())).collect()
}

/// A notify's condition list (`n.p.Conditions`: class names, a null entry `None`).
pub fn conds_of(w: &World, p: &Props, k: &str) -> Option<Vec<Option<Name>>> {
    let a = p.get(k)?.as_arr()?;
    Some(names_of(w, a.iter().map(Value::as_str)))
}

fn win_conds_ok(w: &mut World, f: FighterId, n: NotifyRef) -> bool {
    let c = props(w, f, n).and_then(|p| conds_of(w, p, "BranchWindowConditions"));
    conds_ok(w, f, c.as_deref(), false)
}

/// A branch row of a notify's `Branches`.
fn branch_of(v: &Value) -> Option<Branch> {
    let p = v.as_obj()?;
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some(Branch {
        attack: s("Attack"),
        button: s("Button"),
        direction: s("Direction"),
        conditions: strs(p, "Conditions"),
        priority: p.get("Priority").and_then(Value::as_f64).map_or(0, crate::math::dbl::to_int32),
        ability_slot: p.get("AbilitySlot").and_then(Value::as_f64).map(crate::math::dbl::to_int32),
        allow_pre_attack_rotation: truthy(p.get("AllowPreAttackRotation")),
        display_name: p.get("DisplayName").and_then(Value::as_str).map(str::to_string),
        unique_id: s("UniqueID"),
    })
}

fn strip_enum(s: &str) -> &str {
    s.find("::").map_or(s, |i| &s[i + 2..])
}

fn dead(w: &World, f: FighterId) -> bool {
    w.fighter(f).life.dead > 0.0
}

fn wall_sliding(w: &World, f: FighterId) -> bool {
    w.fighter(f).wall.wall_sliding
}

fn cleared(w: &World, f: FighterId, k: i32) -> bool {
    w.fighter(f).branching.win_cleared.as_ref().is_some_and(|c| c.contains(&k))
}

/// The BranchWindows of the attack instances other than 0 (`fade` on or off), by key.
fn grouped_windows(w: &World, f: FighterId, faded: bool) -> Vec<(i32, NotifyRef)> {
    let mut out = Vec::new();
    let Some((l, _)) = &w.fighter(f).montages.grouped else { return out };
    for e in l {
        let inst = e.inst.unwrap_or(0);
        if inst == 0 || e.fade.is_some() != faded {
            continue;
        }
        let Some(md) = montage_data(w, f, e.m.montage.name) else { continue };
        for &i in &e.m.active {
            if md.notifies[i].cls == BRANCH_WINDOW_CLS {
                out.push((
                    inst * KEY_STRIDE + ix(i),
                    NotifyRef { montage: e.m.montage.name, index: super::montages::u32x(i) },
                ));
            }
        }
    }
    out
}

/// The main montage's BranchWindows that are on, by notify index.
fn main_windows(w: &World, f: FighterId) -> Vec<(i32, NotifyRef)> {
    let Some(m) = &w.fighter(f).montages.montage else { return Vec::new() };
    let Some(md) = montage_data(w, f, m.montage.name) else { return Vec::new() };
    m.active
        .iter()
        .filter(|&&i| md.notifies[i].cls == BRANCH_WINDOW_CLS)
        .map(|&i| (ix(i), NotifyRef { montage: m.montage.name, index: super::montages::u32x(i) }))
        .collect()
}

fn push_cleared(w: &mut World, f: FighterId, k: i32) {
    w.fighter_mut(f).branching.win_cleared.get_or_insert_with(Vec::new).push(k);
}

// ---- presses

/// `windowPress`: a press while branch windows are active; true if it acted at once.
pub fn window_press(w: &mut World, f: FighterId, b: Btn, _in_hitpause: bool) -> bool {
    if !has_attack_inst(w, f) {
        return false;
    }
    let d = press_dir(w, f);
    let force = match b {
        Btn::Dodge => evade_force(w, f),
        Btn::Jump => None,
        _ => forced_press(w, f),
    };
    let raw = press_dir(w, f);
    let ov = item_override(w, f, b, raw);
    buffer_press(w, f, QueuedBtn::Press(b), d, force, None);
    if wall_sliding(w, f) {
        return false;
    }
    let mut all = Vec::new();
    for x in open_windows(w, f, None) {
        if flag(w, f, x.n, "Immediate") && win_conds_ok(w, f, x.n) {
            all.push(x);
        }
    }
    if b == Btn::Jump || b == Btn::Dodge {
        let k = if b == Btn::Jump { "CanBranchToJump" } else { "CanBranchToEvade" };
        if !all.iter().any(|x| flag(w, f, x.n, k)) {
            return false;
        }
        let kind = if b == Btn::Jump { ExecuteKind::Jump } else { ExecuteKind::Dodge };
        execute(w, f, kind, None, force, Dir::Default);
        return true;
    }
    let wins: Vec<Window> = all.into_iter().filter(|x| !flag(w, f, x.n, "MakeBranchesAutomatic")).collect();
    if wins.is_empty() {
        return false;
    }
    // (the same press and stick for every window: one test passes or fails them all)
    if !hold_ok(w, f, Some(QueuedBtn::Press(b)), force.map_or(d as i32, |x| x.dir)) {
        return false;
    }
    if let Some(ov) = ov {
        let ovs = w.names.str(ov).to_string();
        let cur = w.fighter(f).branching.branch_montage;
        if wins.iter().any(|v| !exclude_of(w, f, v.n).contains(&ovs) && (can_same_attack(w, f, v.n) || cur != Some(ov)))
        {
            let br = override_branch(w, ov);
            execute(w, f, ExecuteKind::Branch, Some(&br), force, d);
            return true;
        }
    }
    if b == Btn::Item && w.fighter(f).branching.override_packs.is_none() {
        return false; // no row of a window answers the Item button
    }
    let sd = swapped_dir(w, f, Some(b), d, in_air(w, f));
    let Some((br, _)) = pick_window_branch(w, f, &wins, Some(b), sd, d) else { return false };
    execute(w, f, ExecuteKind::Branch, Some(&br), force, d);
    true
}

/// `openWindows`: the BranchWindows that are on, less the ones a ClearQueuedAttack took
/// out (`win_cleared`); behind them the windows of another anim instance the pawn filed (`file_window`).
pub fn open_windows(w: &mut World, f: FighterId, only: Option<Real>) -> Vec<OpenWindowsOpenWindows> {
    let mut ws: Vec<Window> =
        active_of(w, f, BRANCH_WINDOW, only).into_iter().map(|a| Window { n: a.n, i: a.i }).collect();
    let b = &w.fighter(f).branching;
    if let Some(c) = &b.win_cleared {
        ws.retain(|x| !c.contains(&x.i));
    }
    if only.is_none()
        && let Some(fw) = &b.foreign_wins
    {
        ws.extend(fw.iter().map(|e| Window { n: e.n, i: e.key }));
    }
    ws
}

/// `canSameAttack`: the window's CanBranchToSameAttack, false once SameAttackBlocked.
pub fn can_same_attack(w: &World, f: FighterId, n: NotifyRef) -> bool {
    !w.fighter(f).branching.same_attack_blocked && flag(w, f, n, "CanBranchToSameAttack")
}

/// `setBlockSameAttackBranch`: SetBlockSameAttackBranch @0, the flag's one writer.
/// Unverified: a respawn leaves it set (whether the component is the same object after one is not read).
pub fn set_block_same_attack_branch(w: &mut World, f: FighterId, blocked: bool) {
    w.fighter_mut(f).branching.same_attack_blocked = blocked;
}

// ---- windows of another anim instance

/// `foreignWindowBegin`: BranchGroupBlocked to the pawn's graph, then its buffs'.
pub fn foreign_window_begin(w: &mut World, f: FighterId, n: NotifyRef, montage: Name, duration: Real) {
    if let Some(h) = w.fighter(f).hooks.char.branch_group_blocked {
        h(w, f, n, montage, duration);
    }
    let mut i = 0;
    while let Some(b) = w.fighter(f).buffs.buffs.get(i) {
        let (serial, cls, on) = (b.serial, b.cls, b.frames > 0.0);
        if on && let Some(h) = w.registry.buffs.get(w.names.str(cls)).and_then(|e| e.hooks.branch_group_blocked) {
            h(w, f, BuffRef { fighter: f, serial }, n, montage, duration);
        }
        i += 1;
    }
}

/// `foreignWindowEnd`: BranchGroupEndBlocked to the pawn's graph, then its buffs'.
pub fn foreign_window_end(w: &mut World, f: FighterId, n: NotifyRef) {
    if let Some(h) = w.fighter(f).hooks.char.branch_group_end_blocked {
        h(w, f, n);
    }
    let mut i = 0;
    while let Some(b) = w.fighter(f).buffs.buffs.get(i) {
        let (serial, cls, on) = (b.serial, b.cls, b.frames > 0.0);
        if on && let Some(h) = w.registry.buffs.get(w.names.str(cls)).and_then(|e| e.hooks.branch_group_end_blocked) {
            h(w, f, BuffRef { fighter: f, serial }, n);
        }
        i += 1;
    }
}

/// `branchNotifyStart`: BranchNotifyStart @116-361 for the pawn's own anim instance;
/// `keep` the key of the window being filed, which the caller files again.
pub fn branch_notify_start(w: &mut World, f: FighterId, montage: Option<Name>, keep: Option<i32>) {
    let b = &mut w.fighter_mut(f).branching;
    if montage == b.branch_montage {
        return;
    }
    b.branch_montage = montage;
    clear_filed(w, f, keep);
}

/// `clearFiled`: Map_Clear of the three maps, the stored input with them, the windows of another anim instance, and
/// every BranchWindow of the pawn's own that is on (`win_cleared`).
fn clear_filed(w: &mut World, f: FighterId, keep: Option<i32>) {
    let b = &mut w.fighter_mut(f).branching;
    b.queued = None;
    b.late_branch = None;
    b.late_auto = None;
    drop_foreign_windows(w, f);
    let mut keys: Vec<i32> = open_windows(w, f, None).into_iter().filter(|x| x.i >= 0).map(|x| x.i).collect();
    keys.extend(grouped_windows(w, f, true).into_iter().map(|(k, _)| k));
    for k in keys {
        if Some(k) != keep && !cleared(w, f, k) {
            push_cleared(w, f, k);
        }
    }
}

/// `branchBlendingOut`: OnMontageBlendingOut_Event @1702-2111 for CurrentMontage with
/// the maps empty: CurrentMontage = null.
/// Deviation: the other way out (@2111, CurrentMontageIsBlendingOut) and the CompleteBranchNotify tail it gates
/// (@877-957) are not modelled; the field then keeps the montage until the next BranchNotifyStart.
pub fn branch_blending_out(w: &mut World, f: FighterId, montage: Name) {
    let b = &w.fighter(f).branching;
    if Some(montage) != b.branch_montage || b.late_branch.is_some() || !open_windows(w, f, None).is_empty() {
        return;
    }
    if grouped_windows(w, f, true).into_iter().any(|(k, _)| !cleared(w, f, k)) {
        return;
    }
    w.fighter_mut(f).branching.branch_montage = None;
}

/// `dropForeignWindows`.
pub fn drop_foreign_windows(w: &mut World, f: FighterId) {
    w.fighter_mut(f).branching.foreign_wins = None;
}

/// `clearForeignWindows`: Trigger Current Attack Client's exit (ubergraph @5768-6036)
/// for the windows of another anim instance, with the input stored in them.
/// Deviation: with `keep_immediate` an Immediate window that is automatic too stays whole (the build takes it out of
/// CurrentAutoBranchData and leaves it in CurrentImmediateBranchData).
pub fn clear_foreign_windows(w: &mut World, f: FighterId, keep_immediate: bool) {
    let Some(fw) = w.fighter_mut(f).branching.foreign_wins.take() else { return };
    let q = w.fighter(f).branching.queued.is_some();
    let mut kept = Vec::new();
    let mut gone = Vec::new();
    for e in fw {
        if keep_immediate && flag(w, f, e.n, "Immediate") {
            kept.push(e);
        } else if q {
            gone.push(e.key);
        }
    }
    let b = &mut w.fighter_mut(f).branching;
    b.foreign_wins = if kept.is_empty() { None } else { Some(kept) };
    if q && !gone.is_empty() {
        drop_queued_keys(b, &gone);
    }
}

/// `{ ...q, windows: rest }` or null: the stored press leaves the windows `gone`.
fn drop_queued_keys(b: &mut Branching, gone: &[i32]) {
    let Some(q) = &mut b.queued else { return };
    let n = q.windows.len();
    q.windows.retain(|i| !gone.contains(i));
    if q.windows.len() != n && q.windows.is_empty() {
        b.queued = None;
    }
}

/// `fileWindow`: BranchNotifyStart for a window of another anim instance, from the pawn's
/// handler of BranchGroupBlocked; returns the window's key (below 0).
/// Unverified: among several open windows these are read behind the pawn's own, in the order they were filed.
pub fn file_window(w: &mut World, f: FighterId, n: NotifyRef, montage: Option<Name>, duration: Real) -> i32 {
    branch_notify_start(w, f, montage, None);
    let cur = montage.unwrap_or_default();
    let b = &mut w.fighter_mut(f).branching;
    let mut key = -1;
    let mut found = None;
    for e in b.foreign_wins.get_or_insert_with(Vec::new).iter_mut() {
        if e.n == n {
            e.cur = cur;
            found = Some(e.key);
            break;
        }
        if e.key <= key {
            key = e.key - 1;
        }
    }
    if let Some(k) = found {
        if b.queued.as_ref().is_some_and(|q| q.windows.contains(&k)) {
            drop_queued_keys(b, &[k]);
        }
        set_window_end(w, f, k, duration);
        return k;
    }
    b.foreign_wins.get_or_insert_with(Vec::new).push(ForeignWindow { n, key, cur });
    set_window_end(w, f, key, duration);
    key
}

/// `setWindowEnd`: BranchEndTime = NotifyTotalDuration + GetGameTimeInSeconds (float32),
/// kept only under an InputBufferCap of 30 or less.
pub fn set_window_end(w: &mut World, f: FighterId, key: i32, duration: Real) {
    let fi = w.fighter_mut(f);
    if fi.controls.prefs.input_buffer_cap > 30 {
        return;
    }
    let t = duration + fi.clock.time;
    fi.branching.win_ends.get_or_insert_with(Default::default).insert(key, t);
}

/// `completeWindow`: CompleteBranchNotify for a window `file_window` filed.
pub fn complete_window(w: &mut World, f: FighterId, n: NotifyRef) {
    let b = &mut w.fighter_mut(f).branching;
    let Some(fw) = &mut b.foreign_wins else { return };
    let Some(k) = fw.iter().position(|e| e.n == n) else { return };
    let e = fw.remove(k);
    if fw.is_empty() {
        b.foreign_wins = None;
    }
    if let Some(ends) = &mut b.win_ends {
        ends.shift_remove(&e.key);
    }
    let win = Window { n, i: e.key };
    let held = b.queued.as_ref().filter(|q| q.windows.contains(&e.key)).map(|q| queued_for(q, e.key));
    let Some(p) = held else {
        // nothing stored: button None, direction Default (`resolveUnpressed`)
        if !answers_no_press(w, f, n) {
            return;
        }
        let Some(br) = resolve_unpressed(w, f, &win) else { return };
        w.fighter_mut(f).branching.queued = None;
        if in_hitstun(w, f) || dead(w, f) {
            clear_late(w, f);
            clear_foreign_windows(w, f, false);
            return;
        }
        execute(w, f, ExecuteKind::Branch, Some(&br), None, Dir::Default);
        return;
    };
    drop_queued_keys(&mut w.fighter_mut(f).branching, &[e.key]);
    let Some(r) = resolve_buffered(w, f, &win, &p) else { return };
    if in_hitstun(w, f) || dead(w, f) {
        w.fighter_mut(f).branching.queued = None;
        clear_late(w, f);
        clear_foreign_windows(w, f, false);
        return;
    }
    if r.kind == ResolveBufferedKind::Branch {
        w.fighter_mut(f).branching.queued = None;
    }
    execute(w, f, exec_kind(r.kind), r.br.as_ref(), p.force, p.dir);
}

fn exec_kind(k: ResolveBufferedKind) -> ExecuteKind {
    match k {
        ResolveBufferedKind::Branch => ExecuteKind::Branch,
        ResolveBufferedKind::Jump => ExecuteKind::Jump,
        ResolveBufferedKind::Dodge => ExecuteKind::Dodge,
    }
}

fn clear_late(w: &mut World, f: FighterId) {
    let b = &mut w.fighter_mut(f).branching;
    b.late_branch = None;
    b.late_auto = None;
}

/// `clearQueuedAttack`: ClearQueuedAttack (ubergraph 14913-15054): ActiveAttack reset and
/// Map_Clear of the three maps; the keys go, not only the stored input.
pub fn clear_queued_attack(w: &mut World, f: FighterId) {
    w.fighter_mut(f).branching.queued = None;
    clear_late(w, f);
    // a window of another anim instance has no later BranchNotifyStart
    w.fighter_mut(f).branching.foreign_wins = None;
    let mut keys: Vec<i32> = main_windows(w, f).into_iter().map(|(k, _)| k).collect();
    // the maps are one record: the windows of the other attack instances go with them
    keys.extend(grouped_windows(w, f, false).into_iter().map(|(k, _)| k));
    for k in keys {
        if !cleared(w, f, k) {
            push_cleared(w, f, k);
        }
    }
}

/// `overrideBranch`.
pub fn override_branch(w: &World, montage: Name) -> Branch {
    Branch {
        attack: w.names.str(montage).to_string(),
        button: String::new(),
        direction: "AD_Default".to_string(),
        conditions: Vec::new(),
        priority: 0,
        ability_slot: None,
        allow_pre_attack_rotation: true,
        display_name: None,
        unique_id: String::new(),
    }
}

/// `bufferPress`: InputTriggered_Event @3372-4923: every press is written into each
/// buffered window that is on, valid or not, the last press winning.
pub fn buffer_press(
    w: &mut World,
    f: FighterId,
    press: QueuedBtn,
    dir: Dir,
    force: Option<ForcedPress>,
    taunt: Option<&Branch>,
) -> bool {
    let windows: Vec<i32> =
        open_windows(w, f, None).into_iter().filter(|x| !flag(w, f, x.n, "Immediate")).map(|x| x.i).collect();
    if windows.is_empty() {
        return false;
    }
    let raw = press_dir(w, f);
    let p = QueuedPress { press, dir, raw, force, taunt: taunt.map(|t| Box::new(t.clone())) };
    let old = w.fighter_mut(f).branching.queued.take();
    let q = write_press(w, f, old, p, &windows);
    w.fighter_mut(f).branching.queued = q;
    true
}

/// `writePress`: InputTriggered_Event's write of one press into the windows `keys` over
/// what `old` held; under an InputBufferCap of 30 or less a window takes it only when `float(Cap) / 60 >=
/// BranchEndTime - GetGameTimeInSeconds()` (float32, @4164-4536), and one that fails keeps its press.
pub fn write_press(
    w: &mut World,
    f: FighterId,
    old: Option<QueuedAction>,
    p: QueuedPress,
    keys: &[i32],
) -> Option<QueuedAction> {
    let fi = w.fighter(f);
    let cap = fi.controls.prefs.input_buffer_cap;
    if cap > 30 {
        return Some(QueuedAction::of(p, keys.to_vec()));
    }
    let lim = crate::real(cap) / 60.0;
    let ends = &fi.branching.win_ends;
    let mut take = Vec::new();
    let mut keep = Vec::new();
    let mut older: Option<IndexMap<i32, QueuedPress>> = None;
    for &k in keys {
        let end = ends.as_ref().and_then(|e| e.get(&k)).copied().unwrap_or(0.0);
        if lim >= (end - fi.clock.time) {
            take.push(k);
        } else if let Some(o) = &old
            && o.windows.contains(&k)
        {
            older.get_or_insert_with(IndexMap::new).insert(k, queued_for(o, k));
            keep.push(k);
        }
    }
    if take.is_empty() {
        return old;
    }
    take.extend(keep);
    let mut q = QueuedAction::of(p, take);
    q.older = older;
    Some(q)
}

/// `queuedFor`: the press a window holds: the newest, or the older one the InputBufferCap
/// left it.
pub fn queued_for(q: &QueuedAction, key: i32) -> QueuedPress {
    q.older.as_ref().and_then(|o| o.get(&key)).cloned().unwrap_or_else(|| q.newest())
}

/// `holdOk`: the InputBufferHold preference: the stored button still held and the stick
/// of this moment, a diagonal folded to Up / Down, on the stored byte. `None` is the button None.
pub fn hold_ok(w: &World, f: FighterId, b: Option<QueuedBtn>, dir: i32) -> bool {
    let fi = w.fighter(f);
    if !fi.controls.prefs.input_buffer_hold {
        return true;
    }
    if let Some(b) = b {
        let btn = match b {
            QueuedBtn::Press(Btn::Attack) => 0,
            QueuedBtn::Press(Btn::Special) => 1,
            QueuedBtn::Press(Btn::Jump) => 2,
            QueuedBtn::Press(Btn::Dodge) => 3,
            QueuedBtn::Taunt => 4,
            QueuedBtn::Press(Btn::Item) => 5,
        };
        if fi.branching.button_states.unwrap_or(0) & (1 << btn) == 0 {
            return false;
        }
    }
    if dir == Dir::Default as i32 {
        return true;
    }
    let d8 = get8_way(fi.controls.inp.x, fi.controls.inp.y, fi.body.facing_dir);
    let d = match d8 {
        Dir::FwdUp | Dir::BackUp => Dir::Up,
        Dir::FwdDown | Dir::BackDown => Dir::Down,
        d => d,
    };
    d as i32 == dir
}

/// `resolveBuffered`: SetActiveAttackFromBranch for one buffered window at its end
/// (CompleteBranchNotify, ubergraph 1060-1324).
pub fn resolve_buffered(
    w: &mut World,
    f: FighterId,
    w2: &ResolveBufferedW,
    q: &QueuedPress,
) -> Option<ResolveBufferedOut> {
    let n = w2.n;
    if wall_sliding(w, f) || !win_conds_ok(w, f, n) {
        return None;
    }
    let out = |kind, br| Some(ResolveBufferedOut { kind, br });
    match q.press {
        QueuedBtn::Press(Btn::Jump) => {
            return if flag(w, f, n, "CanBranchToJump") { out(ResolveBufferedKind::Jump, None) } else { None };
        }
        QueuedBtn::Press(Btn::Dodge) => {
            return if flag(w, f, n, "CanBranchToEvade") { out(ResolveBufferedKind::Dodge, None) } else { None };
        }
        _ => {}
    }
    let hd = match q.press {
        QueuedBtn::Taunt => Dir::Default as i32,
        _ => q.force.map_or(q.dir as i32, |x| x.dir),
    };
    if !hold_ok(w, f, Some(q.press), hd) {
        return None;
    }
    let b = match q.press {
        QueuedBtn::Taunt => {
            let t = q.taunt.as_deref()?;
            return if flag(w, f, n, "CanBranchToTaunt") && branch_ok(w, f, t) {
                out(ResolveBufferedKind::Branch, Some(t.clone()))
            } else {
                None
            };
        }
        QueuedBtn::Press(b) => b,
    };
    // An automatic window is skipped here (@3922): its rows belong to the manager's tick (`poll_auto_branches`).
    if flag(w, f, n, "MakeBranchesAutomatic") {
        return None;
    }
    let item = item_override(w, f, b, q.raw);
    if let Some(item) = item {
        let s = w.names.str(item).to_string();
        let cur = w.fighter(f).branching.branch_montage;
        if !exclude_of(w, f, n).contains(&s) && (can_same_attack(w, f, n) || cur != Some(item)) {
            return out(ResolveBufferedKind::Branch, Some(override_branch(w, item)));
        }
    }
    if b == Btn::Item && w.fighter(f).branching.override_packs.is_none() {
        return None;
    }
    let sd = swapped_dir(w, f, Some(b), q.dir, in_air(w, f));
    let (br, _) = pick_window_branch(w, f, &[*w2], Some(b), sd, q.raw)?;
    out(ResolveBufferedKind::Branch, Some(br))
}

/// `answersNoPress`: a buffered window can yield an attack with nothing stored.
pub fn answers_no_press(w: &mut World, f: FighterId, n: NotifyRef) -> bool {
    let Some(p) = props(w, f, n) else { return false };
    if truthy(p.get("Immediate")) || truthy(p.get("MakeBranchesAutomatic")) {
        return false;
    }
    let mut seen = Vec::new();
    let bgs = &super::montages::notify_owner_data(&w.data, w, f, n).branch_groups;
    no_press_row(bgs, p.get("Branches"), &strs(p, "BranchGroups"), &mut seen) || pack_answers_no_press(w, f)
}

/// `noPressRow`: a row of button None and direction Default among a window's collected branches; a group the
/// window owner's data (`bgs`) does not have is passed over without the warning.
fn no_press_row(
    bgs: &mvs_data::fighter::Map<mvs_data::fighter::BranchGroup>,
    rows: Option<&Value>,
    groups: &[Option<String>],
    seen: &mut Vec<String>,
) -> bool {
    let none_row = |v: &Value| {
        v.as_obj().is_some_and(|p| {
            p.get("Button").and_then(Value::as_str) == Some("EActionButton::AB_None")
                && p.get("Direction").and_then(Value::as_str) == Some("EActionDirection::AD_Default")
        })
    };
    if rows.and_then(Value::as_arr).is_some_and(|a| a.iter().any(none_row)) {
        return true;
    }
    for g in groups.iter().flatten() {
        if g.is_empty() || seen.contains(g) {
            continue;
        }
        seen.push(g.clone());
        let Some(bg) = bgs.get(g) else { continue };
        if bg.branches.as_ref().is_some_and(|r| {
            r.iter().any(|b| b.button == "EActionButton::AB_None" && b.direction == "EActionDirection::AD_Default")
        }) {
            return true;
        }
        let sub: Vec<Option<String>> = bg.branch_groups.iter().flatten().map(|s| Some(s.clone())).collect();
        if no_press_row(bgs, None, &sub, seen) {
            return true;
        }
    }
    false
}

/// `resolveUnpressed`: SetActiveAttackFromBranch for a buffered window that ends with
/// nothing stored: button None, direction Default; a row without a montage is no attack (@2367).
pub fn resolve_unpressed(w: &mut World, f: FighterId, w2: &ResolveUnpressedW) -> Option<Branch> {
    if wall_sliding(w, f) || !win_conds_ok(w, f, w2.n) {
        return None;
    }
    if flag(w, f, w2.n, "MakeBranchesAutomatic") {
        return None;
    }
    let (br, _) = pick_window_branch(w, f, &[*w2], None, Dir::Default, Dir::Default)?;
    if br.attack.is_empty() { None } else { Some(br) }
}

/// `pickWindowBranch`: BranchManagerGetAttack over the windows' branches for a button
/// (`None`: the button None) and direction; `raw` the unmodified direction the override check takes.
/// Unverified: Map_Keys order is taken as the order the windows began in.
pub fn pick_window_branch(
    w: &mut World,
    f: FighterId,
    wins: &[Window],
    b: Option<Btn>,
    d: Dir,
    raw: Dir,
) -> Option<(Branch, Window)> {
    let btn_name = match b {
        None => "EActionButton::AB_None",
        Some(Btn::Attack) => "EActionButton::AB_HeavyAttack",
        Some(_) => "EActionButton::AB_SpecialAttack",
    };
    let want = if d == Dir::Default { "AD_Default".to_string() } else { w.names.str(dir_name(d)).to_string() };
    let mut ws = wins.to_vec();
    if wins.len() > 1 {
        let began = active_keys(w, f); // the set keeps the order the notifies began in
        // a window of another anim instance (a key below 0) behind them, in the order they were filed
        let rank = |k: i32| -> i64 {
            if k < 0 {
                i64::try_from(began.len()).unwrap_or(i64::MAX) - i64::from(k)
            } else {
                began.iter().position(|&x| x == k).map_or(-1, |p| i64::try_from(p).unwrap_or(i64::MAX))
            }
        };
        ws.sort_by_key(|x| rank(x.i));
    }
    let packs = w.fighter(f).branching.override_packs.is_some();
    for x in ws {
        // IsBranchValid @49 and the override checks read the manager's CurrentMontage, which need not be the
        // window's own
        let wcur = w.fighter(f).branching.branch_montage;
        if packs {
            let ex = exclude_names(w, &exclude_of(w, f, x.n));
            let same = can_same_attack(w, f, x.n);
            if bucket_has_rows(w, f, x.n, b, &want) {
                match override_window_branch(w, f, b, raw, Some(&ex), same, wcur) {
                    Override::Row(o) => return Some((o, x)),
                    Override::Refused => continue,
                    Override::None => {}
                }
            } else {
                if let Override::Row(a) = override_attack(w, f, b, raw, Some(&ex), same, wcur) {
                    return Some((row_branch(w, &a), x));
                }
                continue; // no row for this press in the window
            }
        }
        if b == Some(Btn::Item) {
            continue;
        }
        let mut best: Option<Branch> = None;
        let mut best_pr = -2;
        for br in window_options(w, f, x.n, wcur) {
            if br.button != btn_name || strip_enum(&br.direction) != want {
                continue;
            }
            let pr = if want == "AD_Default" { -1 } else { br.priority }; // GetBranchPriority
            if pr <= best_pr || !branch_ok(w, f, &br) {
                continue;
            }
            best = Some(br);
            best_pr = pr;
        }
        if let Some(br) = best {
            return Some((br, x));
        }
    }
    None
}

/// `completeLateBranch`: CompleteBranchNotify of the windows of a montage stopped late,
/// each window's own end in notify order, the first that triggers ending it.
/// Deviation: a refused jump / evade window's own entry stays in CurrentBranchData in the build (@1356); not modelled.
pub fn complete_late_branch(w: &mut World, f: FighterId) {
    let b = &w.fighter(f).branching;
    if b.late_branch.is_none() || !b.late_branch_due {
        return;
    }
    let Some(lb) = w.fighter_mut(f).branching.late_branch.take() else { return };
    if in_hitstun(w, f) || dead(w, f) {
        return;
    }
    for &(n, i) in &lb.ws {
        let win = Window { n, i };
        if let Some(q) = lb.q.as_ref().filter(|q| q.windows.contains(&i)) {
            let p = queued_for(q, i);
            if let Some(r) = resolve_buffered(w, f, &win, &p)
                && execute(w, f, exec_kind(r.kind), r.br.as_ref(), p.force, p.dir)
            {
                return;
            }
            continue;
        }
        if let Some(br) = resolve_unpressed(w, f, &win)
            && execute(w, f, ExecuteKind::Branch, Some(&br), None, Dir::Default)
        {
            return;
        }
    }
}

/// `bucketHasRows`: one bucket of a window's BranchInputMappings has a row: the row of the press's button and the
/// column of its direction (`want`: the folded direction's name).
fn bucket_has_rows(w: &World, f: FighterId, n: NotifyRef, b: Option<Btn>, want: &str) -> bool {
    let row = match b {
        Some(Btn::Special) => 1,
        None => 2,
        Some(_) => 0,
    };
    let col = dir_column(want);
    all_branches(w, f, n).iter().any(|br| {
        let r = match strip_enum(&br.button) {
            "AB_HeavyAttack" | "AB_BasicAttack" => 0,
            "AB_SpecialAttack" => 1,
            "AB_None" => 2,
            _ => -1,
        };
        r == row && dir_column(strip_enum(&br.direction)) == col
    })
}

fn dir_column(d: &str) -> i32 {
    if d == "AD_Default" {
        0
    } else if d == "AD_Neutral" {
        1
    } else if d.ends_with("Up") {
        2
    } else if d.ends_with("Down") {
        4
    } else {
        3
    }
}

/// `windowOptions`: Branches + BranchGroups (recursive, in order), minus the excludes and
/// (unless allowed) the current attack `cur`.
pub fn window_options(w: &World, f: FighterId, n: NotifyRef, cur: Option<Name>) -> Vec<Branch> {
    let mut ex = exclude_of(w, f, n);
    if !can_same_attack(w, f, n)
        && let Some(c) = cur
    {
        ex.push(w.names.str(c).to_string());
    }
    all_branches(w, f, n).into_iter().filter(|b| !ex.contains(&b.attack)).collect()
}

/// `allBranches`: CollectAllBranches: the window's Branches, then its BranchGroups' (recursive, each group once).
fn all_branches(w: &World, f: FighterId, n: NotifyRef) -> Vec<Branch> {
    let Some(p) = props(w, f, n) else { return Vec::new() };
    let mut out: Vec<Branch> =
        p.get("Branches").and_then(Value::as_arr).map(|a| a.iter().filter_map(branch_of).collect()).unwrap_or_default();
    // The group assets the window names, in the data of the character whose montage holds it (a Disguise copy's
    // window: the victim's; the build resolves the asset itself).
    let bgs = &super::montages::notify_owner_data(&w.data, w, f, n).branch_groups;
    let mut seen: Vec<String> = Vec::new();
    fn add(
        w: &World,
        g: &str,
        bgs: &mvs_data::fighter::Map<mvs_data::fighter::BranchGroup>,
        seen: &mut Vec<String>,
        out: &mut Vec<Branch>,
    ) {
        // a None entry names no group (C019_Attack_Arm_Combo1's Immediate window)
        if g.is_empty() || seen.iter().any(|s| s == g) {
            return;
        }
        seen.push(g.to_string());
        let Some(bg) = bgs.get(g) else {
            warn_text(w, &format!("missing branch group {g}"));
            return;
        };
        out.extend(bg.branches.iter().flatten().cloned());
        for s in bg.branch_groups.iter().flatten() {
            add(w, s, bgs, seen, out);
        }
    }
    for g in strs(p, "BranchGroups").iter().flatten() {
        add(w, g, bgs, &mut seen, &mut out);
    }
    out
}

/// `rowOk` for a branch row: its Conditions, then with AbilitySlot >= 0 the ability
/// manager's TriggerAbilityCheck.
pub fn branch_ok(w: &mut World, f: FighterId, b: &Branch) -> bool {
    let c = names_of(w, b.conditions.iter().map(|c| c.as_deref()));
    conds_ok(w, f, Some(&c), false) && b.ability_slot.is_none_or(|s| s < 0 || trigger_ability_check(w, f, s))
}

/// `rowOk`: an attack table row or a branch.
pub fn row_ok(w: &mut World, f: FighterId, r: &RowOkR) -> bool {
    let c: Option<Vec<Option<Name>>> = r.conditions.as_ref().map(|c| c.iter().map(|&n| Some(n)).collect());
    conds_ok(w, f, c.as_deref(), false) && r.ability_slot.is_none_or(|s| s < 0 || trigger_ability_check(w, f, s))
}

/// `condsOk`: a condition list as BaseFighter.BPI_CheckConditions walks it; an empty slot
/// is skipped (IsValidClass @253-282). `direct`: the loops that call Check without a cast, where an empty slot warns
/// and fails.
pub fn conds_ok(w: &mut World, f: FighterId, conds: Option<&[Option<Name>]>, direct: bool) -> bool {
    let Some(conds) = conds else { return true };
    for &c in conds {
        let Some(c) = c else {
            if !direct {
                continue;
            }
            warn_text(w, "empty condition slot in a list checked without a cast");
            return false;
        };
        if !cond(w, f, c) {
            return false;
        }
    }
    true
}

/// `cond`: a condition object's Check, by its class; a class with variables checks the
/// fighter's object of the class (BaseFighter.GetCondition @10252 makes one per fighter and class).
pub fn cond(w: &mut World, f: FighterId, c: Name) -> bool {
    match w.registry.conditions.get(w.names.str(c)).copied() {
        Some(Condition::Fn(check)) => check(w, f),
        Some(Condition::Class(k)) => conditions::check(w, f, c, k),
        None => {
            warn_text(w, &format!("unknown condition {}", w.names.str(c)));
            false
        }
    }
}

/// `condReset`: Condition.Reset(owner) on the fighter's object of the class; a class with
/// no variables has Condition2's empty Reset.
pub fn cond_reset(w: &mut World, f: FighterId, c: Name) {
    match w.registry.conditions.get(w.names.str(c)).copied() {
        Some(Condition::Fn(_)) => {}
        Some(Condition::Class(k)) => conditions::reset(w, f, c, k),
        None => warn_text(w, &format!("unknown condition {}", w.names.str(c))),
    }
}

/// `condResetOk`: Reset then Check of one class (AnimNotify_ButtonHoldSectionJump Tick
/// @791 / @836).
pub fn cond_reset_ok(w: &mut World, f: FighterId, c: Name) -> bool {
    cond_reset(w, f, c);
    cond(w, f, c)
}

/// `condsResetOk`: each Condition's Reset then its Check, in list order, the first false
/// one ending it (AnimNotify_ActivateSpawned Begin @5-791 / End @785-885); empty slots skipped.
pub fn conds_reset_ok(w: &mut World, f: FighterId, conds: Option<&[Option<Name>]>) -> bool {
    let Some(conds) = conds else { return true };
    for &c in conds.iter().flatten() {
        cond_reset(w, f, c);
        if !cond(w, f, c) {
            return false;
        }
    }
    true
}

/// `condSignal`: an owner broadcast a condition object binds a handler to: each of the
/// fighter's objects whose class listens is told.
pub fn cond_signal(w: &mut World, f: FighterId, sig: CondSignal) {
    let reg = w.registry;
    let names = w.names.clone();
    let Some(m) = &mut w.fighter_mut(f).buffs.cond_objs else { return };
    for (c, s) in m.iter_mut() {
        if let Some(Condition::Class(k)) = reg.conditions.get(names.str(*c)) {
            conditions::signal(s, *k, sig);
        }
    }
}

/// `pollAutoBranches`: Comp_Pawn_BranchManager ReceiveTick (@15247-15847): while an
/// automatic window is filed, SetActiveAttackFromBranch(auto) tries every one each tick in the order they were filed.
/// Unverified: its order against the MoveCommandManager's tick in the same group is taken from the rollback driver's
/// (BaseFighter ubergraph 180130, 180242: the move commands first); the stock order is native, unread.
pub fn poll_auto_branches(w: &mut World, f: FighterId) {
    let la = w.fighter(f).branching.late_auto.as_ref().map(|l| l.ws.clone());
    let late = la.is_some();
    let mut ws: Option<Vec<NotifyRef>> = la;
    if ws.is_none() {
        for (k, n) in main_windows(w, f) {
            if flag(w, f, n, "MakeBranchesAutomatic") && !cleared(w, f, k) {
                ws.get_or_insert_with(Vec::new).push(n);
            }
        }
    }
    if !late {
        // the automatic windows of the other attack instances, behind instance 0's
        for (k, n) in grouped_windows(w, f, false) {
            if flag(w, f, n, "MakeBranchesAutomatic") && !cleared(w, f, k) {
                ws.get_or_insert_with(Vec::new).push(n);
            }
        }
        // ... and those of another anim instance the pawn filed (`file_window`)
        let fw: Vec<NotifyRef> = w.fighter(f).branching.foreign_wins.iter().flatten().map(|e| e.n).collect();
        for n in fw {
            if flag(w, f, n, "MakeBranchesAutomatic") {
                ws.get_or_insert_with(Vec::new).push(n);
            }
        }
    }
    let Some(ws) = ws else { return };
    if wall_sliding(w, f) {
        return;
    }
    let mut pick: Option<Branch> = None;
    for n in ws {
        if !win_conds_ok(w, f, n) {
            continue;
        }
        // the rows against CurrentMontage (IsBranchValid @49)
        let cur = w.fighter(f).branching.branch_montage;
        let mut opts = Vec::new();
        for b in window_options(w, f, n, cur) {
            if auto_row_ok(w, f, &b) {
                opts.push(b);
            }
        }
        if opts.is_empty() {
            continue;
        }
        opts.sort_by_key(|b| std::cmp::Reverse(b.priority));
        pick = opts.into_iter().next();
        if flag(w, f, n, "Immediate") {
            break;
        }
    }
    let Some(pick) = pick else { return };
    // @15738-15820: InputAxisX / InputAxisY = the pawn's pair of this moment, before Trigger Current Attack Client
    let fi = w.fighter_mut(f);
    fi.branching.br_axis_x = fi.controls.inp.x;
    fi.branching.br_axis_y = fi.controls.inp.y;
    fi.branching.late_auto = None;
    clear_foreign_windows(w, f, false); // the trigger's exit clears the maps, in hitstun or dead too (@5754 -> @5768)
    if in_hitstun(w, f) || dead(w, f) {
        return;
    }
    let special = strip_enum(&pick.button) == "AB_SpecialAttack";
    let attack = w.names.get(&pick.attack).unwrap_or_default();
    // Trigger Current Attack Client's attack path @8639-8666: AddRecentAttackToQueue ahead of the stop @8712
    add_recent_attack(w, f, if special { Btn::Special } else { Btn::Attack }, Some(attack));
    branch_row_tutorial(w, f, &pick.button, &pick.direction);
    let conds: Vec<Name> = pick.conditions.iter().flatten().map(|c| w.names.get(c).unwrap_or_default()).collect();
    trigger_air_special_cost(w, f, special, Some(&conds));
    attack_begin(w, f, true);
    w.fighter_mut(f).abilities.attack_slot = pick.ability_slot.unwrap_or(-1);
    super::grouped::stop_active_grouped(w, f); // Comp_Pawn_BranchManager @8712, before the turn (@9342)
    if pick.allow_pre_attack_rotation {
        let b = &w.fighter(f).branching;
        let (x, rsx) = (b.br_axis_x, b.br_stick_x);
        pre_attack_rotation(w, f, None, Dir::Default, x, rsx);
    }
    start_montage(w, f, attack, &2, None); // Trigger Current Attack Client's play @12224 ('branchAttack')
    clear_late(w, f); // Trigger Current Attack Client clears the maps
}

/// `autoRowOk`: IsBranchValid with AutoBranchesOnlyParam (@263-1256): a row of direction
/// Default or of the stick's 8-way direction, under its conditions and its ability check.
pub fn auto_row_ok(w: &mut World, f: FighterId, b: &Branch) -> bool {
    let bd = strip_enum(&b.direction);
    let fi = w.fighter(f);
    let d8 = get8_way(fi.controls.inp.x, fi.controls.inp.y, fi.body.facing_dir);
    (bd == "AD_Default" || bd == w.names.str(dir_name(d8))) && branch_ok(w, f, b)
}

fn block_same(w: &mut World, f: FighterId, _n: NotifyRef, _k: i32) {
    set_block_same_attack_branch(w, f, true);
}

/// The branch notifies: AnimNotify_BlockSameAttackBranch, a notify state whose Received_NotifyBegin
/// @229 and Received_NotifyEnd @229 both call SetBlockSameAttackBranch(true) (the class has no call with false).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_BlockSameAttackBranch_C",
        NotifyHandler { begin: Some(block_same), end: Some(block_same), ..Default::default() },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(dir: Dir) -> QueuedPress {
        QueuedPress { press: QueuedBtn::Press(Btn::Attack), dir, force: None, taunt: None, raw: dir }
    }

    #[test]
    fn a_window_reads_its_own_older_press_else_the_newest() {
        let mut q = QueuedAction::of(press(Dir::Up), vec![3, 7]);
        q.older = Some(IndexMap::from([(7, press(Dir::Down))]));
        assert_eq!(queued_for(&q, 3).dir, Dir::Up);
        assert_eq!(queued_for(&q, 7).dir, Dir::Down);
    }

    #[test]
    fn the_mapping_columns_fold_the_diagonals() {
        assert_eq!(strip_enum("EActionDirection::AD_ForwardUp"), "AD_ForwardUp");
        assert_eq!(strip_enum("AD_Up"), "AD_Up");
        let cols: Vec<i32> = ["AD_Default", "AD_Neutral", "AD_BackUp", "AD_Forward", "AD_Back", "AD_ForwardDown"]
            .iter()
            .map(|d| dir_column(d))
            .collect();
        assert_eq!(cols, [0, 1, 2, 3, 3, 4]);
    }

    #[test]
    fn dropping_the_last_window_drops_the_press() {
        let mut b = Branching { queued: Some(QueuedAction::of(press(Dir::Up), vec![-1, 4])), ..Default::default() };
        drop_queued_keys(&mut b, &[-1]);
        assert_eq!(b.queued.as_ref().map(|q| q.windows.clone()), Some(vec![4]));
        drop_queued_keys(&mut b, &[4]);
        assert!(b.queued.is_none());
    }
}
