//! The montage instances of the other slot groups: Montage_Play of a montage whose slot group
//! is not DefaultGroup, GetCurrentActiveMontage over both sets, their stops and their part of the montage update. The
//! rules and their sources are in the comments of the functions.

// A written-out `!(a > b)` is true for a NaN where `a <= b` is not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::montages::{
    COPY_INST, KEY_STRIDE, drop_state, new_instance, notify_ticks, running, sound_event, state_ref, text, u32x,
    with_player,
};
use super::{GroupedEntry, MontageSlot, SinkKind};
use crate::Real;
use crate::consts;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::names::Name;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::world::World;
use mvs_anim::montage::{Notify, Play, blend_ticks};

/// `layersInto`: the grouped instances' layers (slot, anim, time, weight) into the caller's
/// lists; returns their count.
pub fn layers_into(
    w: &World,
    f: FighterId,
    slot: &mut Vec<String>,
    anim: &mut Vec<String>,
    time: &mut Vec<Real>,
    wt: &mut Vec<Real>,
) -> usize {
    slot.clear();
    anim.clear();
    time.clear();
    wt.clear();
    let Some((l, _)) = &w.fighter(f).montages.grouped else { return 0 };
    for e in l {
        let Some(md) = super::montage_data(w, f, e.m.montage.name) else { continue };
        let ew = match e.fade {
            Some([left, of, _, fw]) => {
                if of > 0.0 {
                    (fw * left) / of
                } else {
                    0.0
                }
            }
            None => crate::math::narrow(e.m.weight(md)),
        };
        if !(ew > 1e-5) {
            continue;
        }
        for s in &md.slots {
            let Some(g) = e.m.seg_at(&s.segments) else { continue };
            let seg = &s.segments[g];
            slot.push(s.slot.clone());
            anim.push(seg.anim.clone());
            time.push(crate::math::narrow(e.m.seg_time(seg)));
            wt.push(ew);
        }
    }
    slot.len()
}

/// `playGrouped`.
pub fn play_grouped(w: &mut World, f: FighterId, name: Name, stop_group: bool, attack: bool) {
    let data = w.data.clone();
    let Some(m) = super::montages::char_data(&data, &w.names, w.fighter(f).ch)
        .and_then(|c| c.fighter.montages.get(w.names.str(name)))
    else {
        warn_text(w, &format!("missing montage {}", w.names.str(name)));
        return;
    };
    let Some(group) = m.group.as_deref() else {
        warn_text(w, &format!("playGrouped: {} is a DefaultGroup montage", w.names.str(name)));
        return;
    };
    super::root_motion::not_root_instance(w, f, m);
    if w.fighter(f).montages.grouped.is_none() {
        let ms = &mut w.fighter_mut(f).montages;
        ms.grouped = Some((Vec::new(), false));
        ms.grouped_gen += 1;
    }
    if stop_group {
        let n = w.fighter(f).montages.grouped.as_ref().map_or(0, |g| g.0.len());
        for i in (0..n).rev() {
            let same = {
                let e = &w.fighter(f).montages.grouped.as_ref().expect("grouped").0[i];
                super::montage_data(w, f, e.m.montage.name).and_then(|x| x.group.as_deref()) == Some(group)
            };
            if same {
                stop_grouped_entry(w, f, i, crate::math::narrow(m.blend_in), true);
            }
        }
    }
    let g = w.fighter(f).body.on_floor;
    let st = new_instance(w, f, name, g);
    let fi = w.fighter_mut(f);
    let mut e = GroupedEntry { m: st, fade: None, top: false, inst: None, charge: false };
    if fi.montages.montage.is_some() || fi.montages.dash_montage.is_some() {
        e.top = true;
    }
    let list = &mut fi.montages.grouped.as_mut().expect("grouped").0;
    if attack {
        let mut inst = 1;
        while list.iter().any(|x| x.inst == Some(inst)) {
            inst += 1;
        }
        e.inst = Some(inst);
    }
    list.push(e);
    let nav = super::nav_of(w, f);
    let n = w.names.str(name);
    let dash = nav.dash.as_deref() == Some(n) || nav.dash_turn.as_deref() == Some(n);
    super::anim_montage_start(w, f, dash);
    if let Some(h) = w.fighter(f).hooks.char.montage_started {
        h(w, f, name);
    }
}

fn entry_live(e: &GroupedEntry) -> bool {
    !(e.fade.is_some() || e.m.terminated || e.m.ended || e.m.blending_out())
}

/// `activeGrouped`: the entry's index in the list.
pub fn active_grouped(w: &World, f: FighterId) -> Option<usize> {
    let fi = w.fighter(f);
    let (l, _) = fi.montages.grouped.as_ref()?;
    for (i, e) in l.iter().enumerate().rev() {
        if !entry_live(e) {
            continue;
        }
        let def = fi.montages.montage.is_some() || fi.montages.dash_montage.is_some();
        return if e.top || !def { Some(i) } else { None };
    }
    None
}

/// `activeInstance`.
pub fn active_instance(w: &World, f: FighterId) -> Option<MontageSlot> {
    let ms = &w.fighter(f).montages;
    let def = match (&ms.montage, &ms.dash_montage) {
        (Some(m), _) if !m.blending_out() => Some(MontageSlot::Main),
        (_, Some(d)) if !d.blending_out() => Some(MontageSlot::Dash),
        _ => None,
    };
    if let Some((l, _)) = &ms.grouped {
        for (i, e) in l.iter().enumerate().rev() {
            if !entry_live(e) {
                continue;
            }
            if e.top || def.is_none() {
                return Some(MontageSlot::Grouped(u32x(i)));
            }
            break;
        }
    }
    def
}

/// `groupedUnder`.
pub fn grouped_under(w: &mut World, f: FighterId) {
    if let Some((l, _)) = &mut w.fighter_mut(f).montages.grouped {
        for e in l {
            e.top = false;
        }
    }
}

/// `stopAnimMontage`.
pub fn stop_anim_montage(w: &mut World, f: FighterId) {
    if stop_active_grouped(w, f) {
        return;
    }
    let data = w.data.clone();
    let fi = w.fighter(f);
    let d = fi.montages.dash_montage.as_ref().map(|d| (d.montage.uid, d.blending_out()));
    if fi.montages.montage.is_some() {
        super::montages::stop_montage(w, f, None, None);
    } else if let Some((u, false)) = d {
        let bo = super::montages::instance_data(&data, w, f, u).map_or(0.0, |m| m.blend_out);
        super::montages::stop_dash_montage(w, f, bo);
    }
}

/// `stopActiveGrouped`.
pub fn stop_active_grouped(w: &mut World, f: FighterId) -> bool {
    let Some(i) = active_grouped(w, f) else { return false };
    let name = w.fighter(f).montages.grouped.as_ref().expect("grouped").0[i].m.montage.name;
    let bo = super::montage_data(w, f, name).map_or(0.0, |m| m.blend_out);
    stop_grouped_entry(w, f, i, crate::math::narrow(bo), false);
    true
}

/// `stopGrouped`: `name` None for every instance.
pub fn stop_grouped(w: &mut World, f: FighterId, name: Option<Name>, blend: Real) {
    let Some((l, _)) = &w.fighter(f).montages.grouped else { return };
    match name {
        None => {
            let uids: Vec<u32> = l.iter().map(|e| e.m.montage.uid).collect();
            for u in uids {
                if let Some(i) = index_of(w, f, u) {
                    stop_grouped_entry(w, f, i, blend, false);
                }
            }
        }
        Some(n) => {
            if let Some(i) = l.iter().rposition(|e| e.m.montage.name == n) {
                stop_grouped_entry(w, f, i, blend, false);
            }
        }
    }
}

fn index_of(w: &World, f: FighterId, uid: u32) -> Option<usize> {
    w.fighter(f).montages.grouped.as_ref()?.0.iter().position(|e| e.m.montage.uid == uid)
}

/// Runs `charge_blend_out` on the entry at `i` (taken out of the list for the call).
fn charge_blend_out_at(w: &mut World, f: FighterId, i: usize) {
    let Some(mut e) = w.fighter_mut(f).montages.grouped.as_mut().map(|g| g.0.remove(i)) else { return };
    crate::fighter::control::charge_blend_out(w, f, &mut e);
    if let Some((l, _)) = &mut w.fighter_mut(f).montages.grouped {
        l.insert(i.min(l.len()), e);
    }
}

/// `stopGroupedEntry`: the entry at `i` of the list.
pub fn stop_grouped_entry(w: &mut World, f: FighterId, i: usize, blend: Real, play: bool) {
    let data = w.data.clone();
    let Some(e) = w.fighter_mut(f).montages.grouped.as_mut().and_then(|g| g.0.get_mut(i)) else { return };
    if let Some([left, of, time, fw]) = e.fade {
        if play && blend < time {
            let l = blend_ticks(blend);
            e.fade = Some([
                crate::math::narrow(l),
                crate::math::narrow(l),
                blend,
                if of > 0.0 { (fw * left) / of } else { 0.0 },
            ]);
        }
        return;
    }
    if e.m.terminated || e.m.ended {
        return;
    }
    if e.m.blending_out() && !play {
        return;
    }
    let uid = e.m.montage.uid;
    let Some(md) = super::montages::instance_data(&data, w, f, uid) else { return };
    let e = &mut w.fighter_mut(f).montages.grouped.as_mut().expect("grouped").0[i];
    let wt = e.m.weight(md);
    let b = e.m.stop_blend(blend);
    e.fade =
        Some([crate::math::narrow(b.0), crate::math::narrow(b.0), crate::math::narrow(b.1), crate::math::narrow(wt)]);
    super::montage_blending_out(w, f, uid);
    if let Some(i) = index_of(w, f, uid) {
        charge_blend_out_at(w, f, i);
    }
    if let Some(s) = super::montages::state_mut(w, f, uid) {
        s.ended = true; // halt
    }
}

/// `instGone`: the entry `e` has left the list.
fn inst_gone(w: &mut World, f: FighterId, mut e: GroupedEntry) {
    crate::fighter::control::charge_blend_out(w, f, &mut e);
    let name = e.m.montage.name;
    if !w.fighter(f).buffs.buffs.is_empty() {
        let arg = BuffSignalArg { montage: Some(name), interrupted: Some(e.fade.is_some()), ..Default::default() };
        crate::combat::buffs::buff_signal(w, f, BuffSignal::MontageEnded, arg);
    }
    let Some(inst) = e.inst.filter(|&i| i != 0) else { return };
    let fi = w.fighter_mut(f);
    if fi.charge.charge_notify >= 0 && fi.charge.charge_notify.div_euclid(KEY_STRIDE) == inst {
        fi.charge.charge_notify = -1;
    }
    if let Some(wc) = &fi.branching.win_cleared {
        let rest: Vec<i32> = wc.iter().copied().filter(|k| k.div_euclid(KEY_STRIDE) != inst).collect();
        fi.branching.win_cleared = if rest.is_empty() { None } else { Some(rest) };
    }
    if w.fighter(f).shapes.disguise.is_some() {
        crate::combat::disguise::owner_montage_event(w, f, name);
    }
    if let Some(h) = w.fighter(f).hooks.char.montage_ended {
        h(w, f, name);
    }
}

/// The sink of an entry: an attack instance's is the fighter's own under its keys, another's the grouped sink.
fn sink_of(e: &GroupedEntry) -> (SinkKind, i32) {
    match e.inst {
        Some(i) if i != 0 => (SinkKind::Fighter, i * KEY_STRIDE),
        _ => (SinkKind::Grouped, 0),
    }
}

/// `groupedLateEnds`.
pub fn grouped_late_ends(w: &mut World, f: FighterId) {
    let gen0 = w.fighter(f).montages.grouped_gen;
    let n = w.fighter(f).montages.grouped.as_ref().map_or(0, |g| g.0.len());
    for i in 0..n {
        let Some(e) = w.fighter(f).montages.grouped.as_ref().and_then(|g| g.0.get(i)) else { break };
        if e.fade.is_none() || e.m.active.is_empty() {
            continue;
        }
        let (sink, base) = sink_of(e);
        let uid = e.m.montage.uid;
        with_player(w, f, uid, sink, base, |h| h.end_queued());
        if w.fighter(f).montages.grouped.is_none() || w.fighter(f).montages.grouped_gen != gen0 {
            return;
        }
    }
}

/// `clearGrouped`.
pub fn clear_grouped(w: &mut World, f: FighterId) {
    let Some((l, _)) = &w.fighter(f).montages.grouped else { return };
    let entries: Vec<(u32, bool, SinkKind, i32)> =
        l.iter().map(|e| (e.m.montage.uid, e.fade.is_some(), sink_of(e).0, sink_of(e).1)).collect();
    for (uid, fade, sink, base) in entries {
        if fade {
            with_player(w, f, uid, sink, base, |h| {
                h.end_branching();
                h.end_queued();
            });
        } else {
            with_player(w, f, uid, sink, base, |h| h.stop());
        }
    }
    let fi = w.fighter_mut(f);
    if fi.charge.charge_notify >= KEY_STRIDE && fi.charge.charge_notify < COPY_INST * KEY_STRIDE {
        fi.charge.charge_notify = -1;
    }
    if let Some(wc) = &fi.branching.win_cleared {
        let rest: Vec<i32> = wc.iter().copied().filter(|&k| k < KEY_STRIDE).collect();
        fi.branching.win_cleared = if rest.is_empty() { None } else { Some(rest) };
    }
    if let Some((l, _)) = w.fighter_mut(f).montages.grouped.take() {
        for e in l {
            drop_state(w, f, e.m);
        }
    }
}

/// `groupedDone`.
pub fn grouped_done(w: &mut World, f: FighterId) {
    let ms = &mut w.fighter_mut(f).montages;
    if let Some((l, tenticle)) = &ms.grouped
        && l.is_empty()
        && !tenticle
    {
        ms.grouped = None;
    }
}

/// `groupedSink`: the classes entered in the registry and the Ak one-shot sounds.
pub(crate) fn grouped_sink(
    w: &mut World,
    f: FighterId,
    n: &Notify,
    r: NotifyRef,
    key: i32,
    begin: bool,
    instant: bool,
) {
    let h = w.registry.notifies.get(&n.cls).copied().filter(|h| !h.strict);
    let other = || warn_text(w, &format!("notify class not handled on a grouped montage: {}", n.cls));
    match (h, begin, instant) {
        (Some(h), _, true) => {
            if let Some(i) = h.instant {
                i(w, f, r);
            }
        }
        (None, _, true) => {
            if n.cls == "Ak_OneShot_C" || n.cls == "Ak_VO_Effort_C" {
                if let Some(ev) = text(&n.p, "AkEvent").filter(|s| !s.is_empty()) {
                    sound_event(w, f, ev);
                }
            } else {
                other();
            }
        }
        (Some(h), true, _) => {
            if let Some(b) = h.begin {
                b(w, f, r, key);
            }
        }
        (Some(h), false, _) => {
            if let Some(e) = h.end {
                e(w, f, r, key);
            }
        }
        (None, _, _) => other(),
    }
}

/// `advanceGrouped`.
pub fn advance_grouped(w: &mut World, f: FighterId, held: bool, dilation: Real) {
    let dilated = dilation != 1.0;
    let gen0 = w.fighter(f).montages.grouped_gen;
    let uids: Vec<u32> = match &w.fighter(f).montages.grouped {
        Some((l, _)) if !l.is_empty() => l.iter().map(|e| e.m.montage.uid).collect(),
        _ => return,
    };
    let same = |w: &World| w.fighter(f).montages.grouped.is_some() && w.fighter(f).montages.grouped_gen == gen0;
    let mut skip = held;
    for uid in uids {
        if !same(w) {
            return;
        }
        let Some(i) = index_of(w, f, uid) else { continue };
        let (sink, base, fade) = {
            let e = &w.fighter(f).montages.grouped.as_ref().expect("grouped").0[i];
            let (s, b) = sink_of(e);
            (s, b, e.fade)
        };
        if let Some(mut fd) = fade {
            let mut gone: Option<GroupedEntry> = None;
            if fd[0] > 0.0 && !dilated {
                fd[0] -= 1.0;
            }
            w.fighter_mut(f).montages.grouped.as_mut().expect("grouped").0[i].fade = Some(fd);
            running(w, f, uid, |w| {
                if !skip {
                    if fd[0] <= 0.0 {
                        let mut e = w.fighter_mut(f).montages.grouped.as_mut().expect("grouped").0.remove(i);
                        let k = e.m.montage;
                        let st = std::mem::replace(&mut e.m, super::MontageState::new(k, false));
                        w.fighter_mut(f).montages.limbo.push(st);
                        skip = true;
                        gone = Some(e);
                        with_player(w, f, uid, sink, base, |h| h.end_branching());
                    } else if fd[2] > 0.0 {
                        let data = w.data.clone();
                        if let Some(md) = super::montages::instance_data(&data, w, f, uid)
                            && let Some(s) = super::montages::state_mut(w, f, uid)
                        {
                            s.drift(md, consts::DT * dilation);
                        }
                    }
                }
                if state_ref(w, f, uid).is_some_and(|s| !s.active.is_empty()) {
                    with_player(w, f, uid, sink, base, |h| h.end_queued());
                }
                if let Some(mut e) = gone {
                    if let Some(s) = state_ref(w, f, uid) {
                        e.m.clone_from(s);
                    }
                    inst_gone(w, f, e);
                }
            });
            continue;
        }
        let inst = w.fighter(f).montages.grouped.as_ref().expect("grouped").0[i].inst.unwrap_or(0);
        running(w, f, uid, |w| {
            let was = state_ref(w, f, uid).is_some_and(|s| s.terminated);
            let alive = with_player(w, f, uid, sink, base, |h| h.advance(consts::DT * dilation, skip, dilated))
                .unwrap_or(false);
            if state_ref(w, f, uid).is_some_and(|s| s.blending_out()) {
                super::montage_blending_out(w, f, uid);
                if let Some(i) = index_of(w, f, uid) {
                    charge_blend_out_at(w, f, i);
                }
            }
            if !same(w) {
                skip = true; // (the caller returns below)
                return;
            }
            let Some(i) = index_of(w, f, uid) else { return };
            if w.fighter(f).montages.grouped.as_ref().expect("grouped").0[i].fade.is_some() {
                return;
            }
            if !alive {
                let e = w.fighter_mut(f).montages.grouped.as_mut().expect("grouped").0.remove(i);
                inst_gone(w, f, e);
            } else {
                if !was && state_ref(w, f, uid).is_some_and(|s| s.terminated) {
                    skip = true;
                }
                if inst != 0 {
                    notify_ticks(w, f, MontageSlot::Grouped(u32x(i)), uid, inst);
                }
            }
        });
        if !same(w) {
            return;
        }
    }
    grouped_done(w, f);
}
