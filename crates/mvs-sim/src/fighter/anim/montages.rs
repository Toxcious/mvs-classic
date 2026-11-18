//! The DefaultGroup montage, its fade-outs, the notify ticks and the keys of the attack state,
//! with the fighter's side of the montage player: an instance's identity, where its state lives and the sinks its
//! notifies go to (`Fighter implements NotifySink`, `dashSink`, `plainSink`).
//!
//! A montage player is handed around by reference and compared by identity; here an instance is its
//! [`MontageKey`] (the montage and a number the fighter gives out), its state in one of the fighter's places (the
//! slots, the fades, the grouped list, a second mesh's play). A handler that drops an instance while the player
//! still runs it (an abort, a terminated stop) leaves it in `Montages::limbo` until that run ends, as a player held by
//! reference lives on in the caller's hands.

// A written-out `!(a > b)` is true for a NaN where `a <= b` is not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::*;
use crate::combat::ActiveHit;
use crate::combat::hit::{hit_packet, set_active_hit_props};
use crate::events::{Event, NotifyRef};
use crate::fighter::common::warn_text;
use crate::fighter::{Fighter, FighterId};
use crate::names::Name;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::world::World;
use crate::{Real, consts};
use indexmap::IndexSet;
use mvs_anim::montage::{
    Advanced, Host, Montage, MontageProxy, Notify, NotifyTick, PROXY_ATTACK, PROXY_NONE, Play, blend_ticks,
};
use mvs_data::value::{Props, Value};

/// `KEY_STRIDE`: an attack state's key is its instance's number x this + the notify's index.
pub const KEY_STRIDE: i32 = 4096;
/// `COPY_INST` / `POLY_INST`: the second meshes' instance numbers, above every number
/// `playGrouped` gives out.
pub const COPY_INST: i32 = 2048;
pub const POLY_INST: i32 = 2049;
/// `FOREIGN_INSTS`, in key order.
pub const FOREIGN_INSTS: [i32; 2] = [COPY_INST, POLY_INST];

/// The notify class the dash sink leaves out (`PLAIN_LEFT_OUT`).
const PLAIN_LEFT_OUT: &str = "AnimNotify_BranchWindow_C";

/// The move-command classes whose NotifyTick is AnimNotify_MoveCommand's (its CornerCheck).
const CORNER_CHECK_CLASSES: [Name; 4] = [
    crate::name!("AnimNotify_MoveCommand_C"),
    crate::name!("AnimNotify_EvadeMoveCommand_C"),
    crate::name!("AnimNotify_MoveIntoFly_C"),
    crate::name!("AnimNotify_ConditionalMoveCommand_C"),
];

/// A montage instance's identity: the montage played and the number its play was given (`Montages::play_seq`).
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub struct MontageKey {
    pub name: Name,
    pub uid: u32,
}

/// A new instance of montage `name` (`new MontagePlayer(m, ground ? 'ground' : 'air')`), numbered by the fighter.
pub fn new_instance(w: &mut World, f: FighterId, name: Name, ground: bool) -> MontageState {
    let ms = &mut w.fighter_mut(f).montages;
    ms.play_seq += 1;
    MontageState::new(MontageKey { name, uid: ms.play_seq }, ground)
}

pub(crate) fn u32x(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// A JavaScript number made an integer index (its `Math.floor` done by the caller).
pub(crate) fn int(x: Real) -> i32 {
    crate::math::to_int32(x)
}

pub(crate) fn ix(i: usize) -> i32 {
    i32::try_from(i).unwrap_or(i32::MAX)
}

fn ux(i: i32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

/// JavaScript truthiness of a property.
pub(crate) fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

pub(crate) fn num(p: &Props, k: &str) -> Option<Real> {
    p.get(k).and_then(Value::as_f64).map(crate::math::narrow)
}

pub(crate) fn text<'p>(p: &'p Props, k: &str) -> Option<&'p str> {
    p.get(k).and_then(Value::as_str)
}

// ---- where an instance's state lives

fn find_mut(fi: &mut Fighter, uid: u32) -> Option<&mut MontageState> {
    let hit = |s: &Option<MontageState>| s.as_ref().is_some_and(|s| s.montage.uid == uid);
    if hit(&fi.montages.montage) {
        return fi.montages.montage.as_mut();
    }
    if hit(&fi.montages.dash_montage) {
        return fi.montages.dash_montage.as_mut();
    }
    if let Some(k) = fi.montages.fading.iter().position(|x| x.m.montage.uid == uid) {
        return Some(&mut fi.montages.fading[k].m);
    }
    if let Some((l, _)) = &fi.montages.grouped
        && let Some(k) = l.iter().position(|x| x.m.montage.uid == uid)
    {
        return fi.montages.grouped.as_mut().map(|g| &mut g.0[k].m);
    }
    if let Some(k) = fi.montages.limbo.iter().position(|x| x.montage.uid == uid) {
        return Some(&mut fi.montages.limbo[k]);
    }
    let copy = fi.shapes.disguise.as_ref().and_then(|d| d.copy.as_ref()).is_some_and(|c| c.m.montage.uid == uid);
    if copy {
        return fi.shapes.disguise.as_mut().and_then(|d| d.copy.as_mut()).map(|c| &mut c.m);
    }
    let poly = fi.shapes.polymorph.as_ref().and_then(|d| d.play.as_ref()).is_some_and(|c| c.m.montage.uid == uid);
    if poly {
        return fi.shapes.polymorph.as_mut().and_then(|d| d.play.as_mut()).map(|c| &mut c.m);
    }
    None
}

/// The state of instance `uid`, wherever the fighter holds it.
pub fn state_mut(w: &mut World, f: FighterId, uid: u32) -> Option<&mut MontageState> {
    find_mut(w.fighters.get_mut(f)?, uid)
}

pub fn state_ref(w: &World, f: FighterId, uid: u32) -> Option<&MontageState> {
    let fi = w.fighters.get(f)?;
    let ms = &fi.montages;
    fn one(s: &Option<MontageState>, uid: u32) -> Option<&MontageState> {
        s.as_ref().filter(|s| s.montage.uid == uid)
    }
    one(&ms.montage, uid)
        .or_else(|| one(&ms.dash_montage, uid))
        .or_else(|| ms.fading.iter().map(|x| &x.m).find(|s| s.montage.uid == uid))
        .or_else(|| ms.grouped.as_ref().and_then(|g| g.0.iter().map(|x| &x.m).find(|s| s.montage.uid == uid)))
        .or_else(|| ms.limbo.iter().find(|s| s.montage.uid == uid))
        .or_else(|| {
            fi.shapes.disguise.as_ref().and_then(|d| d.copy.as_ref()).map(|c| &c.m).filter(|s| s.montage.uid == uid)
        })
        .or_else(|| {
            fi.shapes.polymorph.as_ref().and_then(|d| d.play.as_ref()).map(|c| &c.m).filter(|s| s.montage.uid == uid)
        })
}

/// The montage a player of the fighter plays (`MontagePlayer.name`); `Name::NONE` for a slot that holds none.
pub fn slot_name(w: &World, f: FighterId, slot: MontageSlot) -> Name {
    slot_uid(w, f, slot).and_then(|u| state_ref(w, f, u)).map_or(Name::NONE, |s| s.montage.name)
}

/// The instance a [`MontageSlot`] names, by its number.
pub fn slot_uid(w: &World, f: FighterId, slot: MontageSlot) -> Option<u32> {
    let fi = w.fighter(f);
    let ms = &fi.montages;
    let s = match slot {
        MontageSlot::Main => ms.montage.as_ref(),
        MontageSlot::Dash => ms.dash_montage.as_ref(),
        MontageSlot::Fading(i) => ms.fading.get(i as usize).map(|x| &x.m),
        MontageSlot::Grouped(i) => ms.grouped.as_ref().and_then(|g| g.0.get(i as usize)).map(|x| &x.m),
        MontageSlot::Foreign => fi.shapes.disguise.as_ref().and_then(|d| d.copy.as_ref()).map(|c| &c.m),
        MontageSlot::Poly => fi.shapes.polymorph.as_ref().and_then(|d| d.play.as_ref()).map(|c| &c.m),
    };
    s.map(|s| s.montage.uid)
}

/// The place instance `uid` is in, as a [`MontageSlot`] (limbo has none).
pub fn uid_slot(w: &World, f: FighterId, uid: u32) -> Option<MontageSlot> {
    let fi = w.fighter(f);
    let ms = &fi.montages;
    let is = |s: Option<&MontageState>| s.is_some_and(|s| s.montage.uid == uid);
    if is(ms.montage.as_ref()) {
        return Some(MontageSlot::Main);
    }
    if is(ms.dash_montage.as_ref()) {
        return Some(MontageSlot::Dash);
    }
    if let Some(k) = ms.fading.iter().position(|x| x.m.montage.uid == uid) {
        return Some(MontageSlot::Fading(u32x(k)));
    }
    if let Some(k) = ms.grouped.as_ref().and_then(|g| g.0.iter().position(|x| x.m.montage.uid == uid)) {
        return Some(MontageSlot::Grouped(u32x(k)));
    }
    if is(fi.shapes.disguise.as_ref().and_then(|d| d.copy.as_ref()).map(|c| &c.m)) {
        return Some(MontageSlot::Foreign);
    }
    if is(fi.shapes.polymorph.as_ref().and_then(|d| d.play.as_ref()).map(|c| &c.m)) {
        return Some(MontageSlot::Poly);
    }
    None
}

pub(crate) fn main_uid(w: &World, f: FighterId) -> Option<u32> {
    w.fighter(f).montages.montage.as_ref().map(|m| m.montage.uid)
}

/// A state dropped from its place: kept in limbo while a player still runs it.
pub(crate) fn drop_state(w: &mut World, f: FighterId, s: MontageState) {
    let ms = &mut w.fighter_mut(f).montages;
    if ms.running.contains(&s.montage.uid) {
        ms.limbo.push(s);
    }
}

/// Runs `body` with instance `uid` marked as run by a player (its state kept if a handler drops it).
pub(crate) fn running<R>(w: &mut World, f: FighterId, uid: u32, body: impl FnOnce(&mut World) -> R) -> R {
    w.fighter_mut(f).montages.running.push(uid);
    let r = body(w);
    let ms = &mut w.fighter_mut(f).montages;
    if let Some(k) = ms.running.iter().rposition(|&u| u == uid) {
        ms.running.remove(k);
    }
    let run = std::mem::take(&mut ms.running);
    ms.limbo.retain(|s| run.contains(&s.montage.uid));
    ms.running = run;
    r
}

// ---- the montages' data

/// The character data of key `ch`.
pub fn char_data<'d>(
    d: &'d mvs_data::GameData,
    names: &crate::names::Names,
    ch: Name,
) -> Option<&'d mvs_data::CharData> {
    d.chars.get(names.str(ch)).map(|c| &**c)
}

/// A montage of the data `key` names: a character's, else (the items owner of a polymorph) the items part's.
pub fn owner_montage<'d>(
    d: &'d mvs_data::GameData,
    names: &crate::names::Names,
    key: Name,
    montage: &str,
) -> Option<&'d Montage> {
    match char_data(d, names, key) {
        Some(c) => c.fighter.montages.get(montage),
        None => d.items.as_ref()?.montages.get(montage),
    }
}

/// The fighter's data (`f.data`).
pub fn fdata(w: &World, f: FighterId) -> &mvs_data::fighter::FighterData {
    &char_data(&w.data, &w.names, w.fighter(f).ch).expect("the fighter's character data").fighter
}

/// `f.data.montages[name]`.
pub fn montage_data(w: &World, f: FighterId, name: Name) -> Option<&Montage> {
    fdata(w, f).montages.get(w.names.str(name))
}

/// The data of the montage instance `uid` plays: the fighter's own, a disguise copy's from its victim, a
/// polymorph's from the data it names.
pub fn instance_data<'d>(d: &'d mvs_data::GameData, w: &World, f: FighterId, uid: u32) -> Option<&'d Montage> {
    let fi = w.fighter(f);
    let name = state_ref(w, f, uid)?.montage.name;
    let n = w.names.str(name);
    let ch = match uid_slot(w, f, uid) {
        Some(MontageSlot::Foreign) => w.fighters.get(fi.shapes.disguise.as_ref()?.from)?.ch,
        Some(MontageSlot::Poly) => fi.shapes.polymorph.as_ref()?.from?,
        _ => fi.ch,
    };
    owner_montage(d, &w.names, ch, n)
}

/// The notify a [`NotifyRef`] names in the montages the fighter can play: its own, then its disguise victim's,
/// then its polymorph's (the notify object itself is held; a window of another anim instance is filed with
/// the victim's).
pub fn any_notify(w: &World, f: FighterId, n: NotifyRef) -> Option<&Notify> {
    any_notify_in(&w.data, w, f, n)
}

/// [`any_notify`] in `data` (a clone of the World's), so a caller can hold the notify across its writes. Last, the
/// montages of every fighter of the match: an end held for the next dispatch (`pending_ends`) outlives the second
/// mesh that played it (a Disguise copy dropped with its buff), and the `Notify` itself is held there. This
/// rests on montage names being unique across the characters (none is shared among the ported ones): a name two
/// characters share would resolve to the first fighter's.
pub fn any_notify_in<'a>(data: &'a mvs_data::GameData, w: &World, f: FighterId, n: NotifyRef) -> Option<&'a Notify> {
    let of = |ch: Name| -> Option<&'a Notify> {
        owner_montage(data, &w.names, ch, w.names.str(n.montage))?.notifies.get(n.index as usize)
    };
    let fi = w.fighter(f);
    of(fi.ch)
        .or_else(|| of(w.fighters.get(fi.shapes.disguise.as_ref()?.from)?.ch))
        .or_else(|| of(fi.shapes.polymorph.as_ref()?.from?))
        .or_else(|| seq_notify(data, w, f, n))
        .or_else(|| w.fighters.iter().find_map(|(_, o)| of(o.ch)))
}

/// The fighter data of the character whose montage holds `n`, in [`any_notify_in`]'s order (the fighter's own, its
/// disguise victim's, its polymorph's, then every fighter's of the match); the fighter's own for a sequence notify,
/// a polymorph's items montage and a montage nobody's data holds.
pub fn notify_owner_data<'a>(
    data: &'a mvs_data::GameData,
    w: &World,
    f: FighterId,
    n: NotifyRef,
) -> &'a mvs_data::fighter::FighterData {
    let m = w.names.str(n.montage);
    let has = |ch: Name| char_data(data, &w.names, ch).filter(|c| c.fighter.montages.contains_key(m));
    let fi = w.fighter(f);
    let own = || char_data(data, &w.names, fi.ch).expect("the fighter's character data");
    let cd = has(fi.ch)
        .or_else(|| has(w.fighters.get(fi.shapes.disguise.as_ref()?.from)?.ch))
        .or_else(|| has(fi.shapes.polymorph.as_ref()?.from?))
        .or_else(|| seq_notify(data, w, f, n).map(|_| own()))
        .or_else(|| w.fighters.iter().find_map(|(_, o)| has(o.ch)))
        .unwrap_or_else(own);
    &cd.fighter
}

/// The notify a [`NotifyRef`] names, in the fighter's own montages, else its sequence notifies.
pub fn notify_of(w: &World, f: FighterId, n: NotifyRef) -> Option<&Notify> {
    notify_in(&w.data, w, f, n)
}

/// [`notify_of`] in `data` (a clone of the World's).
fn notify_in<'a>(data: &'a mvs_data::GameData, w: &World, f: FighterId, n: NotifyRef) -> Option<&'a Notify> {
    let cd = char_data(data, &w.names, w.fighter(f).ch)?;
    match cd.fighter.montages.get(w.names.str(n.montage)) {
        Some(m) => m.notifies.get(n.index as usize),
        None => seq_notify(data, w, f, n).or_else(|| any_notify_in(data, w, f, n)),
    }
}

/// The notify object a handler is called with (the `Notify` itself is passed): the one [`any_notify`] finds. It
/// borrows `data` (a clone of `w.data`'s `Arc`), not the World, so a hook can hold it across its writes.
pub fn notify_data<'a>(w: &World, data: &'a mvs_data::GameData, f: FighterId, n: NotifyRef) -> &'a Notify {
    any_notify_in(data, w, f, n).expect("a notify of the montages the fighter plays")
}

/// A sequence notify (`CharDef.seqNotifies[anim][i]`, seqStatesTick): a [`NotifyRef`] of the
/// sequence's name and the notify's index in its list.
fn seq_notify<'a>(data: &'a mvs_data::GameData, w: &World, f: FighterId, n: NotifyRef) -> Option<&'a Notify> {
    let cd = char_data(data, &w.names, w.fighter(f).ch)?;
    cd.def.seq_notifies.get(w.names.str(n.montage))?.get(n.index as usize)
}

// ---- the sinks

/// One instance as the player sees it: its montage, its state found by number on each access, and its sink.
pub(crate) struct FHost<'w, 'm> {
    pub w: &'w mut World,
    pub f: FighterId,
    pub key: MontageKey,
    pub m: &'m Montage,
    pub sink: SinkKind,
    /// The key of the instance's notify 0 (instance number x KEY_STRIDE).
    pub base: i32,
    spare: MontageState,
}

impl<'w, 'm> FHost<'w, 'm> {
    pub fn new(w: &'w mut World, f: FighterId, key: MontageKey, m: &'m Montage, sink: SinkKind, base: i32) -> Self {
        FHost { w, f, key, m, sink, base, spare: MontageState::new(key, false) }
    }
}

impl<'m> Host<'m, MontageKey> for FHost<'_, 'm> {
    fn m(&self) -> &'m Montage {
        self.m
    }
    fn s(&mut self) -> &mut MontageState {
        if let Some(s) = state_mut(self.w, self.f, self.key.uid) {
            return s;
        }
        &mut self.spare
    }
    fn begin(&mut self, idx: usize) {
        sink_call(self.w, self.f, self.sink, self.m, self.key.name, self.base, idx, Call::Begin);
    }
    fn end(&mut self, idx: usize) {
        sink_call(self.w, self.f, self.sink, self.m, self.key.name, self.base, idx, Call::End);
    }
    fn instant(&mut self, idx: usize) {
        sink_call(self.w, self.f, self.sink, self.m, self.key.name, self.base, idx, Call::Instant);
    }
    fn has_root_step(&self) -> bool {
        self.sink == SinkKind::Fighter
    }
    fn root_step(&mut self, a: f64, b: f64) {
        super::root_motion::root_step(
            self.w,
            self.f,
            self.key.uid,
            self.m,
            crate::math::narrow(a),
            crate::math::narrow(b),
        );
    }
    fn has_pass_end(&self) -> bool {
        self.sink == SinkKind::Fighter
    }
    fn pass_end(&mut self, idx: usize) {
        let n = NotifyRef { montage: self.key.name, index: u32x(idx) };
        pass_end(self.w, self.f, n, self.base + ix(idx));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Call {
    Begin,
    End,
    Instant,
}

#[allow(clippy::too_many_arguments, reason = "the sink's call and the instance's parts")]
fn sink_call(w: &mut World, f: FighterId, sink: SinkKind, m: &Montage, name: Name, base: i32, idx: usize, c: Call) {
    let n = &m.notifies[idx];
    let r = NotifyRef { montage: name, index: u32x(idx) };
    let key = base + ix(idx);
    match sink {
        SinkKind::Plain => {}
        SinkKind::Dash if c != Call::Instant && n.cls == PLAIN_LEFT_OUT => {}
        SinkKind::Dash | SinkKind::Fighter => match c {
            Call::Begin => super::notify_sink::begin_of(w, f, n, r, key),
            Call::End => super::notify_sink::end_of(w, f, n, r, key),
            Call::Instant => super::notify_sink::instant_of(w, f, n, r),
        },
        SinkKind::Grouped => super::grouped::grouped_sink(w, f, n, r, key, c == Call::Begin, c == Call::Instant),
    }
}

/// Runs `body` over instance `uid`'s player with `sink` (`m.advance(.., sink)` and the other calls).
pub(crate) fn with_player<R>(
    w: &mut World,
    f: FighterId,
    uid: u32,
    sink: SinkKind,
    base: i32,
    body: impl FnOnce(&mut FHost<'_, '_>) -> R,
) -> Option<R> {
    let data = w.data.clone();
    let m = instance_data(&data, w, f, uid)?;
    let key = state_ref(w, f, uid)?.montage;
    Some(running(w, f, uid, |w| body(&mut FHost::new(w, f, key, m, sink, base))))
}

// ---- the montage functions

/// `newFade`.
fn new_fade(b: (Real, Real), m: MontageState) -> Fade {
    Fade { left: b.0, time: b.1, m, hits: None, tags: None, ends: None }
}

/// `newActiveHit`.
pub fn new_active_hit(
    n: NotifyRef,
    idx: i32,
    hit_set: &IndexSet<i32>,
    charged_time: Real,
    attack_id: i32,
    attack_name: Name,
    open_pose: Option<NewActiveHitOpenPose>,
) -> ActiveHit {
    ActiveHit {
        n,
        idx,
        hit_set: hit_set.clone(),
        charged_time,
        attack_id,
        attack_name,
        open_pose: open_pose.map(|p| (p.anim, p.time)),
        copy_pose: None,
        p: None,
    }
}

/// The live states of one instance whose class is `cls`, as (key, notify index) pairs, in the state's order.
fn states_of<'a>(m: &'a Montage, s: &'a MontageState, cls: &'a str) -> impl Iterator<Item = usize> + 'a {
    s.active.iter().copied().filter(move |&i| m.notifies[i].cls == cls)
}

/// The foreign play of instance number `inst` (`foreignPlay`).
pub fn foreign_play(w: &World, f: FighterId, inst: i32) -> Option<&crate::combat::shapes::ForeignPlay> {
    let fi = w.fighter(f);
    match inst {
        COPY_INST => fi.shapes.disguise.as_ref()?.copy.as_ref(),
        POLY_INST => fi.shapes.polymorph.as_ref()?.play.as_ref(),
        _ => None,
    }
}

/// `anyActive`.
pub fn any_active(w: &mut World, f: FighterId, cls: Name, test: &dyn Fn(&Notify) -> bool) -> bool {
    let data = w.data.clone();
    let c = w.names.str(cls);
    let fi = w.fighter(f);
    if let Some(m) = &fi.montages.montage
        && let Some(md) = instance_data(&data, w, f, m.montage.uid)
        && states_of(md, m, c).any(|i| test(&md.notifies[i]))
    {
        return true;
    }
    if let Some((l, _)) = &fi.montages.grouped {
        for e in l {
            if e.inst.unwrap_or(0) == 0 || e.fade.is_some() {
                continue;
            }
            if let Some(md) = instance_data(&data, w, f, e.m.montage.uid)
                && states_of(md, &e.m, c).any(|i| test(&md.notifies[i]))
            {
                return true;
            }
        }
    }
    if c != "AnimNotify_BranchWindow_C" {
        for fi in FOREIGN_INSTS {
            if let Some(cp) = foreign_play(w, f, fi)
                && !cp.stopped
                && let Some(md) = instance_data(&data, w, f, cp.m.montage.uid)
                && states_of(md, &cp.m, c).any(|i| test(&md.notifies[i]))
            {
                return true;
            }
        }
    }
    false
}

/// `activeOf`: `only` None is `undefined`.
pub fn active_of(w: &mut World, f: FighterId, cls: Name, only: Option<Real>) -> Vec<ActiveOfActiveOf> {
    let mut out = Vec::new();
    active_of_into(w, f, cls, only, &mut out);
    out
}

/// [`active_of`] into `out` (cleared first).
pub fn active_of_into(w: &World, f: FighterId, cls: Name, only: Option<Real>, out: &mut Vec<ActiveOfActiveOf>) {
    out.clear();
    let data = &w.data;
    let c = w.names.str(cls);
    let fi = w.fighter(f);
    let only_i = only.map(int);
    if let Some(m) = &fi.montages.montage
        && only.is_none_or(|o| o == 0.0)
        && let Some(md) = instance_data(data, w, f, m.montage.uid)
    {
        for i in states_of(md, m, c) {
            out.push(ActiveOfActiveOf { n: NotifyRef { montage: m.montage.name, index: u32x(i) }, i: ix(i) });
        }
    }
    if only != Some(0.0)
        && let Some((l, _)) = &fi.montages.grouped
    {
        for e in l {
            let inst = e.inst.unwrap_or(0);
            if inst == 0 || e.fade.is_some() || only_i.is_some_and(|o| inst != o) {
                continue;
            }
            if let Some(md) = instance_data(data, w, f, e.m.montage.uid) {
                for i in states_of(md, &e.m, c) {
                    let n = NotifyRef { montage: e.m.montage.name, index: u32x(i) };
                    out.push(ActiveOfActiveOf { n, i: inst * KEY_STRIDE + ix(i) });
                }
            }
        }
    }
    if c != "AnimNotify_BranchWindow_C" {
        for fi in FOREIGN_INSTS {
            if let Some(cp) = foreign_play(w, f, fi)
                && !cp.stopped
                && only_i.is_none_or(|o| o == fi)
                && let Some(md) = instance_data(data, w, f, cp.m.montage.uid)
            {
                for i in states_of(md, &cp.m, c) {
                    let n = NotifyRef { montage: cp.m.montage.name, index: u32x(i) };
                    out.push(ActiveOfActiveOf { n, i: fi * KEY_STRIDE + ix(i) });
                }
            }
        }
    }
    out.sort_by_key(|a| a.i);
}

/// `hasActive`: `skip` -1 for none.
pub fn has_active(w: &World, f: FighterId, cls: Name, skip: Real) -> bool {
    let data = w.data.clone();
    let c = w.names.str(cls);
    let fi = w.fighter(f);
    if let Some(m) = &fi.montages.montage
        && let Some(md) = instance_data(&data, w, f, m.montage.uid)
        && states_of(md, m, c).next().is_some()
    {
        return true;
    }
    if let Some((l, _)) = &fi.montages.grouped {
        for e in l {
            if e.inst.unwrap_or(0) == 0 || e.fade.is_some() {
                continue;
            }
            if let Some(md) = instance_data(&data, w, f, e.m.montage.uid)
                && states_of(md, &e.m, c).next().is_some()
            {
                return true;
            }
        }
    }
    if c != "AnimNotify_BranchWindow_C" {
        for fi in FOREIGN_INSTS {
            if crate::real(fi) == skip {
                continue;
            }
            if let Some(cp) = foreign_play(w, f, fi)
                && !cp.stopped
                && let Some(md) = instance_data(&data, w, f, cp.m.montage.uid)
                && states_of(md, &cp.m, c).next().is_some()
            {
                return true;
            }
        }
    }
    false
}

/// `ownerOf`.
pub fn owner_of(w: &World, f: FighterId, key: Real) -> Option<MontageSlot> {
    if key < 0.0 {
        return None;
    }
    let inst = (key / crate::real(KEY_STRIDE)).floor();
    if inst == 0.0 {
        return w.fighter(f).montages.montage.as_ref().map(|_| MontageSlot::Main);
    }
    let inst = int(inst);
    if let Some(cp) = foreign_play(w, f, inst) {
        return if !cp.stopped {
            Some(if inst == COPY_INST { MontageSlot::Foreign } else { MontageSlot::Poly })
        } else {
            None
        };
    }
    if let Some((l, _)) = &w.fighter(f).montages.grouped {
        for (k, e) in l.iter().enumerate() {
            if e.inst == Some(inst) {
                return if e.fade.is_some() { None } else { Some(MontageSlot::Grouped(u32x(k))) };
            }
        }
    }
    None
}

/// `resolve`.
pub fn resolve(w: &World, f: FighterId, key: Real) -> Option<ResolveOut> {
    let m = owner_of(w, f, key)?;
    let uid = slot_uid(w, f, m)?;
    let i = key.floor() % crate::real(KEY_STRIDE);
    let data = w.data.clone();
    let md = instance_data(&data, w, f, uid)?;
    let i = int(i);
    md.notifies.get(ux(i))?;
    let name = state_ref(w, f, uid)?.montage.name;
    Some(ResolveOut { m, n: NotifyRef { montage: name, index: u32x(ux(i)) }, i })
}

/// `stoppedOwner`.
pub fn stopped_owner(w: &World, f: FighterId, key: Real) -> Option<MontageSlot> {
    let inst = int((key / crate::real(KEY_STRIDE)).floor());
    if let Some(cp) = foreign_play(w, f, inst) {
        return if cp.stopped {
            Some(if inst == COPY_INST { MontageSlot::Foreign } else { MontageSlot::Poly })
        } else {
            None
        };
    }
    if inst > 0
        && let Some((l, _)) = &w.fighter(f).montages.grouped
    {
        for (k, e) in l.iter().enumerate() {
            if e.inst == Some(inst) {
                return if e.fade.is_some() { Some(MontageSlot::Grouped(u32x(k))) } else { None };
            }
        }
    }
    None
}

/// `montageIsActive`.
pub fn montage_is_active(w: &World, f: FighterId, name: Name) -> bool {
    let ms = &w.fighter(f).montages;
    if let Some(m) = &ms.montage
        && m.montage.name == name
    {
        return !m.blending_out();
    }
    if let Some(d) = &ms.dash_montage
        && d.montage.name == name
    {
        return !d.blending_out();
    }
    if let Some((l, _)) = &ms.grouped {
        for e in l.iter().rev() {
            if e.m.montage.name == name {
                return !(e.fade.is_some() || e.m.terminated || e.m.ended || e.m.blending_out());
            }
        }
    }
    false
}

/// `activeKeys`.
pub fn active_keys(w: &World, f: FighterId) -> Vec<i32> {
    let fi = w.fighter(f);
    let mut out: Vec<i32> =
        fi.montages.montage.as_ref().map(|m| m.active.iter().map(|&i| ix(i)).collect()).unwrap_or_default();
    if let Some((l, _)) = &fi.montages.grouped {
        for e in l {
            let inst = e.inst.unwrap_or(0);
            if inst != 0 && e.fade.is_none() {
                out.extend(e.m.active.iter().map(|&i| inst * KEY_STRIDE + ix(i)));
            }
        }
    }
    let data = w.data.clone();
    for fi in FOREIGN_INSTS {
        if let Some(cp) = foreign_play(w, f, fi)
            && !cp.stopped
            && let Some(md) = instance_data(&data, w, f, cp.m.montage.uid)
        {
            for &i in &cp.m.active {
                if md.notifies[i].cls != "AnimNotify_BranchWindow_C" {
                    out.push(fi * KEY_STRIDE + ix(i));
                }
            }
        }
    }
    out
}

/// The fighter's state of the frame: grounded.
fn grounded(w: &World, f: FighterId) -> bool {
    w.fighter(f).body.on_floor
}

/// `playPlainMontage`: `sink` the montage's notify receiver (`dashSink` the default).
pub fn play_plain_montage(
    w: &mut World,
    own: FighterId,
    name: Option<Name>,
    rate: Real,
    sink: SinkKind,
    section: Option<Name>,
) {
    let data = w.data.clone();
    let Some(name) = name else { return };
    let Some(m) = char_data(&data, &w.names, w.fighter(own).ch).and_then(|c| c.fighter.montages.get(w.names.str(name)))
    else {
        return;
    };
    w.fighter_mut(own).anim.plain_sink = sink;
    stop_dash(w, own, m.blend_in, true);
    super::grouped::grouped_under(w, own);
    for fd in &mut w.fighter_mut(own).montages.fading {
        if !(crate::math::narrow(m.blend_in) < fd.time) {
            continue;
        }
        fd.time = crate::math::narrow(m.blend_in);
        fd.left = crate::math::narrow(blend_ticks(m.blend_in));
    }
    super::root_motion::not_root_instance(w, own, m);
    let g = grounded(w, own);
    let mut p = new_instance(w, own, name, g);
    p.play_rate = f64::from(rate);
    if let Some(s) = section {
        p.jump_to_section(m, w.names.str(s));
    }
    let fi = w.fighter_mut(own);
    fi.montages.dash_montage = Some(p);
    fi.anim.plain_is_intro = false;
    let dash = is_dash_or_turn(w, own, name);
    super::anim_montage_start(w, own, dash);
    if let Some(h) = w.fighter(own).hooks.char.montage_started {
        h(w, own, name);
    }
}

/// `isDashOrTurn`.
fn is_dash_or_turn(w: &World, f: FighterId, name: Name) -> bool {
    let nav = super::nav_of(w, f);
    let n = w.names.str(name);
    nav.dash.as_deref() == Some(n) || nav.dash_turn.as_deref() == Some(n)
}

/// `isDashAnim`.
pub fn is_dash_anim(w: &World, f: FighterId, name: Name) -> bool {
    let n = w.names.str(name);
    let nav = super::nav_of(w, f);
    nav.dash.as_deref() == Some(n)
        || nav.dash_turn.as_deref() == Some(n)
        || text(&fdata(w, f).pawn, "QuickDashTurnAnim") == Some(n)
}

/// `playIntroMontage`.
pub fn play_intro_montage(w: &mut World, f: FighterId) {
    let d = fdata(w, f);
    let intro =
        d.skin.intro_montage.as_deref().or_else(|| text(&d.pawn, "IntroAnimation")).and_then(|s| w.names.get(s));
    play_plain_montage(w, f, intro, 1.0, SinkKind::Fighter, None);
    let fi = w.fighter_mut(f);
    fi.anim.plain_is_intro = fi.montages.dash_montage.is_some();
}

/// `stopDashMontage`: Montage_Stop(blend) of the montage without attack state. An instance
/// in its own automatic blend-out keeps that blend's time (`MontageState::montage_stop_blend`).
pub fn stop_dash_montage(w: &mut World, f: FighterId, blend: f64) {
    stop_dash(w, f, blend, false);
}

/// `stopDashMontage` for Montage_PlayInternal's group stop (`group`), which shortens a running blend-out to the new
/// montage's BlendIn (spec 07 item 21), or a Montage_Stop.
// Unverified: the stopped instance is assumed to leave the slot for `fading` and its notify states to end at the stop;
// Montage_Stop does not reach an instance in its automatic blend-out at all (spec 07 item 21),
// whose notifies keep firing to its end (exe FUN_14284b1d0).
fn stop_dash(w: &mut World, f: FighterId, blend: f64, group: bool) {
    let Some(m) = w.fighter_mut(f).montages.dash_montage.take() else { return };
    let key = m.montage;
    let terminated = m.terminated;
    if !terminated {
        let b = if group { m.stop_blend(blend) } else { m.montage_stop_blend(blend) };
        w.fighter_mut(f).montages.fading.push(new_fade(
            {
                let (a, b) = b;
                (crate::math::narrow(a), crate::math::narrow(b))
            },
            m,
        ));
    } else {
        w.fighter_mut(f).montages.limbo.push(m);
    }
    running(w, f, key.uid, |w| {
        super::montage_blending_out(w, f, key.uid);
        let sink = w.fighter(f).anim.plain_sink;
        with_player(w, f, key.uid, sink, 0, |h| h.stop());
    });
    if terminated {
        montage_ended_buffs(w, f, key.name, false);
    }
}

/// `montageEndedBuffs`.
fn montage_ended_buffs(w: &mut World, f: FighterId, montage: Name, interrupted: bool) {
    if !w.fighter(f).buffs.buffs.is_empty() {
        let arg = BuffSignalArg { montage: Some(montage), interrupted: Some(interrupted), ..Default::default() };
        crate::combat::buffs::buff_signal(w, f, BuffSignal::MontageEnded, arg);
    }
}

/// `startMontage`. `owed`: the proxies of the instances the play stops, for a caller that
/// runs their handlers itself (`proxy_clear`).
pub fn start_montage(
    w: &mut World,
    own: FighterId,
    name: Name,
    proxy: &MontageProxy,
    owed: Option<&mut Vec<MontageProxy>>,
) {
    let data = w.data.clone();
    let Some(m) = char_data(&data, &w.names, w.fighter(own).ch).and_then(|c| c.fighter.montages.get(w.names.str(name)))
    else {
        warn_text(w, &format!("missing montage {}", w.names.str(name)));
        return;
    };
    let adv = w.fighter(own).montages.advancing.filter(|&u| state_ref(w, own, u).is_some_and(|s| s.in_advance));
    let mut after_local: Vec<MontageProxy> = Vec::new();
    let has_owed = owed.is_some();
    let after: Option<&mut Vec<MontageProxy>> = match owed {
        Some(o) => Some(o),
        None if *proxy == PROXY_ATTACK => Some(&mut after_local),
        None => None,
    };
    let has_after = after.is_some();
    stop_main(w, own, Some(m.blend_in), after, true);
    stop_dash(w, own, m.blend_in, true);
    super::grouped::grouped_under(w, own);
    for fd in &mut w.fighter_mut(own).montages.fading {
        if !(crate::math::narrow(m.blend_in) < fd.time) {
            continue;
        }
        fd.time = crate::math::narrow(m.blend_in);
        fd.left = crate::math::narrow(blend_ticks(m.blend_in));
    }
    let k =
        adv.and_then(|a| w.fighter(own).montages.fading.iter().position(|fd| fd.m.montage.uid == a && fd.left <= 0.0));
    if let Some(k) = k {
        end_fade(w, own, k);
        let fd = w.fighter_mut(own).montages.fading.remove(k);
        drop_state(w, own, fd.m);
        w.fighter_mut(own).montages.montage_abort = true;
    }
    let g = grounded(w, own);
    let mut p = new_instance(w, own, name, g);
    p.proxy = *proxy;
    let fi = w.fighter_mut(own);
    fi.montages.played_in_advance = adv.is_some();
    fi.decay.attack_count += 1;
    fi.montages.montage = Some(p);
    if let Some(wc) = &fi.branching.win_cleared {
        let rest: Vec<i32> = wc.iter().copied().filter(|&k| k >= KEY_STRIDE).collect();
        fi.branching.win_cleared = if rest.is_empty() { None } else { Some(rest) };
    }
    let dash = is_dash_or_turn(w, own, name);
    super::anim_montage_start(w, own, dash);
    if let Some(h) = w.fighter(own).hooks.char.montage_started {
        h(w, own, name);
    }
    let fi = w.fighter_mut(own);
    fi.montages.montage_hit = false;
    let c = &mut fi.charge;
    c.begin_attack = true;
    c.anim_attack_off = 0;
    c.charge_mult = 1.0;
    c.charge_dmg_mult = 1.0;
    c.charge_buff_mult = 1.0;
    c.charge_pct = 0.0;
    c.charge_notify = -1;
    c.charge_released = false;
    if has_after && !has_owed {
        for o in after_local {
            proxy_clear(w, own, o);
        }
    }
}

/// `proxyClear`: the handlers of a play's callback proxy, by the proxy.
pub fn proxy_clear(w: &mut World, f: FighterId, k: MontageProxy) {
    if k == 0 {
        return;
    }
    let fi = w.fighter_mut(f);
    fi.charge.begin_attack = false;
    if k == 2 {
        fi.charge.anim_attack_off = 0;
    } else if k == 3 || k == 4 {
        fi.evade.evading = false;
        if k == 4 {
            fi.platforms.evading_through_platform = false;
        }
    }
}

/// `oweProxyClear`.
fn owe_proxy_clear(w: &mut World, f: FighterId, uid: u32, owed: Option<&mut Vec<MontageProxy>>) {
    let Some(p) = state_ref(w, f, uid).map(|s| s.proxy) else { return };
    if p == 0 {
        return;
    }
    let adv_in = w.fighter(f).montages.advancing.is_some_and(|u| state_ref(w, f, u).is_some_and(|s| s.in_advance));
    let fi = w.fighter_mut(f);
    if let Some(q) = fi.montages.proxy_owed.as_mut().filter(|_| !adv_in) {
        q.push(p);
    } else if let Some(o) = owed {
        o.push(p);
    } else {
        proxy_clear(w, f, p);
    }
}

/// `openProxyEvents`.
pub fn open_proxy_events(w: &mut World, f: FighterId) {
    let ms = &mut w.fighter_mut(f).montages;
    if ms.proxy_owed.is_none() {
        ms.proxy_owed = Some(Vec::new());
    }
}

/// `flushProxyEvents`.
pub fn flush_proxy_events(w: &mut World, f: FighterId) {
    if let Some(q) = w.fighter_mut(f).montages.proxy_owed.take() {
        for p in q {
            proxy_clear(w, f, p);
        }
    }
}

/// `naturalBlendOut`.
fn natural_blend_out(w: &mut World, f: FighterId, uid: u32) {
    if state_ref(w, f, uid).is_none_or(|s| s.blend_out_fired) {
        return;
    }
    super::montage_blending_out(w, f, uid);
    owe_proxy_clear(w, f, uid, None);
}

/// `stopMontage`: Montage_Stop of `montage`, `blend` None for the montage's BlendOut.
/// `owed`: see `start_montage`. An instance in its own automatic blend-out keeps that blend's time
/// (`MontageState::montage_stop_blend`).
pub fn stop_montage(w: &mut World, f: FighterId, blend: Option<f64>, owed: Option<&mut Vec<MontageProxy>>) {
    stop_main(w, f, blend, owed, false);
}

/// `stopMontage` for Montage_PlayInternal's group stop (`group`: a running blend-out is shortened to the new
/// montage's BlendIn, spec 07 item 21), or a Montage_Stop.
// Unverified: as `stop_dash`, the instance leaves the slot and its states end at a Montage_Stop that the exe does not
// make on an instance already stopped.
fn stop_main(w: &mut World, f: FighterId, blend: Option<f64>, owed: Option<&mut Vec<MontageProxy>>, group: bool) {
    let Some(key) = w.fighter(f).montages.montage.as_ref().map(|m| m.montage) else { return };
    let uid = key.uid;
    let data = w.data.clone();
    let Some(md) = instance_data(&data, w, f, uid) else { return };
    let (natural, terminated) = {
        let m = w.fighter_mut(f).montages.montage.as_mut().expect("the montage");
        let natural = m.blend_out_fired;
        if !m.terminated && !natural {
            m.interrupt_fired = true;
        }
        (natural, m.terminated)
    };
    let q_windows: Option<Vec<i32>> = w.fighter(f).branching.queued.as_ref().map(|q| q.windows.clone());
    let wins = super::branches::open_windows(w, f, Some(0.0));
    let mut lws: Vec<(NotifyRef, i32)> = Vec::new();
    let mut laws: Vec<NotifyRef> = Vec::new();
    for x in &wins {
        let Some(n) = md.notifies.get(x.n.index as usize) else { continue };
        if n.tick == NotifyTick::BranchingPoint {
            continue;
        }
        if q_windows.as_ref().is_some_and(|q| q.contains(&x.i)) || super::branches::answers_no_press(w, f, x.n) {
            lws.push((x.n, x.i));
        }
    }
    for x in &wins {
        let Some(n) = md.notifies.get(x.n.index as usize) else { continue };
        if truthy(n.p.get("MakeBranchesAutomatic")) && n.tick != NotifyTick::BranchingPoint {
            laws.push(x.n);
        }
    }
    let m = w.fighter_mut(f).montages.montage.take().expect("the montage");
    let mut fade = false;
    if !terminated {
        let t = blend.unwrap_or(md.blend_out);
        let b = if group { m.stop_blend(t) } else { m.montage_stop_blend(t) };
        if m.in_advance && b.0 <= 0.0 {
            w.fighter_mut(f).montages.montage_abort = true;
            w.fighter_mut(f).montages.limbo.push(m);
        } else {
            w.fighter_mut(f).montages.fading.push(new_fade(
                {
                    let (a, b) = b;
                    (crate::math::narrow(a), crate::math::narrow(b))
                },
                m,
            ));
            fade = true;
        }
    } else {
        w.fighter_mut(f).montages.limbo.push(m);
    }
    running(w, f, uid, |w| {
        let ended_now = if terminated {
            Some(false)
        } else if fade {
            None
        } else {
            Some(true)
        };
        let interrupt_fired = state_ref(w, f, uid).is_some_and(|s| s.interrupt_fired);
        if interrupt_fired || (natural && !terminated && !fade) {
            owe_proxy_clear(w, f, uid, owed);
        }
        // The hit objects are kept: one a handler of the stop drops from `activeHits` (hitframeEnd) is still
        // the fade's. The clones are refreshed from the list after the stop where the hit is still on it.
        let mut keep: Vec<ActiveHit> = Vec::new();
        if fade {
            let fi = w.fighter(f);
            keep.extend(
                fi.montages
                    .active_hits
                    .values()
                    .filter(|h| {
                        h.idx >= 0
                            && h.idx < KEY_STRIDE
                            && notify_of(w, f, h.n).is_some_and(|n| n.tick == NotifyTick::BranchingPoint)
                    })
                    .cloned(),
            );
        }
        // (a stop inside another stop's end handlers saves the outer one's slot and gives it back)
        let outer_keep = std::mem::replace(&mut w.fighter_mut(f).montages.stop_keep, keep.clone());
        let in_adv = state_ref(w, f, uid).is_some_and(|s| s.in_advance);
        let fi = w.fighter_mut(f);
        fi.branching.defer_queued_ends = fi.root.anim_updated && !in_adv;
        // A stop before this frame's dispatch files its windows too: the build's NotifyEnd of a window that is on
        // runs in this frame's TriggerAnimNotifies and CompleteBranchNotify (@339) still finds the entry (@1060), with
        // or without a stored press (an interrupting stop, BPI_MovementStopAnim @106241, is Montage_Stop with no
        // ClearQueuedAttack); this frame's `terminate_fading` makes them due. Filed before OnMontageBlendingOut,
        // whose maps are not empty then (@1802-2111 keeps CurrentMontage, so IsBranchValid @49 refuses a branch to
        // the same attack).
        // Unverified: a stop after the frame's TickPose (`cmc_pose_tick`) and before its dispatch is taken as one
        // before both: the build's dispatch still lists the windows that TickPose queued and ends them one dispatch
        // later, so the stored press runs one frame early here.
        if !lws.is_empty() && !in_adv {
            let q = fi.branching.queued.clone();
            fi.branching.late_branch = Some(LateBranch { ws: lws, q, cur: key.name });
            fi.branching.late_branch_due = false;
        }
        fi.branching.late_auto = if fi.branching.defer_queued_ends && !laws.is_empty() {
            Some(LateAuto { ws: laws, cur: key.name })
        } else {
            None
        };
        fi.montages.fade_ends = if fade { Some(Vec::new()) } else { None };
        super::montage_blending_out(w, f, uid);
        let fi = w.fighter_mut(f);
        fi.crouch.stop_ahead = !fi.root.anim_updated;
        with_player(w, f, uid, SinkKind::Fighter, 0, |h| h.stop());
        let fi = w.fighter_mut(f);
        fi.crouch.stop_ahead = false;
        fi.branching.defer_queued_ends = false;
        let ends = fi.montages.fade_ends.take();
        let proxy = state_ref(w, f, uid).map_or(0, |s| s.proxy);
        let fk = w.fighter(f).montages.fading.iter().position(|x| x.m.montage.uid == uid);
        if let Some(k) = fk
            && let Some(ends) = ends.filter(|e| !e.is_empty())
        {
            w.fighter_mut(f).montages.fading[k].ends = Some(ends);
        }
        // (a hit whose end took it from `activeHits` keeps what the stop's handlers wrote to it: `stop_keep`)
        let kept = std::mem::replace(&mut w.fighter_mut(f).montages.stop_keep, outer_keep);
        for h in &mut keep {
            if let Some(now) = w.fighter(f).montages.active_hits.get(&h.idx).filter(|x| x.n == h.n) {
                h.clone_from(now);
            } else if let Some(k) = kept.iter().find(|k| k.idx == h.idx && k.n == h.n) {
                h.clone_from(k);
            }
        }
        clear_montage_state(w, f, proxy);
        if let Some(k) = fk
            && !keep.is_empty()
        {
            let mut tags: Vec<String> = Vec::new();
            for h in &keep {
                tags.extend(hit_tags_of(w, f, h));
            }
            let fi = w.fighter_mut(f);
            for h in &mut keep {
                fi.montages.linger_seq -= 1;
                h.idx = fi.montages.linger_seq;
                fi.montages.active_hits.insert(h.idx, h.clone());
            }
            fi.montages.fading[k].hits = Some(keep);
            fi.montages.fading[k].tags = Some(tags);
        }
        if w.fighter(f).shapes.disguise.is_some() {
            crate::combat::disguise::owner_montage_event(w, f, key.name);
        }
        if let Some(h) = w.fighter(f).hooks.char.montage_ended {
            h(w, f, key.name);
        }
        if let Some(e) = ended_now {
            montage_ended_buffs(w, f, key.name, e);
        }
    });
}

/// The HitBoxTags of an active Hitframe's packet (`h.n.p.HitBoxTags ?? []`).
pub fn hit_tags_of(w: &World, f: FighterId, h: &ActiveHit) -> Vec<String> {
    hit_packet(w, f, h)
        .and_then(|p| p.get("HitBoxTags"))
        .and_then(Value::as_arr)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// `endFade`: the fade at `k` of `fading`.
pub fn end_fade(w: &mut World, own: FighterId, k: usize) {
    let (uid, name, ends, tags, hits) = {
        let fd = &w.fighter(own).montages.fading[k];
        (fd.m.montage.uid, fd.m.montage.name, fd.ends.clone(), fd.tags.clone(), fd.hits.clone())
    };
    for n in ends.unwrap_or_default() {
        late_fade_end(w, own, n);
    }
    if is_dash_or_turn(w, own, name) {
        w.fighter_mut(own).sm.dash_stop_blend = false;
    }
    if let Some(tags) = tags {
        let nh = hits.as_ref().map_or(0, Vec::len);
        if !w.fighter(own).buffs.buffs.is_empty() {
            for _ in 0..nh {
                crate::combat::buffs::buff_signal(w, own, BuffSignal::HitBoxesDeactivated, BuffSignalArg::default());
            }
        }
        let fi = w.fighter_mut(own);
        for h in hits.iter().flatten() {
            if fi.montages.active_hits.get(&h.idx).is_some_and(|x| x.idx == h.idx && x.n == h.n) {
                fi.montages.active_hits.shift_remove(&h.idx);
            }
        }
        fi.decay.has_hit_a_fighter = false;
        let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
        hit_boxes_off_tags(w, own, &tags, None, true);
    }
    montage_ended_buffs(w, own, name, true);
    let interrupt = state_ref(w, own, uid).is_some_and(|s| s.interrupt_fired);
    if !interrupt {
        owe_proxy_clear(w, own, uid, None);
    }
}

/// `swapLeftRight`.
fn swap_left_right(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    let mut rest = tag;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("Left") {
            out.push_str("Right");
            rest = r;
        } else if let Some(r) = rest.strip_prefix("Right") {
            out.push_str("Left");
            rest = r;
        } else {
            let c = rest.chars().next().expect("a char");
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// `hitBoxesOff` by the tags' names: `own` the ending Hitframe's key (None: Infinity).
pub fn hit_boxes_off_tags(w: &mut World, f: FighterId, tags: &[&str], own: Option<i32>, reset: bool) {
    let mirroring = match w.fighter(f).vis.hit_mirror {
        Some(m) => m,
        None => fdata(w, f)
            .components
            .pawn
            .get("Comp_Pawn_HitBoxManager_GEN_VARIABLE")
            .and_then(|c| match &c.p {
                Value::Obj(p) => p.get("IsHitBoxMirroringEnabled"),
                _ => None,
            })
            .and_then(Value::as_bool)
            .unwrap_or(true),
    };
    let mut off: IndexSet<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    if mirroring || w.fighter(f).body.facing_dir > 0.0 {
        for t in tags {
            off.insert(swap_left_right(t).to_lowercase());
        }
    }
    let mut entry: IndexMap<String, i32> = IndexMap::new();
    let keys: Vec<i32> = w.fighter(f).montages.active_hits.keys().copied().collect();
    for k in keys {
        let Some(h) = w.fighter(f).montages.active_hits.get(&k) else { continue };
        let ht = hit_tags_of(w, f, h);
        let left: Vec<String> = ht.iter().filter(|t| !off.contains(&t.to_lowercase())).cloned().collect();
        if left.len() == ht.len() {
            continue;
        }
        for t in &ht {
            if off.contains(&t.to_lowercase()) {
                entry.insert(t.to_lowercase(), k);
            }
        }
        if Some(k) == own {
            continue;
        }
        if left.is_empty() {
            w.fighter_mut(f).montages.active_hits.shift_remove(&k);
        } else {
            let mut p = hit_packet(w, f, h).cloned().unwrap_or_default();
            p.insert("HitBoxTags".to_owned(), Value::Arr(left.into_iter().map(Value::Str).collect()));
            set_active_hit_props(w, f, k, p);
        }
    }
    if reset {
        let mut seen: IndexSet<i32> = IndexSet::new();
        for (_, k) in entry {
            if !seen.insert(k) {
                continue;
            }
            let once = {
                let Some(h) = w.fighter(f).montages.active_hits.get(&k) else { continue };
                hit_packet(w, f, h).and_then(|p| p.get("HitEachActorOnlyOnce")).and_then(Value::as_bool) != Some(false)
            };
            if once {
                w.fighter_mut(f).montages.active_hit_write(k, |h| h.hit_set.clear());
                // A sweet spot shares its hitframe's set (hit::share_hit_set).
                let pk = crate::registry::notifies::sweet_partner(k);
                if w.fighter(f).montages.active_hits.contains_key(&pk) {
                    w.fighter_mut(f).montages.active_hit_write(pk, |h| h.hit_set.clear());
                }
            }
        }
    }
}

/// `Fighter.hitBoxesOff(tags, own, reset)`: [`hit_boxes_off_tags`] by the tags' names;
/// `own` the key of the Hitframe that is ending (`None`: the default `Infinity`, no key's).
pub fn hit_boxes_off_at(w: &mut World, f: FighterId, tags: &[Name], own: Option<i32>, reset: bool) {
    let names = w.names.clone();
    let t: Vec<&str> = tags.iter().map(|&t| names.str(t)).collect();
    hit_boxes_off_tags(w, f, &t, own, reset);
}

/// The value `spawnerDespawn` gives: All of a class, or the last / first of it.
pub use crate::fighter::match_side::DespawnRequest;

/// `spawnerDespawn`, as the request (None: nothing to destroy).
pub fn spawner_despawn_of(w: &World, p: &Props) -> Option<DespawnRequest> {
    let cls = text(p, "ActorClass").and_then(|s| w.names.get(s)).unwrap_or_default();
    if truthy(p.get("DestroyAllActorsOnNotifyEnd")) {
        return Some(DespawnRequest::All(cls));
    }
    let last = truthy(p.get("DestroyLastActorsOnNotifyEnd"));
    let first = truthy(p.get("DestroyFirstActorsOnNotifyEnd"));
    if last || first { Some(DespawnRequest::Ends { cls, last, first }) } else { None }
}

/// `lateFadeEnd`.
pub fn late_fade_end(w: &mut World, f: FighterId, n: NotifyRef) {
    let Some(nd) = notify_of(w, f, n) else { return };
    let cls = nd.cls.as_str();
    if cls == "AnimNotify_Spawner_C" || cls == "AnimNotify_ConditionalSpawner_C" {
        if let Some(r) = spawner_despawn_of(w, &nd.p) {
            w.fighter_mut(f).requests.despawn_requests.push(r);
        }
    } else {
        let charging = cls == "AnimNotify_ChargingAttack_C";
        crate::fighter::control::charge_play_rate(w, f, 1.0);
        if charging {
            if let Some(h) = w.fighter(f).hooks.char.charge_end {
                h(w, f, n);
            }
            crate::world::flow::broadcast_charge_end(w, f);
        }
        crate::fighter::control::end_charge_loop_sound(w, f, n);
    }
}

/// `clearFading`.
pub fn clear_fading(w: &mut World, own: FighterId) {
    let uids: Vec<u32> = w.fighter(own).montages.fading.iter().map(|x| x.m.montage.uid).collect();
    for &u in &uids {
        if state_ref(w, own, u).is_some_and(|s| !s.interrupt_fired) {
            owe_proxy_clear(w, own, u, None);
        }
    }
    let ends: Vec<NotifyRef> =
        w.fighter(own).montages.fading.iter().flat_map(|x| x.ends.iter().flatten().copied()).collect::<Vec<_>>();
    for n in ends {
        late_fade_end(w, own, n);
    }
    let fi = w.fighter_mut(own);
    let hits: Vec<ActiveHit> = fi.montages.fading.iter().flat_map(|x| x.hits.iter().flatten().cloned()).collect();
    for h in hits {
        if fi.montages.active_hits.get(&h.idx).is_some_and(|x| x.n == h.n) {
            fi.montages.active_hits.shift_remove(&h.idx);
        }
    }
    let gone = std::mem::take(&mut fi.montages.fading);
    for fd in gone {
        drop_state(w, own, fd.m);
    }
}

/// `terminateFading`.
pub fn terminate_fading(w: &mut World, own: FighterId, phase: i32, dilation: Real) -> bool {
    if phase != 1 {
        w.fighter_mut(own).root.anim_updated = true;
        let ends = std::mem::take(&mut w.fighter_mut(own).branching.pending_move_ends);
        for idx in ends {
            crate::fighter::moves::complete_remove_move(w, own, idx);
        }
        let late = std::mem::take(&mut w.fighter_mut(own).branching.pending_ends);
        for (n, idx) in late {
            super::notify_sink::end(w, own, n, idx);
        }
        crate::fighter::owed_attack_end_crouch(w, own);
        let fi = w.fighter_mut(own);
        fi.branching.late_branch_due = fi.branching.late_branch.is_some();
        fi.branching.late_auto = None;
        if phase == 2 {
            return false;
        }
    }
    let fi = w.fighter_mut(own);
    if dilation == 1.0 {
        for fd in &mut fi.montages.fading {
            if fd.left > 0.0 {
                fd.left -= 1.0;
            }
        }
    }
    let Some(k) = fi.montages.fading.iter().position(|fd| fd.left <= 0.0) else { return false };
    let uid = fi.montages.fading[k].m.montage.uid;
    running(w, own, uid, |w| {
        end_fade(w, own, k);
        let ms = &mut w.fighter_mut(own).montages;
        if let Some(k) = ms.fading.iter().position(|x| x.m.montage.uid == uid) {
            let fd = ms.fading.remove(k);
            drop_state(w, own, fd.m);
        }
    });
    true
}

/// `passEnd`.
pub fn pass_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let b = &mut w.fighter_mut(f).branching;
    if let Some(mi) = b.pending_move_ends.iter().position(|&x| x == idx) {
        b.pending_move_ends.remove(mi);
        crate::fighter::moves::complete_remove_move(w, f, idx);
    }
    let b = &mut w.fighter_mut(f).branching;
    if let Some(ei) = b.pending_ends.iter().position(|e| e.0 == n && e.1 == idx) {
        let e = b.pending_ends.remove(ei);
        super::notify_sink::end(w, f, e.0, e.1);
    }
}

/// `endDropped`.
fn end_dropped(w: &mut World, f: FighterId, uid: u32) {
    if state_ref(w, f, uid).is_none_or(|s| s.active.is_empty()) {
        return;
    }
    w.fighter_mut(f).branching.defer_queued_ends = true;
    with_player(w, f, uid, SinkKind::Fighter, 0, |h| h.end_queued());
    w.fighter_mut(f).branching.defer_queued_ends = false;
}

/// `clearMontageState`: `proxy` the stopped or ended instance's (0: none).
pub fn clear_montage_state(w: &mut World, f: FighterId, proxy: MontageProxy) {
    let fi = w.fighter_mut(f);
    // (`retain` keeps the others' order)
    fi.montages.active_hits.retain(|&k, _| !(0..KEY_STRIDE).contains(&k));
    fi.platforms.evading_through_platform = false;
    if proxy == PROXY_NONE {
        fi.evade.evading = false;
        fi.charge.begin_attack = false;
        fi.charge.anim_attack_off = 0;
    }
    fi.branching.queued = None;
    if fi.charge.charge_notify < KEY_STRIDE {
        fi.charge.charge_notify = -1;
    }
}

/// `advanceMontage`.
pub fn advance_montage(w: &mut World, f: FighterId, phase: i32, dilation: Real) {
    let stopped = terminate_fading(w, f, phase, dilation);
    if phase == 2 {
        dispatch_owed(w, f);
        if w.fighter(f).montages.grouped.is_some() {
            super::grouped::advance_grouped(w, f, false, dilation);
        }
        return;
    }
    if phase == 1 && w.fighter(f).montages.grouped.is_some() {
        warn_text(w, "root motion pose tick with grouped montage instances");
    }
    if w.fighter(f).montages.grouped.is_none() || phase == 1 {
        advance_default(w, f, stopped, phase, dilation);
        return;
    }
    let ms = &w.fighter(f).montages;
    let d = ms.dash_montage.as_ref().map(|s| s.montage.uid);
    let c = d.or_else(|| ms.montage.as_ref().map(|s| s.montage.uid));
    let was = c.is_some_and(|u| state_ref(w, f, u).is_some_and(|s| s.terminated));
    advance_default(w, f, stopped, 0, dilation);
    let held = stopped
        || c.is_some_and(|u| {
            !was && (state_ref(w, f, u).is_some_and(|s| s.terminated)
                || (d.is_none() && w.fighter(f).montages.montage_abort))
        });
    super::grouped::advance_grouped(w, f, held, dilation);
}

/// `advanceDefault`.
pub fn advance_default(w: &mut World, f: FighterId, stopped: bool, phase: i32, dilation: Real) {
    let skip = stopped;
    if let Some(d) = w.fighter(f).montages.dash_montage.as_ref().map(|s| s.montage) {
        let sink = w.fighter(f).anim.plain_sink;
        running(w, f, d.uid, |w| {
            let alive = with_player(w, f, d.uid, sink, 0, |h| h.advance(consts::DT * dilation, skip, dilation != 1.0))
                .unwrap_or(false);
            let over = !alive;
            let here = w.fighter(f).montages.dash_montage.as_ref().is_some_and(|s| s.montage.uid == d.uid);
            if !here && state_ref(w, f, d.uid).is_some_and(|s| s.ended && !s.active.is_empty()) {
                with_player(w, f, d.uid, sink, 0, |h| h.end_queued());
            }
            if state_ref(w, f, d.uid).is_some_and(|s| s.blending_out()) {
                super::montage_blending_out(w, f, d.uid);
            }
            let terminated = state_ref(w, f, d.uid).is_some_and(|s| s.terminated);
            if over || terminated {
                let fi = w.fighter_mut(f);
                if let Some(s) = fi.montages.dash_montage.take() {
                    drop_state(w, f, s);
                }
                if is_dash_or_turn(w, f, d.name) {
                    w.fighter_mut(f).sm.dash_stop_blend = false;
                }
                montage_ended_buffs(w, f, d.name, false);
            } else if grounded(w, f)
                && !(crate::fighter::moves::pending_launch_vector(w, f).map_or(0.0, |v| v.1) > 0.0)
                && crate::fighter::control::plain_active(w, f, crate::name!("AnimNotify_AerialAttack_C"))
                && super::grouped::active_grouped(w, f).is_none()
            {
                stop_dash_montage(w, f, 0.1);
                let fi = w.fighter(f);
                if fi.controls.inp.x.abs() >= 0.3 && !crate::fighter::in_hitstun(w, f) && !fi.evade.evading {
                    crate::fighter::moves::dash_dance(w, f, true);
                }
            }
        });
        return;
    }
    advance_current(w, f, skip, phase, dilation);
}

/// `advanceCurrent`.
pub fn advance_current(w: &mut World, f: FighterId, skip: bool, phase: i32, dilation: Real) {
    let Some(key) = w.fighter(f).montages.montage.as_ref().map(|s| s.montage) else { return };
    let uid = key.uid;
    {
        let ms = &mut w.fighter_mut(f).montages;
        ms.montage_abort = false;
        ms.played_in_advance = false;
        ms.advancing = Some(uid);
    }
    let extract = super::root_motion::root_montage(w, f) == Some(MontageSlot::Main);
    w.fighter_mut(f).root.extracting = if extract { Some(uid) } else { None };
    running(w, f, uid, |w| {
        let dt = consts::DT * dilation;
        let dilated = dilation != 1.0;
        let mut owed: Option<Advanced> = None;
        let alive = if phase == 1 {
            owed = with_player(w, f, uid, SinkKind::Fighter, 0, |h| h.advance_only(dt, skip, dilated)).flatten();
            owed.is_some()
        } else {
            with_player(w, f, uid, SinkKind::Fighter, 0, |h| h.advance(dt, skip, dilated)).unwrap_or(false)
        };
        let fi = w.fighter_mut(f);
        fi.root.extracting = None;
        fi.montages.advancing = None;
        if state_ref(w, f, uid).is_some_and(|s| s.blending_out()) {
            natural_blend_out(w, f, uid);
        }
        if main_uid(w, f) != Some(uid) {
            end_dropped(w, f, uid);
            let ms = &w.fighter(f).montages;
            if ms.montage.is_some() && ms.played_in_advance && !ms.montage_abort {
                advance_current(w, f, false, phase, dilation);
            }
            return;
        }
        if let Some(h) = owed {
            w.fighter_mut(f).root.dispatch_owed = Some(DispatchOwed { m: uid, ranges: h.ranges, gone: h.gone });
            return;
        }
        advance_tail(w, f, uid, alive);
    });
}

/// `dispatchOwed`.
fn dispatch_owed(w: &mut World, f: FighterId) {
    let Some(o) = w.fighter_mut(f).root.dispatch_owed.take() else { return };
    let uid = o.m;
    if main_uid(w, f) != Some(uid) {
        return;
    }
    w.fighter_mut(f).montages.advancing = Some(uid);
    running(w, f, uid, |w| {
        let h = Advanced { ranges: o.ranges, gone: o.gone };
        let alive = with_player(w, f, uid, SinkKind::Fighter, 0, |p| p.dispatch_advanced(&h)).unwrap_or(false);
        w.fighter_mut(f).montages.advancing = None;
        if state_ref(w, f, uid).is_some_and(|s| s.blending_out()) {
            natural_blend_out(w, f, uid);
        }
        if main_uid(w, f) != Some(uid) {
            end_dropped(w, f, uid);
            return;
        }
        advance_tail(w, f, uid, alive);
    });
}

/// `advanceTail`.
fn advance_tail(w: &mut World, f: FighterId, uid: u32, alive: bool) {
    let Some((key, terminated, proxy)) = state_ref(w, f, uid).map(|s| (s.montage, s.terminated, s.proxy)) else {
        return;
    };
    if alive && terminated {
        let fi = w.fighter_mut(f);
        if proxy == PROXY_NONE {
            fi.evade.evading = false;
        }
        fi.platforms.evading_through_platform = false;
    }
    if w.fighter(f).rage.rage_started {
        w.fighter_mut(f).rage.rage_started = false;
        let fi = w.fighter(f);
        let rage_is = fi.rage.rage_input.as_ref().is_some_and(|r| r.uid == uid);
        if alive && main_uid(w, f) == Some(uid) && rage_is && !fi.controls.inp.special {
            crate::combat::defense::rage_release(w, f);
        }
    }
    if alive {
        notify_ticks(w, f, MontageSlot::Main, uid, 0);
    }
    let aerial = crate::name!("AnimNotify_AerialAttack_C");
    if alive
        && main_uid(w, f) == Some(uid)
        && grounded(w, f)
        && !(crate::fighter::moves::pending_launch_vector(w, f).map_or(0.0, |v| v.1) > 0.0)
        && has_active(w, f, aerial, crate::real(POLY_INST))
        && super::grouped::active_grouped(w, f).is_none()
    {
        let evading = w.fighter(f).evade.evading;
        stop_montage(w, f, Some(0.1), None);
        if w.fighter(f).controls.inp.x.abs() >= 0.3 && !crate::fighter::in_hitstun(w, f) && !evading {
            crate::fighter::moves::dash_dance(w, f, true);
        }
        return;
    }
    if !alive && main_uid(w, f) == Some(uid) {
        let s = w.fighter_mut(f).montages.montage.take().expect("the montage");
        let fired = s.blend_out_fired;
        w.fighter_mut(f).montages.limbo.push(s);
        if !fired {
            owe_proxy_clear(w, f, uid, None);
        }
        clear_montage_state(w, f, proxy);
        if w.fighter(f).shapes.disguise.is_some() {
            crate::combat::disguise::owner_montage_event(w, f, key.name);
        }
        if let Some(h) = w.fighter(f).hooks.char.montage_ended {
            h(w, f, key.name);
        }
        montage_ended_buffs(w, f, key.name, false);
    }
}

/// `instLive`.
pub fn inst_live(w: &World, f: FighterId, uid: u32, inst: i32) -> bool {
    if inst == 0 {
        return main_uid(w, f) == Some(uid);
    }
    if FOREIGN_INSTS.contains(&inst) {
        return foreign_play(w, f, inst).is_some_and(|cp| cp.m.montage.uid == uid && !cp.stopped);
    }
    if let Some((l, _)) = &w.fighter(f).montages.grouped {
        for e in l {
            if e.m.montage.uid == uid {
                return e.inst.unwrap_or(0) == inst && e.fade.is_none();
            }
        }
    }
    false
}

fn btn_held(fi: &Fighter, b: i32) -> bool {
    use crate::input::AttackBtn;
    let inp = &fi.controls.inp;
    match b {
        0 => inp.attack && inp.force_release != Some(AttackBtn::Attack),
        1 => inp.special && inp.force_release != Some(AttackBtn::Special),
        2 => inp.jump,
        3 => inp.dodge,
        _ => false,
    }
}

/// `notifyTicks`: the NotifyTicks of one attack instance's own states (`slot` its
/// place, `uid` its number, `inst` its instance number).
pub fn notify_ticks(w: &mut World, f: FighterId, slot: MontageSlot, uid: u32, inst: i32) {
    let names = w.names.clone();
    let data = w.data.clone();
    let only = Some(crate::real(inst));
    let empty = Props::new();
    let props = |w: &World, n: NotifyRef| -> &Props {
        instance_data(&data, w, f, uid).and_then(|m| m.notifies.get(n.index as usize)).map_or(&empty, |n| &n.p)
    };
    // the list of each class's states, the World's buffer (a hook below that asks for one makes its own)
    let mut xs = std::mem::take(&mut w.scratch.anim.active);
    let name_of = |p: &Props, k: &str| text(p, k).and_then(|s| names.get(s));
    if inst_live(w, f, uid, inst) {
        active_of_into(w, f, crate::name!("AnimNotify_ConditionSectionJump_C"), only, &mut xs);
        for &x in &xs {
            let p = props(w, x.n);
            let conds = super::branches::conds_of(w, p, "Conditions");
            if !super::branches::conds_ok(w, f, conds.as_deref(), true) {
                continue;
            }
            if truthy(p.get("ClearBranchManagerOnJump")) {
                super::branches::clear_queued_attack(w, f);
            }
            crate::fighter::moves::nav_jump(w, f, slot, name_of(p, "JumpToSection").unwrap_or(crate::name!("None")));
            break;
        }
    }
    if inst_live(w, f, uid, inst) {
        active_of_into(w, f, crate::name!("AnimNotify_ConditionalControllerRumble_C"), only, &mut xs);
        for &x in &xs {
            let p = props(w, x.n);
            let conds = super::branches::conds_of(w, p, "Conditions");
            if super::branches::conds_ok(w, f, conds.as_deref(), false) {
                crate::fighter::tick::controller_rumble(w, f, name_of(p, "ControllerRumble"));
            }
        }
    }
    if inst_live(w, f, uid, inst) {
        active_of_into(w, f, crate::name!("AnimNotify_ButtonHoldSectionJump_C"), only, &mut xs);
        for &x in &xs {
            let p = props(w, x.n);
            let b = int(num(p, "ActiveButton").unwrap_or(0.0));
            if btn_held(w.fighter(f), b) {
                continue;
            }
            let c = text(p, "Conditions");
            let ok = match c {
                None | Some("") | Some("None") => true,
                Some(c) => super::branches::cond_reset_ok(w, f, names.get(c).unwrap_or_default()),
            };
            let s = if ok { name_of(p, "JumpToSection") } else { name_of(p, "FailedConditionJumpToSection") };
            crate::fighter::moves::nav_jump(w, f, slot, s.unwrap_or(crate::name!("None")));
            break;
        }
    }
    if inst_live(w, f, uid, inst) {
        active_of_into(w, f, crate::name!("AnimNotify_ButtonPressedSectionJump_C"), only, &mut xs);
        for &x in &xs {
            let p = props(w, x.n);
            let btns: Vec<i32> = p
                .get("ActiveButton")
                .and_then(Value::as_arr)
                .map(|a| a.iter().filter_map(|v| v.as_f64()).map(crate::math::dbl::to_int32).collect())
                .unwrap_or_default();
            if !btns.iter().any(|&b| btn_held(w.fighter(f), b)) {
                continue;
            }
            crate::fighter::moves::nav_jump(w, f, slot, name_of(p, "JumpToSection").unwrap_or(crate::name!("None")));
            break;
        }
    }
    if inst_live(w, f, uid, inst) {
        active_of_into(w, f, crate::name!("AnimNotify_InputSectionJump_C"), only, &mut xs);
        for &x in &xs {
            let p = props(w, x.n);
            let conds = super::branches::conds_of(w, p, "Conditions");
            if !super::branches::conds_ok(w, f, conds.as_deref(), false) {
                continue;
            }
            let Some(dirs) = p.get("InputeDirection").and_then(Value::as_arr) else { continue };
            let fi = w.fighter(f);
            let d = crate::real(crate::input::get8_way(fi.controls.inp.x, fi.controls.inp.y, fi.body.facing_dir) as u8);
            if !dirs.iter().any(|v| v.as_f64() == Some(f64::from(d))) {
                continue;
            }
            let sec = text(p, "JumpToSection").unwrap_or("None");
            if let Some(a) = super::grouped::active_instance(w, f)
                && let Some(u) = slot_uid(w, f, a)
                && let Some(md) = instance_data(&data, w, f, u)
                && let Some(s) = state_mut(w, f, u)
            {
                s.jump_to_section(md, sec);
            }
            if w.fighter(f).shapes.disguise.is_some() {
                crate::combat::disguise::mirror_section_jump(
                    w,
                    f,
                    Some(names.get(sec).unwrap_or(crate::name!("None"))),
                );
            }
            break;
        }
    }
    if inst_live(w, f, uid, inst) && !w.fighter(f).moves.move_cmds.is_empty() {
        for c in CORNER_CHECK_CLASSES {
            active_of_into(w, f, c, only, &mut xs);
            for &x in &xs {
                let p = props(w, x.n);
                if !truthy(p.get("CornerCheck")) {
                    continue;
                }
                let has = w.fighter(f).moves.move_cmds.iter().any(|m| m.idx == x.i);
                if has
                    && crate::fighter::moves::corner_trace(w, f, 1).is_none()
                    && crate::fighter::moves::corner_trace(w, f, -1).is_some()
                    && let Some(m) = w.fighter_mut(f).moves.move_cmds.iter_mut().find(|m| m.idx == x.i)
                {
                    m.wall_skate = true;
                    m.corner = true;
                }
            }
        }
    }
    w.scratch.anim.active = xs;
}

/// The Ak one-shot of a grouped instance's instant (`groupedSink`): a sound event.
pub(crate) fn sound_event(w: &mut World, f: FighterId, ev: &str) {
    let Some(ev) = w.names.get(ev) else { return };
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: id,
        ev,
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    });
}
