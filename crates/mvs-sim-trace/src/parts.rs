//! The fighter's parts as the record writes them: the typed sub-records (`MontageRecord`, `BuffRecord`, ...: the
//! MONTAGE, BUFF, ...) and the plain values of its `rest` (a montage, a fade, the small
//! structs), each mapped once here.

use indexmap::{IndexMap, IndexSet};
use mvs_anim::montage::{Started, blend_ticks};
use mvs_data::fighter::Branch;
use mvs_data::value::{Props, Value as DataValue};
use mvs_sim::Real;
use mvs_sim::actors::ItemId;
use mvs_sim::actors::actor_montage::ActorAnim;
use mvs_sim::actors::item_bounce::Bounce;
use mvs_sim::combat::buffs::BuffOwn;
use mvs_sim::combat::defense::ArmorPack;
use mvs_sim::combat::hit::Launch;
use mvs_sim::combat::holds::HoldHit;
use mvs_sim::combat::perks::PerkExt;
use mvs_sim::combat::{ActiveHit, BuffClass, BuffExt, FighterBuff};
use mvs_sim::events::NotifyRef;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::GroupedEntry;
use mvs_sim::fighter::anim::montages::KEY_STRIDE;
use mvs_sim::fighter::anim::{
    AnimKey, ChargeBtn, ChargeRamp, DispatchOwed, Fade, ForcedPress, GroundState, HoldOverride, LateBranch, MontageKey,
    MontageMesh, QueuedAction, QueuedBtn, SmBlend,
};
use mvs_sim::fighter::control::{AbilityState, SpamDelayed};
use mvs_sim::fighter::match_side::{Ally, AllySafe, ProjAim};
use mvs_sim::fighter::moves::{GroundJump, Impulse, MoveCmd, MoveCtl, MoveEnd, V2};
use mvs_sim::fighter::tick::FrameMode;
use mvs_sim::fighter::{DodgeState, MontageState};
use mvs_sim::input::{AttackBtn, Btn, Dir, InputState, Order};
use mvs_sim::names::Name;
use mvs_sim::rng::Rng;
use mvs_sim::world::stage::ComponentMove;
use mvs_trace::{
    AbilityRecord, ActiveHitRecord, ArmorRecord, BuffRecord, ForcedRecord, ImpulseRecord, InputRecord,
    LateBranchRecord, LaunchRecord, MontageRecord, MoveCmdRecord, MoveEndRecord, NamedRecord, NotifyAtRecord, Obj,
    QueuedRecord, RecentAttackRecord, Value,
};

use crate::conv::{
    Plain, Rec, as_obj, check_keys, int, named_enum, num, obj, plain_keys, plain_struct, rec_rows, rec_struct, text,
};
use crate::cx::{Lx, Wx};

// ------------------------------------------------------------------ enums by their oracle strings

named_enum!(GroundState, "ground state", [
    GroundState::Fall => "fall", GroundState::Jump1 => "jump1", GroundState::Jump2 => "jump2",
    GroundState::FastFall => "fastFall", GroundState::Wall => "wall", GroundState::HitStunned => "hitStunned",
    GroundState::Recovery => "recovery", GroundState::Bounce => "bounce", GroundState::HitWobble => "hitWobble",
    GroundState::A2g => "a2g", GroundState::Entry => "entry", GroundState::Run => "run",
    GroundState::RunStop => "runStop", GroundState::Idle => "idle", GroundState::Crouch => "crouch",
    GroundState::AltEnter => "altEnter", GroundState::AltLoop => "altLoop", GroundState::IdleReset => "idleReset",
    GroundState::RunStop2 => "runStop2", GroundState::CrouchIn => "crouchIn", GroundState::CrouchOut => "crouchOut",
    GroundState::HoverIn => "hoverIn", GroundState::HoverLoop => "hoverLoop", GroundState::CrouchRun => "crouchRun",
    GroundState::CrouchRunIn => "crouchRunIn", GroundState::CrouchIntoCrouchRun => "crouchIntoCrouchRun",
    GroundState::CrouchRunIntoCrouch => "crouchRunIntoCrouch",
]);

named_enum!(FrameMode, "frame mode", [
    FrameMode::Dead => "dead", FrameMode::Paused => "paused", FrameMode::Hitstun => "hitstun",
    FrameMode::Normal => "normal",
]);

named_enum!(Btn, "button", [
    Btn::Attack => "attack", Btn::Special => "special", Btn::Jump => "jump", Btn::Dodge => "dodge", Btn::Item => "item",
]);

named_enum!(AttackBtn, "attack button", [AttackBtn::Attack => "attack", AttackBtn::Special => "special"]);

named_enum!(ChargeBtn, "charge button", [
    ChargeBtn::Attack(AttackBtn::Attack) => "attack", ChargeBtn::Attack(AttackBtn::Special) => "special",
    ChargeBtn::Jump => "jump", ChargeBtn::Dodge => "dodge",
]);

named_enum!(QueuedBtn, "queued press", [
    QueuedBtn::Press(Btn::Attack) => "attack", QueuedBtn::Press(Btn::Special) => "special",
    QueuedBtn::Press(Btn::Jump) => "jump", QueuedBtn::Press(Btn::Dodge) => "dodge",
    QueuedBtn::Press(Btn::Item) => "item", QueuedBtn::Taunt => "taunt",
]);

/// The proxy a montage play made (`MontageProxy`), by its index in [`PROXIES`].
const PROXIES: [&str; 5] = ["", "attack", "branchAttack", "evade", "evadeDrop"];

/// A direction by its number (`AttackDir`, the oracle's `0..9`).
const DIRS: [Dir; 10] = [
    Dir::Up,
    Dir::Down,
    Dir::Fwd,
    Dir::Back,
    Dir::Neutral,
    Dir::Default,
    Dir::FwdUp,
    Dir::FwdDown,
    Dir::BackUp,
    Dir::BackDown,
];

impl Rec<f64> for Dir {
    fn put(&self, _: &Wx) -> f64 {
        f64::from(*self as u8)
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        if let Some(d) = Self::make(r, l) {
            *self = d;
        }
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        let i = int(*r, l)?;
        let d = usize::try_from(i).ok().and_then(|i| DIRS.get(i).copied());
        if d.is_none() {
            l.miss(format_args!("{i} is not a direction"));
        }
        d
    }
}

impl Plain for Dir {
    fn to_value(&self, x: &Wx) -> Value {
        Value::Num(Rec::<f64>::put(self, x))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(d) = <Self as Plain>::from_value(v, l) {
            *self = d;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let n = num(v, l)?;
        <Self as Rec<f64>>::make(&n, l)
    }
}

impl Rec<f64> for Rng {
    fn put(&self, _: &Wx) -> f64 {
        f64::from(self.s)
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        if let Some(s) = int(*r, l) {
            self.s = s;
        }
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        int(*r, l).map(Rng::new)
    }
}

/// An item by its oracle id (`ID(f.heldItem)`).
impl Rec<f64> for Option<ItemId> {
    fn put(&self, x: &Wx) -> f64 {
        self.map_or(-1.0, |id| x.w.actors.items.get(id).map_or(-1.0, |i| f64::from(i.seq)))
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        if let Some(v) = Self::make(r, l) {
            *self = v;
        }
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        if *r < 0.0 {
            return Some(None);
        }
        match l.aref("item", *r)? {
            mvs_sim::actors::ActorRef::Item(id) => Some(Some(id)),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ the montage

/// `MontagePlayer.weight`: the blend-in ramp by updates, its own blend-out falling from the weight it began at.
fn weight(m: &MontageState, blend_in: Real) -> Real {
    if m.terminated || m.ended {
        return 0.0;
    }
    let in_ticks = blend_ticks(f64::from(blend_in));
    if m.blend_left < 0.0 {
        return if in_ticks > 0.0 { mvs_sim::math::narrow((m.age / in_ticks).min(1.0)) } else { 1.0 };
    }
    let out_ticks = blend_ticks(m.blend_time);
    if out_ticks.is_nan() || out_ticks <= 0.0 {
        return 0.0;
    }
    let at = (m.age - (out_ticks - m.blend_left)).max(0.0);
    let from = if in_ticks > 0.0 { (at / in_ticks).min(1.0) } else { 1.0 };
    mvs_sim::math::narrow(from * (m.blend_left / out_ticks).min(1.0))
}

/// `sectionIndexAt(pos)`'s name: the last section starting at or before `t`.
fn section_at(sections: &[(String, f64)], t: Real) -> &str {
    const EPS: f64 = 1e-7;
    let mut idx = None;
    for (i, (_, at)) in sections.iter().enumerate() {
        if t >= mvs_sim::math::narrow(at - EPS) {
            idx = Some(i);
        }
    }
    idx.or(if sections.is_empty() { None } else { Some(0) }).map_or("", |i| sections[i].0.as_str())
}

fn started_put(s: Started) -> Value {
    match s {
        Started::No => Value::Bool(false),
        Started::Yes => Value::Bool(true),
        Started::Jump => Value::Str("jump".to_owned()),
    }
}

fn started_load(v: &Value, l: &mut Lx) -> Option<Started> {
    match v {
        Value::Bool(false) => Some(Started::No),
        Value::Bool(true) => Some(Started::Yes),
        Value::Str(s) if s == "jump" => Some(Started::Jump),
        _ => {
            l.miss(format_args!("{v:?} is not a montage's started"));
            None
        }
    }
}

fn proxy_load(s: &str, l: &mut Lx) -> Option<u8> {
    let i = PROXIES.iter().position(|p| *p == s);
    if i.is_none() {
        l.miss(format_args!("{s:?} is not a montage proxy"));
    }
    i.map(|i| u8::try_from(i).expect("five proxies"))
}

/// A montage of the data by name, else named (a montage the data does not have cannot be played).
fn montage_name(s: &str, l: &mut Lx) -> Name {
    if !l.book.montages.contains_key(s) {
        l.miss(format_args!("no montage {s} in the data"));
    }
    l.name(s)
}

/// The number a loaded instance is given: counted down from `u32::MAX`, so it meets no play's number
/// (`Montages::play_seq` counts up from 0) and no other loaded instance's.
pub(crate) fn loaded_key(name: Name) -> MontageKey {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);
    MontageKey { name, uid: NEXT.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) }
}

/// A new instance of a montage, its fields then loaded: `MontagePlayer`'s initialisers.
fn new_montage(montage: Name) -> MontageState {
    MontageState {
        montage: loaded_key(montage),
        pos: 0.0,
        ended: false,
        active: Vec::new(),
        in_advance: false,
        terminated: false,
        blend_out_fired: false,
        proxy: 0,
        interrupt_fired: false,
        play_rate: 1.0,
        walking: false,
        started: Started::No,
        age: 0.0,
        at_end: false,
        blend_left: -1.0,
        blend_time: 0.0,
    }
}

impl Rec<MontageRecord> for MontageState {
    fn put(&self, x: &Wx) -> MontageRecord {
        let name = x.str(self.montage.name);
        let info = x.book.montages.get(name);
        if info.is_none() {
            x.unmapped(&format!("a montage {name} the data does not have (its section and weight)"));
        }
        let mut r = MontageRecord {
            name: name.to_owned(),
            section: info.map_or("", |i| section_at(&i.sections, mvs_sim::math::narrow(self.pos))).to_owned(),
            weight: info.map_or(0.0, |i| f64::from(weight(self, mvs_sim::math::narrow(i.blend_in)))),
            blending_out: self.blend_left >= 0.0 || self.terminated,
            started: started_put(self.started),
            proxy: PROXIES.get(usize::from(self.proxy)).copied().unwrap_or("?").to_owned(),
            active: self.active.iter().map(|&i| i as f64).collect(),
            ..MontageRecord::default()
        };
        rec_rows!(put self, x, r;
            position: pos, age: age, play_rate: play_rate, ended: ended, terminated: terminated,
            blend_out_fired: blend_out_fired, interrupt_fired: interrupt_fired, at_end: at_end,
            blend_left: blend_left, blend_time: blend_time, in_advance: in_advance, walking: walking,
        );
        r
    }
    fn load(&mut self, r: &MontageRecord, l: &mut Lx) {
        if l.str(self.montage.name) != r.name {
            self.montage = loaded_key(montage_name(&r.name, l));
        }
        rec_rows!(load self, r, l;
            position: pos, age: age, play_rate: play_rate, ended: ended, terminated: terminated,
            blend_out_fired: blend_out_fired, interrupt_fired: interrupt_fired, at_end: at_end,
            blend_left: blend_left, blend_time: blend_time, in_advance: in_advance, walking: walking,
        );
        l.at("active", |l| {
            self.active = r.active.iter().filter_map(|&a| usize::try_from(int(a, l)?).ok()).collect();
        });
        if let Some(s) = l.at("started", |l| started_load(&r.started, l)) {
            self.started = s;
        }
        if let Some(p) = l.at("proxy", |l| proxy_load(&r.proxy, l)) {
            self.proxy = p;
        }
        // (section, weight and blending_out are read from the fields above and the data: nothing to set)
    }
    fn make(r: &MontageRecord, l: &mut Lx) -> Option<Self> {
        let mut m = new_montage(montage_name(&r.name, l));
        m.load(r, l);
        Some(m)
    }
}

/// A montage instance in `rest` (stateOf of a MontagePlayer: its own keys, the montage as `{static: name}`).
impl Plain for MontageState {
    fn to_value(&self, x: &Wx) -> Value {
        let active = Value::List(self.active.iter().map(|&i| Value::Num(i as f64)).collect());
        let proxy = PROXIES.get(usize::from(self.proxy)).copied().unwrap_or("?");
        obj(vec![
            ("active", obj(vec![("set", active)])),
            ("age", Value::Num(self.age)),
            ("atEnd", Value::Bool(self.at_end)),
            ("blendLeft", Value::Num(self.blend_left)),
            ("blendOutFired", Value::Bool(self.blend_out_fired)),
            ("blendTime", Value::Num(self.blend_time)),
            ("ended", Value::Bool(self.ended)),
            ("inAdvance", Value::Bool(self.in_advance)),
            ("interruptFired", Value::Bool(self.interrupt_fired)),
            ("m", obj(vec![("static", Value::Str(x.str(self.montage.name).to_owned()))])),
            ("playRate", Value::Num(self.play_rate)),
            ("pos", Value::Num(self.pos)),
            ("proxy", Value::Str(proxy.to_owned())),
            ("started", started_put(self.started)),
            ("terminated", Value::Bool(self.terminated)),
            ("walking", Value::Bool(self.walking)),
        ])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        if let Some(Value::Obj(m)) = o.get("m")
            && let Some(Value::Str(name)) = m.get("static")
        {
            if l.str(self.montage.name) != name {
                self.montage = loaded_key(montage_name(name, l));
            }
        } else {
            l.miss("a montage instance with no m {static}");
        }
        let f = |k: &str, l: &mut Lx, set: &mut dyn FnMut(&Value, &mut Lx)| match o.get(k) {
            Some(v) => l.at(k, |l| set(v, l)),
            None => l.miss(format_args!("key {k} absent")),
        };
        f("active", l, &mut |v, l| {
            let mut s: IndexSet<i32> = IndexSet::new();
            s.load_value(v, l);
            self.active = s.into_iter().filter_map(|i| usize::try_from(i).ok()).collect();
        });
        f("age", l, &mut |v, l| self.age.load_value(v, l));
        f("atEnd", l, &mut |v, l| self.at_end.load_value(v, l));
        f("blendLeft", l, &mut |v, l| self.blend_left.load_value(v, l));
        f("blendOutFired", l, &mut |v, l| self.blend_out_fired.load_value(v, l));
        f("blendTime", l, &mut |v, l| self.blend_time.load_value(v, l));
        f("ended", l, &mut |v, l| self.ended.load_value(v, l));
        f("inAdvance", l, &mut |v, l| self.in_advance.load_value(v, l));
        f("interruptFired", l, &mut |v, l| self.interrupt_fired.load_value(v, l));
        f("playRate", l, &mut |v, l| self.play_rate.load_value(v, l));
        f("pos", l, &mut |v, l| self.pos.load_value(v, l));
        f("proxy", l, &mut |v, l| {
            if let Some(p) = text(v, l).map(str::to_owned).and_then(|s| proxy_load(&s, l)) {
                self.proxy = p;
            }
        });
        f("started", l, &mut |v, l| {
            if let Some(s) = started_load(v, l) {
                self.started = s;
            }
        });
        f("terminated", l, &mut |v, l| self.terminated.load_value(v, l));
        f("walking", l, &mut |v, l| self.walking.load_value(v, l));
        check_keys(o, &MONTAGE_KEYS, l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut m = new_montage(Name::NONE);
        m.load_value(v, l);
        Some(m)
    }
}

/// A grouped montage instance (`GroupedEntry`): `fade` null or {left, of, time, w}; `top`,
/// `inst` and `charge` only while set (the oracle's undefined).
impl Plain for GroupedEntry {
    fn to_value(&self, x: &Wx) -> Value {
        let fade = self.fade.map_or(Value::Null, |[left, of, time, w]| {
            obj(vec![
                ("left", Value::Num(f64::from(left))),
                ("of", Value::Num(f64::from(of))),
                ("time", Value::Num(f64::from(time))),
                ("w", Value::Num(f64::from(w))),
            ])
        });
        let mut e = vec![("fade", fade), ("m", self.m.to_value(x))];
        if self.top {
            e.push(("top", Value::Bool(true)));
        }
        if let Some(i) = self.inst {
            e.push(("inst", Value::Num(f64::from(i))));
        }
        if self.charge {
            e.push(("charge", Value::Bool(true)));
        }
        obj(e)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        match o.get("m") {
            Some(v) => l.at("m", |l| self.m.load_value(v, l)),
            None => l.miss("key m absent"),
        }
        self.fade = match o.get("fade") {
            None | Some(Value::Null) => None,
            Some(Value::Obj(f)) => {
                let mut at = |k: &str| match f.get(k) {
                    Some(Value::Num(n)) => *n,
                    _ => {
                        l.at("fade", |l| l.miss(format_args!("key {k} absent")));
                        0.0
                    }
                };
                Some([
                    mvs_sim::math::narrow(at("left")),
                    mvs_sim::math::narrow(at("of")),
                    mvs_sim::math::narrow(at("time")),
                    mvs_sim::math::narrow(at("w")),
                ])
            }
            Some(_) => {
                l.at("fade", |l| l.miss("no fade object"));
                None
            }
        };
        self.top = matches!(o.get("top"), Some(Value::Bool(true)));
        self.inst = match o.get("inst") {
            Some(v) => l.at("inst", |l| i32::from_value(v, l)),
            None => None,
        };
        self.charge = matches!(o.get("charge"), Some(Value::Bool(true)));
        check_keys(o, &["fade", "m", "top", "inst", "charge"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut e = GroupedEntry { m: new_montage(Name::NONE), fade: None, top: false, inst: None, charge: false };
        e.load_value(v, l);
        Some(e)
    }
}

/// `Fighter.grouped`: `{list, tenticle}` (the port's pair), left out while unset.
pub(crate) fn grouped_put(g: &(Vec<GroupedEntry>, bool), x: &Wx) -> Value {
    obj(vec![("list", g.0.to_value(x)), ("tenticle", Value::Bool(g.1))])
}

/// [`grouped_put`]'s inverse.
pub(crate) fn grouped_load(g: &mut Option<(Vec<GroupedEntry>, bool)>, v: Option<&Value>, l: &mut Lx) {
    let Some(v) = v else {
        *g = None;
        return;
    };
    let Some(o) = as_obj(v, l) else { return };
    let (list, tenticle) = g.get_or_insert_with(|| (Vec::new(), false));
    match o.get("list") {
        Some(v) => l.at("list", |l| list.load_value(v, l)),
        None => l.miss("key list absent"),
    }
    match o.get("tenticle") {
        Some(v) => l.at("tenticle", |l| tenticle.load_value(v, l)),
        None => l.miss("key tenticle absent"),
    }
    check_keys(o, &["list", "tenticle"], l);
}

const MONTAGE_KEYS: [&str; 16] = [
    "active",
    "age",
    "atEnd",
    "blendLeft",
    "blendOutFired",
    "blendTime",
    "ended",
    "inAdvance",
    "interruptFired",
    "m",
    "playRate",
    "pos",
    "proxy",
    "started",
    "terminated",
    "walking",
];

// ------------------------------------------------------------------ hits

/// The record of an active hit names its notify, not its packet: one a handler gave it (`p`) is not written.
fn hit_packet_unmapped(h: &ActiveHit, x: &Wx) {
    if h.p.is_some() {
        x.unmapped("an active hit's own packet (the record names the notify only)");
    }
}

/// The oracle keys a sweet spot's hit beside its hitframe's at `idx + 0.5` (`sweetKey`); the port's keys are integers,
/// half a stride on (`registry::notifies::sweet_key`).
fn key_out(key: i32) -> f64 {
    let half = KEY_STRIDE / 2;
    if key >= 0 && key.rem_euclid(KEY_STRIDE) >= half { f64::from(key - half) + 0.5 } else { f64::from(key) }
}

/// [`key_out`]'s inverse.
fn key_in(key: f64, l: &mut Lx) -> Option<i32> {
    if key.fract() == 0.5 { int(key - 0.5, l).map(|k| k + KEY_STRIDE / 2) } else { int(key, l) }
}

const HITFRAME: &str = "AnimNotify_Hitframe_C";
/// The notify classes whose begin hands the hitframe's a copy `{ ...n, cls: Hitframe }`: a sweet spot hitframe (its
/// two hits), Iron Giant's throw hitframe, Finn's bag, D attack and high five hitframes, Jake's aim hitframe.
const AS_HITFRAME: [&str; 6] = [
    "AnimNotify_SweetSpotHitframe_C",
    "AnimNotify_Hitframe_Jake_Aim_C",
    "AnimNotify_Hitframe_C017_Throw_C",
    "AnimNotify_FinnBag_Hitframe_C",
    "AnimNotify_FinnDAttack_Hitframe_C",
    "AnimNotify_FinnHighFive_Hitframe_C",
];

const UPDATE_HITFRAME: &str = "AnimNotify_UpdateHitframeDamage_C";
/// Jake's aim update, whose begin hands the shared update's a copy `{ ...n, cls: UpdateHitframeDamage }`.
const AS_UPDATE: &str = "AnimNotify_UpdateHitframeDamage_Jake_Aim_C";

/// The notify of an active hit as the oracle records it (`AS_HITFRAME`'s classes as the Hitframe, `AS_UPDATE` as the
/// shared update).
fn hit_notify_put(n: NotifyRef, x: &Wx) -> mvs_trace::NotifyRecord {
    let mut r = n.put(x);
    if AS_HITFRAME.contains(&r.class.as_str()) {
        r.class = HITFRAME.to_owned();
    } else if r.class == AS_UPDATE {
        r.class = UPDATE_HITFRAME.to_owned();
    }
    r
}

/// The notify of a recorded active hit: the data's notify of an `AS_HITFRAME` class where the record's Hitframe is
/// one of it.
fn hit_notify_find(r: &mvs_trace::NotifyRecord, l: &mut Lx) -> mvs_trace::NotifyRecord {
    if r.class == UPDATE_HITFRAME {
        let mut s = r.clone();
        AS_UPDATE.clone_into(&mut s.class);
        if crate::conv::notify_id(&s, l).is_some_and(|id| l.book.find(&id).is_some()) {
            return s;
        }
    }
    if r.class == HITFRAME {
        for c in AS_HITFRAME {
            let mut s = r.clone();
            c.clone_into(&mut s.class);
            let found = crate::conv::notify_id(&s, l).is_some_and(|id| l.book.find(&id).is_some());
            if found {
                return s;
            }
        }
    }
    r.clone()
}

/// The `n` of a faded hit (a plain value, `fading[].hits[]`): the notify by its identity, or for an `AS_HITFRAME`
/// class the oracle's copy `{ ...n, cls: Hitframe }`, which is no notify of the data and is written whole: its `p` the
/// hit's own packet where a handler gave it one (Finn's D attack hitframe: `{ ...n, p: dAttackPacket }`,
/// [`own_packet_put`]), else the notify's.
fn hit_n_put(h: &ActiveHit, x: &Wx) -> Value {
    let n = h.n;
    let Some(id) = x.notify(n) else { return Value::Null };
    // the shared update's notify is a copy where an update cut its HitBoxTags (`{ ...h.n, p }`: the hit has its own
    // packet)
    let update = id.cls == AS_UPDATE || (id.cls == UPDATE_HITFRAME && h.p.is_some());
    if !update && !AS_HITFRAME.contains(&id.cls.as_str()) {
        hit_packet_unmapped(h, x);
        return n.to_value(x);
    }
    let w = x.w;
    let Some((f, _)) = w.fighters.iter().next() else { return Value::Null };
    let Some(d) = mvs_sim::fighter::anim::montages::any_notify_in(&w.data, w, f, n) else {
        x.unmapped("a faded hit's notify not in the data");
        return Value::Null;
    };
    let tick = match d.tick {
        mvs_data::fighter::NotifyTick::Queued => "Queued",
        mvs_data::fighter::NotifyTick::BranchingPoint => "BranchingPoint",
    };
    let mut e = vec![
        ("cls", Value::Str(if update { UPDATE_HITFRAME } else { HITFRAME }.to_owned())),
        ("dur", Value::Num(d.dur)),
        ("p", h.p.as_deref().filter(|p| **p != d.p).map_or_else(|| static_put(&d.p), |p| own_packet_put(p, &d.p))),
        ("state", Value::Bool(d.state)),
        ("t", Value::Num(d.t)),
        ("tick", Value::Str(tick.to_owned())),
        ("track", Value::Num(f64::from(d.track))),
    ];
    for (k, v) in [("inst", &d.inst), ("seq", &d.seq), ("slot", &d.slot)] {
        if let Some(v) = v {
            e.push((k, Value::Str(v.clone())));
        }
    }
    obj(e)
}

/// A hit's own packet (a handler's `{ ...p, ... }`, no static of the data) as the oracle writes it: whole, a value it
/// keeps of the notify's packet (the spread's reference to the data) by the static rule.
// Unverified: a kept value is told from a new one by being equal to the notify's (the oracle tells them by reference);
// the two write alike unless the value is a large object or one with a `name`, `cls` or `key`.
fn own_packet_put(p: &Props, data: &Props) -> Value {
    let e = p
        .iter()
        .map(|(k, v)| {
            let kept = data.get(k) == Some(v);
            let w = match v {
                DataValue::Obj(o) if kept => static_put(o),
                DataValue::Arr(_) if kept => {
                    let mut budget = PLAIN_LEAVES;
                    plain_static(v, &mut budget, 0)
                        .unwrap_or_else(|| obj(vec![("static", Value::Str("list".to_owned()))]))
                }
                v => data_put(v),
            };
            (k.clone(), w)
        })
        .collect();
    Value::Obj(mvs_trace::Obj::new(e))
}

/// A faded hit's own packet from its recorded copy (`n.p` written whole, [`own_packet_put`]); `None` for a notify's
/// static `p` (the hit reads its notify's).
fn own_packet_load(v: &Value) -> Option<std::sync::Arc<Props>> {
    let Value::Obj(o) = v else { return None };
    match o.get("p") {
        Some(p @ Value::Obj(po)) if po.get("static").is_none() => match data_load(p) {
            DataValue::Obj(p) => Some(std::sync::Arc::new(p)),
            _ => None,
        },
        _ => None,
    }
}

/// [`hit_n_put`]'s inverse: a copy written whole is found as the notify of an `AS_HITFRAME` class with its
/// identity. `keep`: the hit's notify, kept where the identity is its.
fn hit_n_find(v: &Value, keep: Option<NotifyRef>, l: &mut Lx) -> Option<NotifyRef> {
    let Value::Obj(o) = v else { return NotifyRef::from_value(v, l) };
    if o.get("notify").is_some() {
        let Some(mut n) = keep else { return NotifyRef::from_value(v, l) };
        n.load_value(v, l);
        return Some(n);
    }
    let s = |k: &str| match o.get(k) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let (Some(Value::Num(t)), Some(Value::Num(track))) = (o.get("t"), o.get("track")) else {
        l.miss("a faded hit's notify copy without t and track");
        return None;
    };
    let track = int(*track, l)?;
    // (the shared update's own class too: a copy `{ ...n, p }` where an update cut the hit's HitBoxTags)
    for c in AS_HITFRAME.into_iter().chain([AS_UPDATE, UPDATE_HITFRAME]) {
        let id = crate::cx::NotifyId {
            cls: c.to_owned(),
            inst: s("inst"),
            t: t.to_bits(),
            track,
            seq: s("seq"),
            slot: s("slot"),
        };
        if l.book.find(&id).is_some() {
            return l.notify(&id, keep);
        }
    }
    l.miss("a faded hit's notify copy of no Hitframe class of the data");
    None
}

fn put_hit_record(key: i32, h: &ActiveHit, x: &Wx) -> ActiveHitRecord {
    hit_packet_unmapped(h, x);
    ActiveHitRecord {
        key: key_out(key),
        attack_id: f64::from(h.attack_id),
        attack_name: x.str(h.attack_name).to_owned(),
        charged_time: f64::from(h.charged_time),
        index: key_out(h.idx),
        notify: hit_notify_put(h.n, x),
        hit_set: h.hit_set.put(x),
        copy_pose: crate::arya::disguise::put_pose(h.copy_pose, x),
        open_pose: h.open_pose.to_value(x),
    }
}

fn load_hit_record(h: &mut ActiveHit, r: &ActiveHitRecord, l: &mut Lx) {
    rec_rows!(load h, r, l;
        attack_id: attack_id, attack_name: attack_name, charged_time: charged_time, hit_set: hit_set,
    );
    l.at("index", |l| {
        if let Some(i) = key_in(r.index, l) {
            h.idx = i;
        }
    });
    let n = hit_notify_find(&r.notify, l);
    l.at("notify", |l| Rec::load(&mut h.n, &n, l));
    l.at("copy_pose", |l| crate::arya::disguise::load_pose(&mut h.copy_pose, &r.copy_pose, l));
    l.at("open_pose", |l| h.open_pose.load_value(&r.open_pose, l));
}

/// `activeHits` (a Map by notify index), in its order.
impl Rec<Vec<ActiveHitRecord>> for IndexMap<i32, ActiveHit> {
    fn put(&self, x: &Wx) -> Vec<ActiveHitRecord> {
        self.iter().map(|(&k, h)| put_hit_record(k, h, x)).collect()
    }
    fn load(&mut self, r: &Vec<ActiveHitRecord>, l: &mut Lx) {
        let mut old = std::mem::take(self);
        for (i, e) in r.iter().enumerate() {
            l.at(&format!("[{i}]"), |l| {
                let Some(k) = key_in(e.key, l) else { return };
                let h = match old.shift_remove(&k) {
                    Some(mut h) => {
                        load_hit_record(&mut h, e, l);
                        Some(h)
                    }
                    None => {
                        let nr = hit_notify_find(&e.notify, l);
                        let n = NotifyRef::make(&nr, l);
                        n.map(|n| {
                            let mut h = ActiveHit {
                                n,
                                idx: 0,
                                hit_set: IndexSet::new(),
                                charged_time: 0.0,
                                attack_id: 0,
                                attack_name: Name::NONE,
                                open_pose: None,
                                copy_pose: None,
                                p: None,
                            };
                            load_hit_record(&mut h, e, l);
                            h
                        })
                    }
                };
                if let Some(h) = h {
                    self.insert(k, h);
                }
            });
        }
    }
    fn make(r: &Vec<ActiveHitRecord>, l: &mut Lx) -> Option<Self> {
        let mut m = IndexMap::new();
        m.load(r, l);
        Some(m)
    }
}

/// An ActiveHit in `rest` (a fade's `hits`): its own keys; `copyPose` is left out while unset (the oracle object has no
/// such key then).
impl Plain for ActiveHit {
    fn to_value(&self, x: &Wx) -> Value {
        let mut e = vec![
            ("attackId", Value::Num(f64::from(self.attack_id))),
            ("attackName", self.attack_name.to_value(x)),
            ("chargedTime", Value::Num(f64::from(self.charged_time))),
            ("hitSet", self.hit_set.to_value(x)),
            ("idx", Value::Num(f64::from(self.idx))),
            ("n", hit_n_put(self, x)),
            ("openPose", self.open_pose.to_value(x)),
        ];
        if self.copy_pose.is_some() {
            e.push(("copyPose", crate::arya::disguise::put_pose(self.copy_pose, x)));
        }
        obj(e)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        let f = |k: &str, l: &mut Lx, set: &mut dyn FnMut(&Value, &mut Lx)| match o.get(k) {
            Some(v) => l.at(k, |l| set(v, l)),
            None => l.miss(format_args!("key {k} absent")),
        };
        f("attackId", l, &mut |v, l| self.attack_id.load_value(v, l));
        f("attackName", l, &mut |v, l| self.attack_name.load_value(v, l));
        f("chargedTime", l, &mut |v, l| self.charged_time.load_value(v, l));
        f("hitSet", l, &mut |v, l| self.hit_set.load_value(v, l));
        f("idx", l, &mut |v, l| self.idx.load_value(v, l));
        f("n", l, &mut |v, l| {
            if let Some(n) = hit_n_find(v, Some(self.n), l) {
                self.n = n;
                if matches!(v, Value::Obj(o) if o.get("notify").is_none()) {
                    self.p = own_packet_load(v);
                }
            }
        });
        f("openPose", l, &mut |v, l| self.open_pose.load_value(v, l));
        match o.get("copyPose") {
            Some(v) => l.at("copyPose", |l| crate::arya::disguise::load_pose(&mut self.copy_pose, v, l)),
            None => self.copy_pose = None,
        }
        check_keys(o, &["attackId", "attackName", "chargedTime", "hitSet", "idx", "n", "openPose", "copyPose"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        let n = hit_n_find(o.get("n")?, None, l)?;
        let mut h = ActiveHit {
            n,
            idx: 0,
            hit_set: IndexSet::new(),
            charged_time: 0.0,
            attack_id: 0,
            attack_name: Name::NONE,
            open_pose: None,
            copy_pose: None,
            p: None,
        };
        h.load_value(v, l);
        Some(h)
    }
}

/// A montage fading out (`fading[]`): `ends` the notifies its NotifyEnds are owed, `tags` the HitBoxTags of its
/// hits (each written only while set, as the oracle's undefined is left out).
impl Plain for Fade {
    fn to_value(&self, x: &Wx) -> Value {
        let mut e = vec![
            ("left", Value::Num(f64::from(self.left))),
            ("m", self.m.to_value(x)),
            ("time", Value::Num(f64::from(self.time))),
        ];
        if let Some(h) = &self.hits {
            e.push(("hits", h.to_value(x)));
        }
        if let Some(t) = &self.tags {
            e.push(("tags", Value::List(t.iter().map(|s| Value::Str(s.clone())).collect())));
        }
        if let Some(n) = &self.ends {
            e.push(("ends", n.to_value(x)));
        }
        obj(e)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        match o.get("left") {
            Some(v) => l.at("left", |l| self.left.load_value(v, l)),
            None => l.miss(format_args!("key left absent")),
        }
        match o.get("time") {
            Some(v) => l.at("time", |l| self.time.load_value(v, l)),
            None => l.miss(format_args!("key time absent")),
        }
        match o.get("m") {
            Some(v) => l.at("m", |l| self.m.load_value(v, l)),
            None => l.miss("key m absent"),
        }
        match o.get("hits") {
            Some(v) => l.at("hits", |l| self.hits.load_value(v, l)),
            None => self.hits = None,
        }
        match o.get("tags") {
            Some(Value::List(t)) => {
                let mut out = Vec::new();
                for v in t {
                    match v {
                        Value::Str(s) => out.push(s.clone()),
                        v => l.at("tags", |l| l.miss(format_args!("{v:?} is not a tag"))),
                    }
                }
                self.tags = Some(out);
            }
            Some(v) => l.at("tags", |l| l.miss(format_args!("{v:?} is not a tag list"))),
            None => self.tags = None,
        }
        match o.get("ends") {
            Some(v) => l.at("ends", |l| self.ends.load_value(v, l)),
            None => self.ends = None,
        }
        check_keys(o, &["left", "m", "time", "hits", "tags", "ends"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        let m = MontageState::from_value(o.get("m")?, l)?;
        let mut f = Fade { left: 0.0, time: 0.0, m, hits: None, tags: None, ends: None };
        f.load_value(v, l);
        Some(f)
    }
}

/// A montage instance by its number (`Montages::advancing`, `DispatchOwed::m`): the oracle holds the player object.
/// Unverified: neither is set between two steps (the advance and the owed dispatch end inside the step), so the record
/// meets only their null; one that is set is named.
impl Plain for u32 {
    fn to_value(&self, x: &Wx) -> Value {
        x.unmapped("a montage instance by number (advancing, dispatchOwed.m) set between two steps");
        Value::Null
    }
    fn load_value(&mut self, _: &Value, l: &mut Lx) {
        l.miss("a montage instance by number (advancing, dispatchOwed.m) set between two steps");
    }
    fn from_value(_: &Value, l: &mut Lx) -> Option<Self> {
        l.miss("a montage instance by number (advancing, dispatchOwed.m) set between two steps");
        None
    }
}

impl Plain for DispatchOwed {
    fn to_value(&self, x: &Wx) -> Value {
        obj(vec![("m", self.m.to_value(x))])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        match o.get("m") {
            Some(m) => l.at("m", |l| self.m.load_value(m, l)),
            None => l.miss("key m absent"),
        }
        check_keys(o, &["m"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        Some(DispatchOwed { m: u32::from_value(o.get("m")?, l)?, ranges: Vec::new(), gone: false })
    }
}

// ------------------------------------------------------------------ the typed sub-records

rec_struct!(Launch => LaunchRecord { vx: vx, vy: vy, di: di, wall: wall, kbi: kbi });
rec_struct!(AbilityState => AbilityRecord {
    cooldown: cooldown, cd_frames: cd_frames, ammo: ammo, ammo_cooldown: ammo_cooldown, ammo_frames: ammo_frames,
    default_cd: default_cd, active_cd: active_cd, ammo_max: ammo_max,
});
rec_struct!(ArmorPack => ArmorRecord {
    kind: kind, amount: amount, max_hits: max_hits, threshold: threshold, dmg_res: dmg_res, hs_res: hs_res,
    kb_res: kb_res, can_override: can_override,
});
// (Impulse.d, LaunchSelf's duration, is no key of the oracle impulse the record names: the exact row would refuse it)
rec_struct!(Impulse => ImpulseRecord { kind: kind, vx: vx, vy: vy, xy: xy, z: z, depth: d });

/// `(NotifyRef, index)`: `pendingEnds`, `lateBranch.ws`.
impl Rec<NotifyAtRecord> for (NotifyRef, i32) {
    fn put(&self, x: &Wx) -> NotifyAtRecord {
        NotifyAtRecord { index: f64::from(self.1), notify: self.0.put(x) }
    }
    fn load(&mut self, r: &NotifyAtRecord, l: &mut Lx) {
        l.at("notify", |l| self.0.load(&r.notify, l));
        l.at("index", |l| self.1.load(&r.index, l));
    }
    fn make(r: &NotifyAtRecord, l: &mut Lx) -> Option<Self> {
        Some((l.at("notify", |l| NotifyRef::make(&r.notify, l))?, int(r.index, l)?))
    }
}

/// A Map of names to numbers written as `[key, value]` records (`comboResMap`).
impl Rec<Vec<NamedRecord>> for IndexMap<Name, Real> {
    fn put(&self, x: &Wx) -> Vec<NamedRecord> {
        self.iter().map(|(k, v)| NamedRecord { key: x.str(*k).to_owned(), value: f64::from(*v) }).collect()
    }
    fn load(&mut self, r: &Vec<NamedRecord>, l: &mut Lx) {
        self.clear();
        for e in r {
            let k = l.name(&e.key);
            self.insert(k, mvs_sim::math::narrow(e.value));
        }
    }
    fn make(r: &Vec<NamedRecord>, l: &mut Lx) -> Option<Self> {
        let mut m = IndexMap::new();
        m.load(r, l);
        Some(m)
    }
}

impl Rec<RecentAttackRecord> for (Btn, Option<Name>) {
    fn put(&self, x: &Wx) -> RecentAttackRecord {
        RecentAttackRecord { button: self.0.put(x), montage: self.1.put(x) }
    }
    fn load(&mut self, r: &RecentAttackRecord, l: &mut Lx) {
        l.at("button", |l| self.0.load(&r.button, l));
        l.at("montage", |l| self.1.load(&r.montage, l));
    }
    fn make(r: &RecentAttackRecord, l: &mut Lx) -> Option<Self> {
        Some((Btn::make(&r.button, l)?, <Option<Name>>::make(&r.montage, l)?))
    }
}

/// A move end's bound (`min`, `max`): a V2 the oracle holds as `{X, Y}`.
impl Plain for V2 {
    fn to_value(&self, _: &Wx) -> Value {
        obj(vec![("X", Value::Num(f64::from(self.x))), ("Y", Value::Num(f64::from(self.y)))])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        for (k, set) in [("X", &mut self.x), ("Y", &mut self.y)] {
            match o.get(k) {
                Some(v) => l.at(k, |l| set.load_value(v, l)),
                None => l.miss(format_args!("key {k} absent")),
            }
        }
        check_keys(o, &["X", "Y"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut s = V2::default();
        s.load_value(v, l);
        Some(s)
    }
}

plain_struct!(MoveCtl { "decay" = decay, "friction" = friction, "max" = max, "min" = min, "speed" = speed });

impl Rec<MoveEndRecord> for MoveEnd {
    fn put(&self, x: &Wx) -> MoveEndRecord {
        MoveEndRecord { stop: self.stop, clamp: self.clamp, min: self.min.to_value(x), max: self.max.to_value(x) }
    }
    fn load(&mut self, r: &MoveEndRecord, l: &mut Lx) {
        self.stop = r.stop;
        self.clamp = r.clamp;
        l.at("min", |l| self.min.load_value(&r.min, l));
        l.at("max", |l| self.max.load_value(&r.max, l));
    }
    fn make(r: &MoveEndRecord, l: &mut Lx) -> Option<Self> {
        crate::conv::via_default(r, l, Self::load)
    }
}

impl Rec<MoveCmdRecord> for MoveCmd {
    fn put(&self, x: &Wx) -> MoveCmdRecord {
        let mut r = MoveCmdRecord {
            control: self.ctl.to_value(x),
            stop: self.end.stop,
            clamp: self.end.clamp,
            min: self.end.min.to_value(x),
            max: self.end.max.to_value(x),
            // Deviation: the oracle command's corner / gate flags are set at its creation (always defined); the Rust
            // command's are plain flags, written defined.
            corner: Some(self.corner),
            up_gate: Some(self.up_gate),
            side_gate: Some(self.side_gate),
            old_foot: Some(self.old_foot),
            ..MoveCmdRecord::default()
        };
        rec_rows!(put self, x, r;
            index: idx, total: total, current: cur, vx: vx, vy: vy, bx: bx, by: by, wall_skate: wall_skate,
        );
        r
    }
    fn load(&mut self, r: &MoveCmdRecord, l: &mut Lx) {
        rec_rows!(load self, r, l;
            index: idx, total: total, current: cur, vx: vx, vy: vy, bx: bx, by: by, wall_skate: wall_skate,
        );
        for (k, v, set) in [
            ("corner", r.corner, &mut self.corner),
            ("up_gate", r.up_gate, &mut self.up_gate),
            ("side_gate", r.side_gate, &mut self.side_gate),
            ("old_foot", r.old_foot, &mut self.old_foot),
        ] {
            match v {
                Some(b) => *set = b,
                None => l.at(k, |l| l.miss("undefined (the Rust flag is always defined)")),
            }
        }
        l.at("control", |l| self.ctl.load_value(&r.control, l));
        self.end.stop = r.stop;
        self.end.clamp = r.clamp;
        l.at("min", |l| self.end.min.load_value(&r.min, l));
        l.at("max", |l| self.end.max.load_value(&r.max, l));
    }
    fn make(r: &MoveCmdRecord, l: &mut Lx) -> Option<Self> {
        crate::conv::via_default(r, l, Self::load)
    }
}

// ------------------------------------------------------------------ buffs

pub(crate) fn buff_class(cls: &str, l: &mut Lx) -> Option<BuffClass> {
    // A class no module registers is the data's (BuffClass::Data), by its interned name.
    let c = BuffClass::ALL.iter().copied().find(|c| c.registered_name() == Some(cls));
    Some(c.unwrap_or_else(|| BuffClass::Data(l.name(cls))))
}

pub(crate) fn ext_put(e: &BuffExt, x: &Wx) -> Value {
    match e {
        BuffExt::None => Value::Null,
        BuffExt::DyingRage { bound, activated } => {
            obj(vec![("activated", Value::Bool(*activated)), ("bound", Value::Bool(*bound))])
        }
        // Unverified: the oracle ScoobySnack buff's extension keys; no trace holds one.
        BuffExt::ScoobySnack { by } => obj(vec![("by", by.map_or(Value::Null, |f| f.to_value(x)))]),
        BuffExt::Perk(p) => perk_put(p, x),
        // Unverified: the extension keys of the buff classes as the oracle's graphs set them; no trace holds one.
        BuffExt::JumpIncrease { life } => obj(vec![("life", life.to_value(x))]),
        BuffExt::MoveCompTo { mv } => obj(vec![("move", mv.as_ref().map_or(Value::Null, |m| m.to_value(x)))]),
        BuffExt::Thorns { per } => obj(vec![("per", per.to_value(x))]),
        BuffExt::Ice { old, frozen, ally_breaks } => obj(vec![
            ("allyBreaks", ally_breaks.to_value(x)),
            ("frozen", Value::Bool(*frozen)),
            ("old", old.to_value(x)),
        ]),
        BuffExt::FearStack { by } => obj(vec![("by", by.map_or(Value::Null, |f| f.to_value(x)))]),
        BuffExt::PieHeal { left } => obj(vec![("left", left.to_value(x))]),
        BuffExt::Occlusion { hidden, occ } => obj(vec![("hidden", Value::Bool(*hidden)), ("occ", Value::Bool(*occ))]),
        // (Buff_SpeedUp has no extension until DelayDestroy)
        BuffExt::SpeedUp { left: None } => Value::Null,
        BuffExt::SpeedUp { left: Some(left) } => obj(vec![("left", left.to_value(x))]),
        BuffExt::Evade { n, next, life } => {
            obj(vec![("life", life.to_value(x)), ("n", n.to_value(x)), ("next", next.to_value(x))])
        }
        BuffExt::PassiveApply { active } => obj(vec![("active", Value::Bool(*active))]),
        BuffExt::BatmanOnHit { active } => obj(vec![("active", Value::Bool(*active))]),
        BuffExt::FireSlam { wall } => obj(vec![("wall", wall.to_value(x))]),
        BuffExt::SniperPunch { bound } => obj(vec![("bound", Value::Bool(*bound))]),
        // (the buff actor's dodge entries live only inside the explosion's call)
        BuffExt::BasketballPass(e) => obj(vec![("from", e.from.map_or(Value::Null, |f| f.to_value(x)))]),
        BuffExt::AryaTrophy { delay, bound, team } => {
            obj(vec![("bound", Value::Bool(*bound)), ("delay", delay.to_value(x)), ("team", team.to_value(x))])
        }
        BuffExt::BackStab { made: false, .. } => Value::Null,
        BuffExt::BackStab { pop: None, .. } => obj(vec![]),
        // (`pop`: the victim's oracle id, a number)
        BuffExt::BackStab { pop: Some(v), .. } => obj(vec![("pop", Value::Num(f64::from(x.fid(*v))))]),
        BuffExt::TakeFace { done: false } => Value::Null,
        BuffExt::TakeFace { done: true } => obj(vec![("done", Value::Bool(true))]),
        BuffExt::FireStorm { bound } => obj(vec![("bound", Value::Bool(*bound))]),
        BuffExt::ChainVictim { at, by } => {
            obj(vec![("at", at.to_value(x)), ("by", by.map_or(Value::Null, |f| f.to_value(x)))])
        }
        BuffExt::EarthSmashVictim(e) => crate::black_adam::bolt::pillar_put(e, x),
        BuffExt::NullifyNextHit(e) => crate::wonder_woman::actors::nullify_put(e, x),
        BuffExt::FastFallSlam(e) => crate::wonder_woman::perks::slam_put(e, x),
        BuffExt::ProjectileBlock(e) => crate::wonder_woman::perks::block_put(e, x),
        BuffExt::ConfettiCount(e) => crate::harley::actors::count_put(e, x),
        BuffExt::ConfettiExplosion(e) => crate::harley::actors::explosion_put(e, x),
        BuffExt::Cook(e) => crate::morty::perks::cook_put(e, x),
        BuffExt::AllyBurst(e) => crate::iron_giant::actors::burst_put(e, x),
        BuffExt::AllyPilot(e) => crate::iron_giant::turret::pilot_put(e, x),
        BuffExt::DamageReflect(e) => crate::iron_giant::perks::reflect_put(e, x),
        BuffExt::HeldByPlasmaBall(e) => crate::iron_giant::plasma::ball_put(e, x),
        BuffExt::FastWeaponMode(e) => crate::iron_giant::perks::fast_put(e, x),
        BuffExt::ScorchedEarth(e) => crate::iron_giant::perks::scorch_put(e, x),
        BuffExt::StevenSharedShield(e) => crate::steven::shield::shared_shield_put(e, x),
        BuffExt::BubbleCount(e) => crate::steven::tether::count_put(e, x),
        BuffExt::StevenBubbleEnemy(e) => crate::steven::tether::bubble_enemy_put(e, x),
        BuffExt::GreenThumbBoost(e) => crate::steven::watermelon::green_thumb_put(e, x),
        BuffExt::ElectricGroove(e) => crate::garnet::groove::groove_put(e, x),
        BuffExt::StackingSinging(e) => crate::garnet::sing::singing_put(e, x),
        BuffExt::Chicken(e) => crate::taz::chicken::chicken_put(e, x),
        BuffExt::AttachToDogpile(e) => crate::taz::perks_cloud::attach_put(e, x),
        BuffExt::TeamFightCloud(e) => crate::taz::perks_cloud::cloud_put(e, x),
        BuffExt::Tasty(e) => crate::taz::tasty::tasty_put(e, x),
        BuffExt::ApplyTasty(e) => crate::taz::tasty::apply_tasty_put(e, x),
        BuffExt::TastyOnHit(e) => crate::taz::tasty::tasty_on_hit_put(e, x),
        BuffExt::Tunneling(e) => crate::bugs::tunnel::tunneling_put(e, x),
        BuffExt::TunnelTeleport(e) => crate::bugs::tunnel::teleport_put(e, x),
        BuffExt::Charm(e) => crate::bugs::charm::charm_put(e, x),
        BuffExt::C020Silenced(e) => crate::rick::silenced::silenced_put(e, x),
        BuffExt::Teleporting3(e) => crate::rick::teleport::teleporting_put(e, x),
        BuffExt::PortalBuff(e) => crate::rick::teleport::portal_buff_put(e, x),
        BuffExt::GemOnCharge(e) => crate::finn::perks::gem_on_charge_put(e, x),
        BuffExt::GoingOutOfBusiness(e) => crate::finn::perks::sale_put(e, x),
        BuffExt::ShopBoots(e) => crate::finn::shop::boots_put(e, x),
        BuffExt::ArmorBuns(e) => crate::finn::shop::buns_put(e, x),
        BuffExt::EvidenceCount(e) => crate::velma::evidence::count_put(e, x),
        BuffExt::Studied(e) => crate::velma::perks::studied_put(e, x),
        BuffExt::Arrested(e) => crate::velma::police::arrested_put(e, x),
        BuffExt::HealOverTimeBlast(e) => crate::velma::beam::blast_put(e, x),
        BuffExt::VelmaAttachToAlly(e) => crate::velma::capture::capture_put(e, x),
        BuffExt::SlowIntoStun(e) => crate::velma::gadgets::slow_put(e, x),
        BuffExt::DashProjectileBlock(e) => crate::stripe::dash::block_put(e, x),
        BuffExt::JakeWeaponMaster(e) => crate::jake::weapons::master_put(e, x),
        BuffExt::JakeSafetyNet(e) => crate::jake::perks::safety_net_put(e, x),
        BuffExt::AirSuperiority(e) => crate::jake::perks::air_put(e, x),
        BuffExt::Cheese(e) => crate::tom_and_jerry::cheese::cheese_put(e, x),
        BuffExt::C018HomingVictim(e) => crate::marvin::homing::victim_put(e, x),
        BuffExt::DynamicMove(e) => crate::moves::dynamic_move_put(e, x),
        BuffExt::MoveToActor2(e) => e.to_value(x),
        BuffExt::BowSlow(e) => crate::gizmo::bow::bow_slow_put(e, x),
        BuffExt::Backpack(e) => crate::gizmo::perks::backpack_put(e, x),
        BuffExt::DanceCount(e) => crate::gizmo::perks::dance_count_put(e, x),
        BuffExt::GizmoMusic(e) => crate::gizmo::music::music_put(e, x),
        BuffExt::C023ACarBuff(e) => crate::gizmo::car::car_buff_put(e, x),
        BuffExt::ItemCarBuff(e) => crate::items::car::car_buff_put(e, x),
        BuffExt::InstantKill(e) => crate::mutators::instant_kill_put(e, x),
        BuffExt::Chaos(e) => crate::mutators::chaos_put(e, x),
        BuffExt::GrowWithDamage(e) => crate::mutators::grow_put(e, x),
        BuffExt::TastyBlock(e) => crate::taz::tasty::tasty_block_put(e, x),
        BuffExt::Lick(e) => crate::taz::tasty::lick_put(e, x),
        BuffExt::Electric(e) => obj(vec![
            ("at", e.at.to_value(x)),
            ("by", e.by.map_or(Value::Null, |f| f.to_value(x))),
            ("now", Value::Bool(e.now)),
            ("team", e.team.to_value(x)),
        ]),
        // (no extension until a begin with an Instigator or the first hit; `from` and `closed` left out while unset)
        BuffExt::Gauntlet(g) if g.from.is_none() && !g.closed => Value::Null,
        BuffExt::Gauntlet(g) => {
            let mut o = Vec::new();
            if g.closed {
                o.push(("closed", Value::Bool(true)));
            }
            if let Some(f) = g.from {
                o.push(("from", f.to_value(x)));
            }
            obj(o)
        }
        // (Buff_C021_SetHomingTowardsTarget has no extension until its timer's first pass)
        BuffExt::SetHoming { done: false } => Value::Null,
        BuffExt::SetHoming { done: true } => obj(vec![("done", Value::Bool(true))]),
        BuffExt::DelayKnockUp(e) => {
            // `rev` (true | undefined) and `dodge` are left out while unset
            let mut o = vec![("at", e.at.to_value(x)), ("by", e.by.to_value(x)), ("stopped", Value::Bool(e.stopped))];
            if e.rev {
                o.push(("rev", Value::Bool(true)));
            }
            if let Some(d) = &e.dodge {
                o.push(("dodge", d.to_value(x)));
            }
            obj(o)
        }
    }
}

/// One key of a buff extension's object into its field.
fn ext_key<T: Plain>(o: &mvs_trace::Obj, k: &str, set: &mut T, l: &mut Lx) {
    match o.get(k) {
        Some(v) => l.at(k, |l| set.load_value(v, l)),
        None => l.miss(format_args!("key {k} absent")),
    }
}

/// A perk buff class's extension (`combat::perks::PerkExt`) as the oracle class's `ext` object.
// Unverified: no trace holds a perk class with an extension of its own (the six matches' perks have none).
fn perk_put(p: &PerkExt, x: &Wx) -> Value {
    let v = |p: &dyn Plain| p.to_value(x);
    match p {
        PerkExt::Memoriam { active, at } => obj(vec![("active", v(active)), ("at", v(at))]),
        PerkExt::ArmorDeath { phase, left, applied } => {
            obj(vec![("applied", v(applied)), ("left", v(left)), ("phase", v(phase))])
        }
        PerkExt::Dir { dir, left } => obj(vec![("dir", v(dir)), ("left", v(left))]),
        PerkExt::Block { stacks, timer, delay } => {
            obj(vec![("delay", v(delay)), ("stacks", v(stacks)), ("timer", v(timer))])
        }
        PerkExt::LastStand { by, activated } => obj(vec![("activated", v(activated)), ("by", v(by))]),
        PerkExt::JumpCount { on, more } => obj(vec![("more", v(more)), ("on", v(on))]),
        PerkExt::Collateral { by } => obj(by.iter().map(|f| ("by", f.to_value(x))).collect()),
        PerkExt::Tracker { dmg, buffer, left, by } => {
            let mut e = vec![("buffer", v(buffer)), ("dmg", v(dmg)), ("left", v(left))];
            e.extend(by.iter().map(|f| ("by", f.to_value(x))));
            obj(e)
        }
        PerkExt::Detonate { latch } => obj(vec![("latch", v(latch))]),
        PerkExt::Grounded { left, charged } => obj(vec![("charged", v(charged)), ("left", v(left))]),
    }
}

fn perk_load(p: &mut PerkExt, o: &Obj, l: &mut Lx) {
    let keys: &[&str] = match p {
        PerkExt::Memoriam { active, at } => {
            ext_key(o, "active", active, l);
            ext_key(o, "at", at, l);
            &["active", "at"]
        }
        PerkExt::ArmorDeath { phase, left, applied } => {
            ext_key(o, "applied", applied, l);
            ext_key(o, "left", left, l);
            ext_key(o, "phase", phase, l);
            &["applied", "left", "phase"]
        }
        PerkExt::Dir { dir, left } => {
            ext_key(o, "dir", dir, l);
            ext_key(o, "left", left, l);
            &["dir", "left"]
        }
        PerkExt::Block { stacks, timer, delay } => {
            ext_key(o, "delay", delay, l);
            ext_key(o, "stacks", stacks, l);
            ext_key(o, "timer", timer, l);
            &["delay", "stacks", "timer"]
        }
        PerkExt::LastStand { by, activated } => {
            ext_key(o, "activated", activated, l);
            ext_key(o, "by", by, l);
            &["activated", "by"]
        }
        PerkExt::JumpCount { on, more } => {
            ext_key(o, "more", more, l);
            ext_key(o, "on", on, l);
            &["more", "on"]
        }
        // `by` absent: the oracle's undefined.
        PerkExt::Collateral { by } => {
            match o.get("by") {
                Some(v) => l.at("by", |l| by.load_value(v, l)),
                None => *by = None,
            }
            &["by"]
        }
        PerkExt::Tracker { dmg, buffer, left, by } => {
            ext_key(o, "buffer", buffer, l);
            ext_key(o, "dmg", dmg, l);
            ext_key(o, "left", left, l);
            match o.get("by") {
                Some(v) => l.at("by", |l| by.load_value(v, l)),
                None => *by = None,
            }
            &["buffer", "by", "dmg", "left"]
        }
        PerkExt::Detonate { latch } => {
            ext_key(o, "latch", latch, l);
            &["latch"]
        }
        PerkExt::Grounded { left, charged } => {
            ext_key(o, "charged", charged, l);
            ext_key(o, "left", left, l);
            &["charged", "left"]
        }
    };
    check_keys(o, keys, l);
}

fn ext_load(e: &mut BuffExt, v: &Value, l: &mut Lx) {
    match (e, v) {
        (BuffExt::None, Value::Null) => {}
        (BuffExt::DyingRage { bound, activated }, Value::Obj(o)) => {
            for (k, set) in [("activated", activated), ("bound", bound)] {
                match o.get(k) {
                    Some(v) => l.at(k, |l| set.load_value(v, l)),
                    None => l.miss(format_args!("key {k} absent")),
                }
            }
            check_keys(o, &["activated", "bound"], l);
        }
        (BuffExt::ScoobySnack { by } | BuffExt::FearStack { by }, Value::Obj(o)) => match o.get("by") {
            Some(v) => l.at("by", |l| by.load_value(v, l)),
            None => l.miss("key by absent"),
        },
        (BuffExt::Perk(p), Value::Obj(o)) => perk_load(p, o, l),
        (BuffExt::JumpIncrease { life }, Value::Obj(o)) => ext_key(o, "life", life, l),
        (BuffExt::MoveCompTo { mv }, Value::Obj(o)) => ext_key(o, "move", mv, l),
        (BuffExt::Thorns { per }, Value::Obj(o)) => ext_key(o, "per", per, l),
        (BuffExt::PieHeal { left }, Value::Obj(o)) => ext_key(o, "left", left, l),
        (BuffExt::Occlusion { hidden, occ }, Value::Obj(o)) => {
            ext_key(o, "hidden", hidden, l);
            ext_key(o, "occ", occ, l);
            check_keys(o, &["hidden", "occ"], l);
        }
        (BuffExt::SpeedUp { left }, Value::Null) => *left = None,
        // (a Buff_DecrementEvadeCount ended in its begin, on a NotPlayerCharacter: no extension)
        (e @ BuffExt::Evade { .. }, Value::Null) => *e = BuffExt::None,
        (BuffExt::SpeedUp { left }, Value::Obj(o)) => {
            let mut v = 0.0;
            ext_key(o, "left", &mut v, l);
            *left = Some(v);
            check_keys(o, &["left"], l);
        }
        (BuffExt::Evade { n, next, life }, Value::Obj(o)) => {
            ext_key(o, "n", n, l);
            ext_key(o, "next", next, l);
            ext_key(o, "life", life, l);
            check_keys(o, &["life", "n", "next"], l);
        }
        (BuffExt::PassiveApply { active }, Value::Obj(o)) => {
            ext_key(o, "active", active, l);
            check_keys(o, &["active"], l);
        }
        (BuffExt::BatmanOnHit { active }, Value::Obj(o)) => {
            ext_key(o, "active", active, l);
            check_keys(o, &["active"], l);
        }
        (BuffExt::FireSlam { wall }, Value::Obj(o)) => {
            ext_key(o, "wall", wall, l);
            check_keys(o, &["wall"], l);
        }
        // (a Buff_Superman_SniperPunch has no extension until SetMultipliersServer)
        (e @ BuffExt::SniperPunch { .. }, Value::Null) => *e = BuffExt::None,
        (BuffExt::SniperPunch { bound }, Value::Obj(o)) => {
            ext_key(o, "bound", bound, l);
            check_keys(o, &["bound"], l);
        }
        (BuffExt::BasketballPass(e), Value::Obj(o)) => {
            ext_key(o, "from", &mut e.from, l);
            e.by = e.from;
            check_keys(o, &["from"], l);
        }
        (BuffExt::DelayKnockUp(e), Value::Obj(o)) => {
            ext_key(o, "at", &mut e.at, l);
            ext_key(o, "by", &mut e.by, l);
            ext_key(o, "stopped", &mut e.stopped, l);
            e.rev = matches!(o.get("rev"), Some(Value::Bool(true)));
            e.dodge = match o.get("dodge") {
                Some(v) => l.at("dodge", |l| Plain::from_value(v, l)),
                None => None,
            };
            check_keys(o, &["at", "by", "dodge", "rev", "stopped"], l);
        }
        (BuffExt::AryaTrophy { delay, bound, team }, Value::Obj(o)) => {
            ext_key(o, "delay", delay, l);
            ext_key(o, "bound", bound, l);
            ext_key(o, "team", team, l);
            check_keys(o, &["bound", "delay", "team"], l);
        }
        (BuffExt::BackStab { made, pop }, Value::Null) => {
            *made = false;
            *pop = None;
        }
        (BuffExt::BackStab { made, pop }, Value::Obj(o)) => {
            *made = true;
            *pop = match o.get("pop") {
                Some(Value::Num(n)) => l.fighter(*n),
                Some(_) => {
                    l.miss("a pop that is not a fighter id");
                    None
                }
                None => None,
            };
            check_keys(o, &["pop"], l);
        }
        (BuffExt::TakeFace { done }, Value::Null) => *done = false,
        // (a Buff_Creature_FireStorm has no extension until its begin)
        (e @ BuffExt::FireStorm { .. }, Value::Null) => *e = BuffExt::None,
        (BuffExt::FireStorm { bound }, Value::Obj(o)) => {
            ext_key(o, "bound", bound, l);
            check_keys(o, &["bound"], l);
        }
        // (a C021_Buff_ElectricChainVictim has no extension until its begin)
        (e @ BuffExt::ChainVictim { .. }, Value::Null) => *e = BuffExt::None,
        (BuffExt::ChainVictim { at, by }, Value::Obj(o)) => {
            ext_key(o, "at", at, l);
            *by = match o.get("by") {
                Some(v) => l.at("by", |l| Plain::from_value(v, l)),
                None => None,
            };
            check_keys(o, &["at", "by"], l);
        }
        (BuffExt::EarthSmashVictim(e), v) => crate::black_adam::bolt::pillar_load(e, v, l),
        (BuffExt::NullifyNextHit(e), v) => crate::wonder_woman::actors::nullify_load(e, v, l),
        (BuffExt::FastFallSlam(e), v) => crate::wonder_woman::perks::slam_load(e, v, l),
        (BuffExt::ProjectileBlock(e), v) => crate::wonder_woman::perks::block_load(e, v, l),
        (BuffExt::ConfettiCount(e), v) => crate::harley::actors::count_load(e, v, l),
        (BuffExt::ConfettiExplosion(e), v) => crate::harley::actors::explosion_load(e, v, l),
        (BuffExt::Cook(e), v) => crate::morty::perks::cook_load(e, v, l),
        (BuffExt::AllyBurst(e), v) => crate::iron_giant::actors::burst_load(e, v, l),
        (BuffExt::AllyPilot(e), v) => crate::iron_giant::turret::pilot_load(e, v, l),
        (BuffExt::DamageReflect(e), v) => crate::iron_giant::perks::reflect_load(e, v, l),
        (BuffExt::HeldByPlasmaBall(e), v) => crate::iron_giant::plasma::ball_load(e, v, l),
        (BuffExt::FastWeaponMode(e), v) => crate::iron_giant::perks::fast_load(e, v, l),
        (BuffExt::ScorchedEarth(e), v) => crate::iron_giant::perks::scorch_load(e, v, l),
        (BuffExt::StevenSharedShield(e), v) => crate::steven::shield::shared_shield_load(e, v, l),
        (BuffExt::BubbleCount(e), v) => crate::steven::tether::count_load(e, v, l),
        (BuffExt::StevenBubbleEnemy(e), v) => crate::steven::tether::bubble_enemy_load(e, v, l),
        (BuffExt::GreenThumbBoost(e), v) => crate::steven::watermelon::green_thumb_load(e, v, l),
        (BuffExt::ElectricGroove(e), v) => crate::garnet::groove::groove_load(e, v, l),
        (BuffExt::StackingSinging(e), v) => crate::garnet::sing::singing_load(e, v, l),
        (BuffExt::Chicken(e), v) => crate::taz::chicken::chicken_load(e, v, l),
        (BuffExt::AttachToDogpile(e), v) => crate::taz::perks_cloud::attach_load(e, v, l),
        (BuffExt::TeamFightCloud(e), v) => crate::taz::perks_cloud::cloud_load(e, v, l),
        (BuffExt::Tasty(e), v) => crate::taz::tasty::tasty_load(e, v, l),
        (BuffExt::ApplyTasty(e), v) => crate::taz::tasty::apply_tasty_load(e, v, l),
        (BuffExt::TastyOnHit(e), v) => crate::taz::tasty::tasty_on_hit_load(e, v, l),
        (BuffExt::Tunneling(e), v) => crate::bugs::tunnel::tunneling_load(e, v, l),
        (BuffExt::TunnelTeleport(e), v) => crate::bugs::tunnel::teleport_load(e, v, l),
        (BuffExt::Charm(e), v) => crate::bugs::charm::charm_load(e, v, l),
        (BuffExt::C020Silenced(e), v) => crate::rick::silenced::silenced_load(e, v, l),
        (BuffExt::Teleporting3(e), v) => crate::rick::teleport::teleporting_load(e, v, l),
        (BuffExt::PortalBuff(e), v) => crate::rick::teleport::portal_buff_load(e, v, l),
        (BuffExt::GemOnCharge(e), v) => crate::finn::perks::gem_on_charge_load(e, v, l),
        (BuffExt::GoingOutOfBusiness(e), v) => crate::finn::perks::sale_load(e, v, l),
        (BuffExt::ShopBoots(e), v) => crate::finn::shop::boots_load(e, v, l),
        (BuffExt::ArmorBuns(e), v) => crate::finn::shop::buns_load(e, v, l),
        (BuffExt::EvidenceCount(e), v) => crate::velma::evidence::count_load(e, v, l),
        (BuffExt::Studied(e), v) => crate::velma::perks::studied_load(e, v, l),
        (BuffExt::Arrested(e), v) => crate::velma::police::arrested_load(e, v, l),
        (BuffExt::HealOverTimeBlast(e), v) => crate::velma::beam::blast_load(e, v, l),
        (BuffExt::VelmaAttachToAlly(e), v) => crate::velma::capture::capture_load(e, v, l),
        (BuffExt::SlowIntoStun(e), v) => crate::velma::gadgets::slow_load(e, v, l),
        (BuffExt::DashProjectileBlock(e), v) => crate::stripe::dash::block_load(e, v, l),
        (BuffExt::JakeWeaponMaster(e), v) => crate::jake::weapons::master_load(e, v, l),
        // (a Buff_Jake_SafetyNet has no extension until the first ally is given the buff)
        (e @ BuffExt::JakeSafetyNet(_), Value::Null) => *e = BuffExt::None,
        (BuffExt::JakeSafetyNet(e), v) => crate::jake::perks::safety_net_load(e, v, l),
        (BuffExt::AirSuperiority(e), v) => crate::jake::perks::air_load(e, v, l),
        (BuffExt::Cheese(e), v) => crate::tom_and_jerry::cheese::cheese_load(e, v, l),
        (BuffExt::C018HomingVictim(e), v) => crate::marvin::homing::victim_load(e, v, l),
        // (a DynamicMove buff has no extension until its begin)
        (e @ BuffExt::DynamicMove(_), Value::Null) => *e = BuffExt::None,
        (BuffExt::DynamicMove(e), v) => crate::moves::dynamic_move_load(e, v, l),
        (BuffExt::MoveToActor2(e), v) => e.load_value(v, l),
        (BuffExt::BowSlow(e), v) => crate::gizmo::bow::bow_slow_load(e, v, l),
        (BuffExt::Backpack(e), v) => crate::gizmo::perks::backpack_load(e, v, l),
        (BuffExt::DanceCount(e), v) => crate::gizmo::perks::dance_count_load(e, v, l),
        (BuffExt::GizmoMusic(e), v) => crate::gizmo::music::music_load(e, v, l),
        (BuffExt::C023ACarBuff(e), v) => crate::gizmo::car::car_buff_load(e, v, l),
        (BuffExt::ItemCarBuff(e), v) => crate::items::car::car_buff_load(e, v, l),
        (BuffExt::InstantKill(e), v) => crate::mutators::instant_kill_load(e, v, l),
        // (Freeze Touch's has no extension until its first hit)
        (e @ BuffExt::Chaos(_), Value::Null) => *e = BuffExt::None,
        (BuffExt::Chaos(e), v) => crate::mutators::chaos_load(e, v, l),
        (BuffExt::GrowWithDamage(e), v) => crate::mutators::grow_load(e, v, l),
        (BuffExt::TastyBlock(e), v) => crate::taz::tasty::tasty_block_load(e, v, l),
        (BuffExt::Lick(e), v) => crate::taz::tasty::lick_load(e, v, l),
        // (a Buff_Damage_Electric or Buff_Low_Damage_Electric has no extension until its begin)
        (e @ BuffExt::Electric(_), Value::Null) => *e = BuffExt::None,
        (BuffExt::Electric(e), Value::Obj(o)) => {
            ext_key(o, "at", &mut e.at, l);
            ext_key(o, "now", &mut e.now, l);
            ext_key(o, "team", &mut e.team, l);
            e.by = match o.get("by") {
                Some(v) => l.at("by", |l| Plain::from_value(v, l)),
                None => None,
            };
            check_keys(o, &["at", "by", "now", "team"], l);
        }
        (BuffExt::Gauntlet(g), Value::Null) => *g = Default::default(),
        (BuffExt::Gauntlet(g), Value::Obj(o)) => {
            g.closed = matches!(o.get("closed"), Some(Value::Bool(true)));
            g.from = match o.get("from") {
                Some(v) => l.at("from", |l| Plain::from_value(v, l)),
                None => None,
            };
            check_keys(o, &["closed", "from"], l);
        }
        (BuffExt::SetHoming { done }, Value::Null) => *done = false,
        (BuffExt::SetHoming { done }, Value::Obj(o)) => {
            ext_key(o, "done", done, l);
            check_keys(o, &["done"], l);
        }
        (BuffExt::TakeFace { done }, Value::Obj(o)) => {
            ext_key(o, "done", done, l);
            check_keys(o, &["done"], l);
        }
        (BuffExt::Ice { old, frozen, ally_breaks }, Value::Obj(o)) => {
            ext_key(o, "old", old, l);
            ext_key(o, "frozen", frozen, l);
            ext_key(o, "allyBreaks", ally_breaks, l);
            check_keys(o, &["allyBreaks", "frozen", "old"], l);
        }
        (e, v) => l.miss(format_args!("{v:?} for a buff extension {e:?}")),
    }
}

/// A new buff's extension, from its class (the oracle buff classes' own fields).
fn ext_for(class: BuffClass) -> BuffExt {
    if let Some(p) = PerkExt::for_class(class) {
        return BuffExt::Perk(p);
    }
    match class.registered_name().unwrap_or_default() {
        "Buff_Shaggy_DyingRage_C" => BuffExt::DyingRage { bound: false, activated: false },
        "Buff_ScoobySnack_C" => BuffExt::ScoobySnack { by: None },
        "Buff_JumpIncrease_C" => BuffExt::JumpIncrease { life: 0.0 },
        "Buff_MoveCompTo_C" | "Buff_MoveCompTo_WithStun_C" => BuffExt::MoveCompTo { mv: None },
        "Buff_Thorns_C" => BuffExt::Thorns { per: 0.0 },
        "Buff_Ice_C" => BuffExt::Ice { old: -1.0, frozen: false, ally_breaks: 0.0 },
        "Buff_FearStack_C" => BuffExt::FearStack { by: None },
        "Buff_PieHeal_C" => BuffExt::PieHeal { left: 0.0 },
        "Buff_Invisibility_C" | "Buff_Invisibility_Fog_C" => BuffExt::Occlusion { hidden: false, occ: false },
        "Buff_SpeedUp_C" => BuffExt::SpeedUp { left: None },
        "Buff_DecrementEvadeCount_C" => BuffExt::Evade { n: 0.0, next: 0.0, life: 0.0 },
        "Buff_PassiveApplyBuffonHit_C" => BuffExt::PassiveApply { active: false },
        "Buff_BatmanOnHitApplyBuff_C" => BuffExt::BatmanOnHit { active: false },
        "Buff_DelayKnockUp_C" => BuffExt::DelayKnockUp(Default::default()),
        "Buff_Superman_FireSlam_C" => BuffExt::FireSlam { wall: -1 },
        "Buff_Superman_SniperPunch_C" => BuffExt::SniperPunch { bound: false },
        "Buff_BasketballPass_C" | "Buff_IgnitedBasketballPass_C" => BuffExt::BasketballPass(Default::default()),
        "Buff_Arya_Trophy_C" => BuffExt::AryaTrophy { delay: 0.0, bound: false, team: -1 },
        "Buff_AryaBackStab_C" | "Buff_BackStab_C" => BuffExt::BackStab { made: false, pop: None },
        "Buff_TakeFace_C" => BuffExt::TakeFace { done: false },
        "Buff_Creature_FireStorm_C" => BuffExt::FireStorm { bound: false },
        "C021_Buff_ElectricChainVictim_C" => BuffExt::ChainVictim { at: 0.0, by: None },
        "Buff_C021_EarthSmashVictim_C" => BuffExt::EarthSmashVictim(Default::default()),
        "Buff_C021_SetHomingTowardsTarget_C" => BuffExt::SetHoming { done: false },
        "Buff_Damage_Electric_C" | "Buff_Low_Damage_Electric_C" => BuffExt::Electric(Default::default()),
        "Buff_SupermanAllyIceGauntlet_C"
        | "Buff_AllyElectricGauntlets_C"
        | "Buff_FireGauntlets_C"
        | "Buff_GarnetElecInsideBubble_C"
        | "Buff_GarnetElecOutsideBubble_C"
        | "Buff_GarnetFullPowerGauntlets_C" => BuffExt::Gauntlet(Default::default()),
        "Buff_NullifyNextHit_C" => BuffExt::NullifyNextHit(Default::default()),
        "Buff_Perk_WonderWoman_FastFallSlam_C" => BuffExt::FastFallSlam(Default::default()),
        "Buff_Perk_WonderWoman_ProjectileBlock_C" => BuffExt::ProjectileBlock(Default::default()),
        "Buff_ConfettiCount_C" => BuffExt::ConfettiCount(Default::default()),
        "Buff_ConfettiExplosionCount_C" => BuffExt::ConfettiExplosion(Default::default()),
        "Buff_C019_Cook_C" => BuffExt::Cook(Default::default()),
        "Buff_C017_AllyBurst_C" => BuffExt::AllyBurst(Default::default()),
        "Buff_C017_Ally_Pilot_C" => BuffExt::AllyPilot(Default::default()),
        "C017_TempHealth_DamageReflect_Passive_C" => BuffExt::DamageReflect(Default::default()),
        "Buff_HeldByPlasmaBall_C" => BuffExt::HeldByPlasmaBall(Default::default()),
        "Buff_C017_FastWeaponMode_C" => BuffExt::FastWeaponMode(Default::default()),
        "Buff_C017_ScorchedEarth_C" => BuffExt::ScorchedEarth(Default::default()),
        "Buff_StevenSharedShield_C" => BuffExt::StevenSharedShield(Default::default()),
        "Buff_BubbleCount_C" => BuffExt::BubbleCount(Default::default()),
        "Buff_StevenBubbleEnemy_C" => BuffExt::StevenBubbleEnemy(Default::default()),
        "Buff_Steven_GreenThumbDamageBoost_C" => BuffExt::GreenThumbBoost(Default::default()),
        "Buff_Garnet_ElectricGroove_C" => BuffExt::ElectricGroove(Default::default()),
        "Buff_GarnetStackingSinging_C" => BuffExt::StackingSinging(Default::default()),
        "Buff_Chicken_C" => BuffExt::Chicken(Default::default()),
        "Buff_C015AttachToDogpile_C" => BuffExt::AttachToDogpile(Default::default()),
        "Buff_C015_TeamFightCloud_C" => BuffExt::TeamFightCloud(Default::default()),
        "Buff_Tasty_C" => BuffExt::Tasty(Default::default()),
        "Buff_ApplyTasty_C" => BuffExt::ApplyTasty(Default::default()),
        "Buff_TastyOnHit_C" => BuffExt::TastyOnHit(Default::default()),
        "Buff_TastyBlock_C" => BuffExt::TastyBlock(Default::default()),
        "Buff_c015_ApplybuffOnOverlap_C" => BuffExt::Lick(Default::default()),
        "Buff_Tunneling_C" => BuffExt::Tunneling(Default::default()),
        "Buff_TunnelTeleport_C" => BuffExt::TunnelTeleport(Default::default()),
        "Buff_Charm_C" => BuffExt::Charm(Default::default()),
        "C020_Buff_Silenced_C" => BuffExt::C020Silenced(Default::default()),
        "Buff_Teleporting3_C" => BuffExt::Teleporting3(Default::default()),
        "Buff_PortalBuff_C" => BuffExt::PortalBuff(Default::default()),
        "Buff_Finn_GemOnCharge_C" => BuffExt::GemOnCharge(Default::default()),
        "Buff_Finn_GoingOutOfBusiness_C" => BuffExt::GoingOutOfBusiness(Default::default()),
        "Finn_Shop_Boots_Buff_C" | "Finn_Shop_Boots_Buff_Ally_C" => BuffExt::ShopBoots(Default::default()),
        "Finn_Shop_ArmorBuns_Buff_C" => BuffExt::ArmorBuns(Default::default()),
        "Buff_EvidenceCount_C" => BuffExt::EvidenceCount(Default::default()),
        "Buff_Studied_C" => BuffExt::Studied(Default::default()),
        "Buff_Arrested_C" => BuffExt::Arrested(Default::default()),
        "Buff_VelmaHealOverTimeBlast_C" => BuffExt::HealOverTimeBlast(Default::default()),
        "Buff_VelmaAttachToAlly_C" => BuffExt::VelmaAttachToAlly(Default::default()),
        "Buff_SlowIntoStun_C" => BuffExt::SlowIntoStun(Default::default()),
        "C023B_DashProjectileBlock_C" => BuffExt::DashProjectileBlock(Default::default()),
        "Buff_Jake_WeaponMaster_C" => BuffExt::JakeWeaponMaster(Default::default()),
        "Buff_Jake_SafetyNet_C" => BuffExt::JakeSafetyNet(Default::default()),
        "Buff_AirSuperiority_C" => BuffExt::AirSuperiority(Default::default()),
        "Buff_Cheese_C" => BuffExt::Cheese(Default::default()),
        "Buff_C018HomingVictim_C" => BuffExt::C018HomingVictim(Default::default()),
        "Buff_DynamicMoveToActor_C"
        | "Buff_StunnedDynamicMoveToActor_C"
        | "Buff_C018ProjectileWarpMove_C"
        | "Buff_C018ProjectileWarpEnemy_C" => BuffExt::DynamicMove(Default::default()),
        "Buff_MoveToActor2_C" => BuffExt::MoveToActor2(Default::default()),
        "C023A_BowSlow_C" => BuffExt::BowSlow(Default::default()),
        "Buff_C023A_Backpack_C" => BuffExt::Backpack(Default::default()),
        "C023A_DanceCount_Debuff_C" => BuffExt::DanceCount(Default::default()),
        "C023A_Music_Buff_C" | "C023A_Music_DeBuff_C" => BuffExt::GizmoMusic(Default::default()),
        "C023A_Car_Buff_C" => BuffExt::C023ACarBuff(Default::default()),
        "Car_Buff_C" => BuffExt::ItemCarBuff(Default::default()),
        "Mutator_InstantKill_C" => BuffExt::InstantKill(Default::default()),
        "Mutator_GreyHealth_C" | "Mutator_Boost_IceGauntlets_C" => BuffExt::Chaos(Default::default()),
        "Mutator_GrowWithDamage_C" => BuffExt::GrowWithDamage(Default::default()),
        _ => BuffExt::None,
    }
}

/// A data value (`mvs_data`) as the trace writes it.
pub(crate) fn data_put(v: &DataValue) -> Value {
    match v {
        DataValue::Null => Value::Null,
        DataValue::Bool(b) => Value::Bool(*b),
        DataValue::Num(n) => Value::Num(*n),
        DataValue::Str(s) => Value::Str(s.clone()),
        DataValue::Arr(a) => Value::List(a.iter().map(data_put).collect()),
        DataValue::Obj(o) => Value::Obj(mvs_trace::Obj::new(o.iter().map(|(k, v)| (k.clone(), data_put(v))).collect())),
        DataValue::F32s(a) => Value::List(a.iter().map(|x| Value::Num(f64::from(*x))).collect()),
    }
}

/// The most leaves a static object of the data is written whole with.
const PLAIN_LEAVES: i32 = 64;

/// The oracle's `plainStatic`: a static value of plain leaves alone, at most [`PLAIN_LEAVES`] of them and no
/// object or list below depth 3; else `None`. A list of float32s is a typed array there, no plain object.
fn plain_static(v: &DataValue, budget: &mut i32, depth: u32) -> Option<Value> {
    match v {
        DataValue::Null | DataValue::Bool(_) | DataValue::Num(_) | DataValue::Str(_) => {
            *budget -= 1;
            (*budget >= 0).then(|| data_put(v))
        }
        _ if depth > 3 => None,
        DataValue::Arr(a) => {
            a.iter().map(|x| plain_static(x, budget, depth + 1)).collect::<Option<Vec<_>>>().map(Value::List)
        }
        DataValue::Obj(o) => o
            .iter()
            .map(|(k, x)| plain_static(x, budget, depth + 1).map(|v| (k.clone(), v)))
            .collect::<Option<Vec<_>>>()
            .map(|e| Value::Obj(mvs_trace::Obj::new(e))),
        DataValue::F32s(_) => None,
    }
}

/// The oracle's `staticOf` of an object of the data that is no notify: `{static: name}` by its `name`, `cls` or
/// `key` string, else its value where it is small ([`plain_static`]), else `{static: "object"}`.
pub(crate) fn static_put(p: &Props) -> Value {
    for k in ["name", "cls", "key"] {
        if let Some(DataValue::Str(s)) = p.get(k) {
            return obj(vec![("static", Value::Str(s.clone()))]);
        }
    }
    let whole = DataValue::Obj(p.clone());
    let mut budget = PLAIN_LEAVES;
    plain_static(&whole, &mut budget, 0).unwrap_or_else(|| obj(vec![("static", Value::Str("object".to_owned()))]))
}

/// Two values the same to the bit (a number's NaN equal to itself).
pub(crate) fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => x.to_bits() == y.to_bits(),
        (Value::List(x), Value::List(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_value(x, y)),
        (Value::Obj(x), Value::Obj(y)) => {
            x.len() == y.len()
                && x.entries().iter().zip(y.entries()).all(|((kx, x), (ky, y))| kx == ky && same_value(x, y))
        }
        _ => a == b,
    }
}

/// A trace value as a data value (an object's keys in the trace's sorted order).
pub(crate) fn data_load(v: &Value) -> DataValue {
    match v {
        Value::Null => DataValue::Null,
        Value::Bool(b) => DataValue::Bool(*b),
        Value::Num(n) => DataValue::Num(*n),
        Value::Str(s) => DataValue::Str(s.clone()),
        Value::List(a) => DataValue::Arr(a.iter().map(data_load).collect()),
        Value::Obj(o) => DataValue::Obj(o.entries().iter().map(|(k, v)| (k.clone(), data_load(v))).collect()),
    }
}

// Unverified: the oracle buff's `own` (its own variables, any data value) as an object; the traces hold none. A loaded
// one has its keys in the trace's order, not the order of the writes.
pub(crate) fn own_put(o: &Option<BuffOwn>, _x: &Wx) -> Value {
    o.as_ref().map_or(Value::Null, |o| {
        Value::Obj(mvs_trace::Obj::new(o.values.iter().map(|(k, v)| (k.clone(), data_put(v))).collect()))
    })
}

fn own_load(o: &mut Option<BuffOwn>, v: &Value, l: &mut Lx) {
    match v {
        Value::Null => *o = None,
        Value::Obj(ob) => {
            let values: Props = ob.entries().iter().map(|(k, v)| (k.clone(), data_load(v))).collect();
            *o = Some(BuffOwn { values });
        }
        _ => l.miss(format_args!("{v:?} is not a buff's own values")),
    }
}

plain_struct!(ComponentMove {
    "easeIn" = ease_in, "easeOut" = ease_out, "elapsed" = elapsed, "interpolating" = interpolating, "ix" = ix,
    "iy" = iy, "total" = total, "tx" = tx, "ty" = ty, "x" = x, "y" = y,
});

/// A `holdHits` entry: `{ vic, p }`.
// Unverified: the oracle entry's keys as the notifies push them; the traces hold none set.
impl Plain for HoldHit {
    fn to_value(&self, x: &Wx) -> Value {
        let p = Value::Obj(mvs_trace::Obj::new(self.p.iter().map(|(k, v)| (k.clone(), data_put(v))).collect()));
        obj(vec![("p", p), ("vic", self.vic.to_value(x))])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(h) = Self::from_value(v, l) {
            *self = h;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        let vic = l.at("vic", |l| o.get("vic").and_then(|v| FighterId::from_value(v, l)))?;
        let p = match o.get("p") {
            Some(Value::Obj(p)) => p.entries().iter().map(|(k, v)| (k.clone(), data_load(v))).collect(),
            _ => {
                l.miss("a hold hit without its packet");
                return None;
            }
        };
        check_keys(o, &["p", "vic"], l);
        Some(HoldHit { vic, p })
    }
}

/// A buff (`FighterBuff`). A new one's serial is `u32::MAX` until the loader numbers it from `next_serial`.
impl Rec<BuffRecord> for FighterBuff {
    fn put(&self, x: &Wx) -> BuffRecord {
        let mut r = BuffRecord {
            class: x.str(self.cls).to_owned(),
            ext: ext_put(&self.ext, x),
            own: own_put(&self.own, x),
            ..BuffRecord::default()
        };
        rec_rows!(put self, x, r;
            frames: frames, stacks: stacks, stack_frames: stack_frames, heal: heal, heal_amt: heal_amt, born: born,
            rescue: rescue, by: by, pulled: pulled, notify: notify, fire: fire, fire_dmg: fire_dmg, src: src,
            stun: stun, stun_by: stun_by, mods_off: mods_off, gate: gate, add_age: add_age, ally_hit: ally_hit,
            swept: swept,
        );
        r
    }
    fn load(&mut self, r: &BuffRecord, l: &mut Lx) {
        if l.str(self.cls) != r.class
            && let Some(c) = buff_class(&r.class, l)
        {
            self.cls = l.name(&r.class);
            self.class = c;
            self.ext = ext_for(c);
        }
        rec_rows!(load self, r, l;
            frames: frames, stacks: stacks, stack_frames: stack_frames, heal: heal, heal_amt: heal_amt, born: born,
            rescue: rescue, by: by, pulled: pulled, notify: notify, fire: fire, fire_dmg: fire_dmg, src: src,
            stun: stun, stun_by: stun_by, mods_off: mods_off, gate: gate, add_age: add_age, ally_hit: ally_hit,
            swept: swept,
        );
        // (an entry the sim made with no extension gains it later: Buff_Superman_SniperPunch at SetMultipliersServer,
        // Buff_AryaBackStab at its first back stab; the entry loaded as it was before)
        if matches!(self.ext, BuffExt::None) && !matches!(r.ext, Value::Null) {
            self.ext = ext_for(self.class);
        }
        l.at("ext", |l| ext_load(&mut self.ext, &r.ext, l));
        l.at("own", |l| own_load(&mut self.own, &r.own, l));
    }
    fn make(r: &BuffRecord, l: &mut Lx) -> Option<Self> {
        let class = buff_class(&r.class, l)?;
        let mut b = FighterBuff {
            serial: u32::MAX,
            cls: l.name(&r.class),
            class,
            frames: 0.0,
            stacks: 0.0,
            stack_frames: 0.0,
            heal: None,
            heal_amt: None,
            born: None,
            rescue: None,
            by: None,
            pulled: None,
            notify: None,
            fire: None,
            fire_dmg: None,
            src: None,
            ext: ext_for(class),
            stun: false,
            stun_by: None,
            mods_off: None,
            own: None,
            gate: None,
            add_age: None,
            ally_hit: false,
            swept: false,
        };
        b.load(r, l);
        Some(b)
    }
}

// ------------------------------------------------------------------ the input buffer

impl Rec<QueuedRecord> for QueuedAction {
    fn put(&self, x: &Wx) -> QueuedRecord {
        if self.older.is_some() {
            x.unmapped("a queued action's older presses (no rule to write them)");
        }
        QueuedRecord {
            press: self.press.put(x),
            dir: self.dir.put(x),
            force: self.force.map(|d| ForcedRecord { dir: f64::from(d.dir) }),
            taunt: self.taunt.as_deref().map_or(Value::Null, branch_value),
            raw: self.raw.put(x),
            windows: Some(self.windows.put(x)),
            older: Value::Null,
        }
    }
    fn load(&mut self, r: &QueuedRecord, l: &mut Lx) {
        l.at("press", |l| self.press.load(&r.press, l));
        l.at("dir", |l| self.dir.load(&r.dir, l));
        l.at("raw", |l| self.raw.load(&r.raw, l));
        l.at("force", |l| {
            self.force = match &r.force {
                None => None,
                Some(f) => int(f.dir, l).map(|dir| ForcedPress { dir }),
            }
        });
        l.at("taunt", |l| match &r.taunt {
            Value::Null => self.taunt = None,
            v => self.taunt = branch_load(v, l).map(Box::new),
        });
        l.at("windows", |l| match &r.windows {
            Some(w) => self.windows.load(w, l),
            None => l.miss("undefined (the Rust windows are always defined)"),
        });
        if r.older != Value::Null {
            l.at("older", |l| l.miss("older presses (no rule to load them)"));
        }
    }
    fn make(r: &QueuedRecord, l: &mut Lx) -> Option<Self> {
        let mut q = QueuedAction {
            press: QueuedBtn::make(&r.press, l)?,
            dir: Dir::default(),
            force: None,
            taunt: None,
            raw: Dir::default(),
            windows: Vec::new(),
            older: None,
        };
        q.load(r, l);
        Some(q)
    }
}

impl Rec<LateBranchRecord> for LateBranch {
    fn put(&self, x: &Wx) -> LateBranchRecord {
        let queued = self.q.as_ref().map(|q| q.put(x));
        LateBranchRecord { current: self.cur.put(x), queued, windows: self.ws.put(x) }
    }
    fn load(&mut self, r: &LateBranchRecord, l: &mut Lx) {
        l.at("current", |l| self.cur.load(&r.current, l));
        l.at("queued", |l| match (&mut self.q, &r.queued) {
            (Some(q), Some(r)) => q.load(r, l),
            (q, Some(r)) => *q = QueuedAction::make(r, l),
            (q, None) => *q = None,
        });
        l.at("windows", |l| self.ws.load(&r.windows, l));
    }
    fn make(r: &LateBranchRecord, l: &mut Lx) -> Option<Self> {
        let mut b = LateBranch { ws: Vec::new(), q: None, cur: Name::NONE };
        b.load(r, l);
        Some(b)
    }
}

// ------------------------------------------------------------------ the input

fn order_put(o: &Order, x: &Wx) -> Vec<String> {
    o.iter().map(|b| b.put(x)).collect()
}

fn order_load(r: &[String], l: &mut Lx) -> Option<Order> {
    let mut o = Order::default();
    if r.len() > o.btns.len() {
        l.miss(format_args!("an order of {} presses", r.len()));
        return None;
    }
    for (i, b) in r.iter().enumerate() {
        o.btns[i] = Some(Btn::make(b, l)?);
    }
    o.len = u8::try_from(r.len()).expect("checked");
    Some(o)
}

// An optional flag of the oracle InputState (`jumpLocal`, `item`, `downBtn`, `force`, ...) is an `Option<bool>`: `None`
// is the oracle undefined.
impl Rec<InputRecord> for InputState {
    fn put(&self, x: &Wx) -> InputRecord {
        let mut r = InputRecord {
            order: self.order.as_ref().map(|o| order_put(o, x)),
            atk_dir: self.atk_dir.map(|d| d.put(x)),
            force_press: self.force_press.map(|b| b.put(x)),
            force_release: self.force_release.map(|b| b.put(x)),
            ..InputRecord::default()
        };
        rec_rows!(put self, x, r;
            x: x, y: y, attack: attack, special: special, jump: jump, dodge: dodge, jump_local: jump_local,
            item: item, down_btn: down_btn, force_dir: force_dir, force: force, neutral_evade: neutral_evade,
            taunt: taunt, taunt_held: taunt_held, rsx: rsx, axis: axis, call_x: call_x, call_y: call_y,
        );
        r
    }
    fn load(&mut self, r: &InputRecord, l: &mut Lx) {
        rec_rows!(load self, r, l;
            x: x, y: y, attack: attack, special: special, jump: jump, dodge: dodge, jump_local: jump_local,
            item: item, down_btn: down_btn, force_dir: force_dir, force: force, neutral_evade: neutral_evade,
            taunt: taunt, taunt_held: taunt_held, rsx: rsx, axis: axis, call_x: call_x, call_y: call_y,
        );
        l.at("order", |l| self.order = r.order.as_ref().and_then(|o| order_load(o, l)));
        l.at("atk_dir", |l| self.atk_dir.load(&r.atk_dir, l));
        l.at("force_press", |l| self.force_press.load(&r.force_press, l));
        l.at("force_release", |l| self.force_release.load(&r.force_release, l));
    }
    fn make(r: &InputRecord, l: &mut Lx) -> Option<Self> {
        crate::conv::via_default(r, l, Self::load)
    }
}

/// A fighter's own input (`inp`, `prev`): the canonical input (`canon_input`) the fighter took, every optional
/// press of the oracle copy set as it is.
pub fn fighter_input(i: &InputState, x: &Wx) -> InputRecord {
    i.put(x)
}

// ------------------------------------------------------------------ rest values: the small structs

plain_struct!(Ally {
    "buff" = buff, "cast" = cast, "launch" = launch, "moveId" = move_id, "noAlly" = no_ally, "reach" = reach,
    "reached" = reached, "shield" = shield, "vfx" = vfx, "vfxBorn" = vfx_born, "safe" ~ safe,
});
plain_struct!(AllySafe { "speed" = speed, "x" = x, "y" = y });
plain_struct!(ChargeRamp { "at" = at, "n" = n, "pending" = pending, "span" = span });
plain_struct!(GroundJump { "ff" = ff, "v" = v });
plain_struct!(mvs_sim::combat::hit::HitPauseTimer { "sec" = sec, "tick" = tick });
/// A HoldingItemOverride entry: a montage the data leaves unset is null in the oracle (the port's `Name::NONE`).
impl Plain for HoldOverride {
    fn to_value(&self, x: &Wx) -> Value {
        let montage = if self.montage == Name::NONE { Value::Null } else { self.montage.to_value(x) };
        obj(vec![("air", Value::Bool(self.air)), ("dir", self.dir.to_value(x)), ("montage", montage)])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        if let Some(a) = o.get("air") {
            l.at("air", |l| self.air.load_value(a, l));
        }
        if let Some(d) = o.get("dir") {
            l.at("dir", |l| self.dir.load_value(d, l));
        }
        match o.get("montage") {
            Some(Value::Null) => self.montage = Name::NONE,
            Some(m) => l.at("montage", |l| self.montage.load_value(m, l)),
            None => l.miss(format_args!("key montage absent")),
        }
        check_keys(o, &["air", "dir", "montage"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut h = HoldOverride { air: false, dir: 0, montage: Name::NONE };
        h.load_value(v, l);
        Some(h)
    }
}
plain_struct!(MontageMesh { "age" = age, "anim" = anim, "loop" = looping, "mesh" = mesh, "socket" = socket });
plain_struct!(SpamDelayed { "at" = at, "b" = b, "d" = d, "force" ~ force, "u" ~ u } make |v, l| {
    let o = as_obj(v, l)?;
    let mut s = SpamDelayed {
        b: Btn::from_value(o.get("b")?, l)?,
        d: Dir::from_value(o.get("d")?, l)?,
        at: 0.0,
        force: None,
        u: None,
    };
    s.load_value(v, l);
    Some(s)
});
plain_struct!(ProjAim { "at" = at, "reset" = reset, "t" = t, "y" = y, "z" = z });
plain_struct!(SmBlend { "alpha" = alpha, "dur" = dur, "el" = el, "hermite" = hermite, "next" = next, "prev" = prev });
plain_struct!(Impulse { "type" = kind, "vx" = vx, "vy" = vy, "xy" = xy, "z" = z, "d" ~ d });
plain_struct!(mvs_sim::fighter::tick::SurfaceTrace { "ev" = ev, "sock" = sock, "queued" = queued });

/// The evade's dodge tracking (`dodge`): `fighters` a Map by fighter id.
impl Plain for DodgeState {
    fn to_value(&self, x: &Wx) -> Value {
        obj(vec![
            ("fighters", self.fighters.to_value(x)),
            ("fxGate", Value::Num(f64::from(self.fx_gate))),
            ("gate", Value::Num(f64::from(self.gate))),
            ("questGate", Value::Num(f64::from(self.quest_gate))),
        ])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        match o.get("fighters") {
            Some(v) => l.at("fighters", |l| self.fighters.load_value(v, l)),
            None => l.miss("key fighters absent"),
        }
        for (k, set) in [("fxGate", &mut self.fx_gate), ("gate", &mut self.gate), ("questGate", &mut self.quest_gate)] {
            match o.get(k) {
                Some(v) => l.at(k, |l| set.load_value(v, l)),
                None => l.miss(format_args!("key {k} absent")),
            }
        }
        check_keys(o, &["fighters", "fxGate", "gate", "questGate"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        crate::conv::via_default(v, l, Self::load_value)
    }
}

// `Fighter.folds`: only the values off their base are keys.
crate::conv::plain_struct!(mvs_sim::fighter::moves::Folds {
    "maxJumps" ~ max_jumps, "jumpH" ~ jump_h, "djumpH" ~ djump_h, "wjumpH" ~ wjump_h, "jumpVel" ~ jump_vel,
    "jumpStop" ~ jump_stop, "ffSpeed" ~ ff_speed, "evadeRate" ~ evade_rate, "evadeInvuln" ~ evade_invuln,
    "evadeRecharge" ~ evade_recharge, "bounce" ~ bounce, "airCtl" ~ air_ctl, "di" ~ di, "fallAccel" ~ fall_accel,
    "defaultSpeed" ~ default_speed, "boostThr" ~ boost_thr,
});

plain_struct!(mvs_sim::fighter::control::DodgeEntry { "id" = id, "seen" = seen, "at" = at });
plain_struct!(mvs_sim::fighter::control::AbilityOver {
    "Cooldown" = row.cooldown, "Icon" ~ row.icon, "MaxCount" = row.max_count, "StartCount" = row.start_count,
    "CountCooldown" = row.count_cooldown, "CountSpentPerTrigger" = row.count_spent_per_trigger,
    "CountRefreshedPerCooldown" = row.count_refreshed_per_cooldown, "CooldownAlwaysVisible" = row.cooldown_always_visible,
    "AllyCooldownVisible" = row.ally_cooldown_visible, "FoeCooldownVisible" = row.foe_cooldown_visible,
    "AlternateCooldowns" ~ alternate_cooldowns, "StaticAbilityCooldown" = row.static_ability_cooldown,
    "StaticAmmoCooldown" = row.static_ammo_cooldown, "RefreshCooldownOnRespawn" = row.refresh_cooldown_on_respawn,
});

/// A branch row as the oracle object holds it (a queued taunt's: Attack, Button, Direction, Conditions, Priority,
/// AllowPreAttackRotation; AbilitySlot, DisplayName and UniqueID only where set).
fn branch_value(b: &Branch) -> Value {
    let conds =
        Value::List(b.conditions.iter().map(|c| c.as_ref().map_or(Value::Null, |s| Value::Str(s.clone()))).collect());
    let mut e = vec![
        ("Attack", Value::Str(b.attack.clone())),
        ("Button", Value::Str(b.button.clone())),
        ("Direction", Value::Str(b.direction.clone())),
        ("Conditions", conds),
        ("Priority", Value::Num(f64::from(b.priority))),
        ("AllowPreAttackRotation", Value::Bool(b.allow_pre_attack_rotation)),
    ];
    if let Some(a) = b.ability_slot {
        e.push(("AbilitySlot", Value::Num(f64::from(a))));
    }
    if let Some(d) = &b.display_name {
        e.push(("DisplayName", Value::Str(d.clone())));
    }
    if !b.unique_id.is_empty() {
        e.push(("UniqueID", Value::Str(b.unique_id.clone())));
    }
    obj(e)
}

fn branch_load(v: &Value, l: &mut Lx) -> Option<Branch> {
    let o = as_obj(v, l)?;
    let s = |k: &str| match o.get(k) {
        Some(Value::Str(s)) => s.clone(),
        _ => String::new(),
    };
    let conditions = match o.get("Conditions") {
        Some(Value::List(c)) => c
            .iter()
            .map(|c| match c {
                Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let num = |k: &str, l: &mut Lx| match o.get(k) {
        Some(Value::Num(n)) => int(*n, l),
        _ => None,
    };
    let priority = num("Priority", l).unwrap_or(0);
    let ability_slot = num("AbilitySlot", l);
    Some(Branch {
        attack: s("Attack"),
        button: s("Button"),
        direction: s("Direction"),
        conditions,
        priority,
        ability_slot,
        allow_pre_attack_rotation: matches!(o.get("AllowPreAttackRotation"), Some(Value::Bool(true))),
        display_name: match o.get("DisplayName") {
            Some(Value::Str(d)) => Some(d.clone()),
            _ => None,
        },
        unique_id: s("UniqueID"),
    })
}

/// `animKey`: the oracle's string, `''`, `'m' + anim` or `'g' + locoGen`.
impl Rec<String> for AnimKey {
    fn put(&self, x: &Wx) -> String {
        match *self {
            AnimKey::None => String::new(),
            AnimKey::Montage(a) => format!("m{}", x.str(a)),
            AnimKey::Gen(g) => format!("g{g}"),
        }
    }
    fn load(&mut self, r: &String, l: &mut Lx) {
        if let Some(k) = Self::make(r, l) {
            *self = k;
        }
    }
    fn make(r: &String, l: &mut Lx) -> Option<Self> {
        if r.is_empty() {
            return Some(AnimKey::None);
        }
        if let Some(a) = r.strip_prefix('m') {
            return Some(AnimKey::Montage(l.name(a)));
        }
        match r.strip_prefix('g').and_then(|g| g.parse().ok()) {
            Some(g) => Some(AnimKey::Gen(g)),
            None => {
                l.miss(format!("animKey {r}"));
                None
            }
        }
    }
}

// ------------------------------------------------------------------ the shared parts of the actors' records

// A spawned actor's `anim` (its own keys; `side` only while set).
plain_struct!(ActorAnim {
    "mesh" = mesh, "m" = m, "proxy" = proxy, "ended" = ended, "owed" = owed, "side" ~ side,
});

/// The `anim` of an actor whose class holds an [`ActorAnim`], as the record writes it.
#[allow(
    dead_code,
    reason = "the hosts' puts and loads (the Meeseeks, Neptr, the gem, the dropped bomb) call it as their ports land"
)]
pub fn anim_put(a: &ActorAnim, x: &Wx) -> Value {
    a.to_value(x)
}

/// [`anim_put`]'s inverse.
#[allow(
    dead_code,
    reason = "the hosts' puts and loads (the Meeseeks, Neptr, the gem, the dropped bomb) call it as their ports land"
)]
pub fn anim_load(a: &mut ActorAnim, v: &Value, l: &mut Lx) {
    a.load_value(v, l);
}

/// The keys `BouncingItem` adds to its item's `rest` (a bouncing item class's put calls it).
#[allow(
    dead_code,
    reason = "the hosts' puts and loads (the Meeseeks, Neptr, the gem, the dropped bomb) call it as their ports land"
)]
pub fn bounce_put(b: &Bounce, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put b, x, o;
        "prevHitT" = prev_hit_t, "prevNx" = prev_nx, "prevNy" = prev_ny, "sliding" = sliding, "startPen" = start_pen,
        "bounciness" = bounciness, "friction" = friction,
    );
}

/// [`bounce_put`]'s inverse; the keys it knows go into `known`.
#[allow(
    dead_code,
    reason = "the hosts' puts and loads (the Meeseeks, Neptr, the gem, the dropped bomb) call it as their ports land"
)]
pub fn bounce_load(b: &mut Bounce, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load b, ob, l;
        "prevHitT" = prev_hit_t, "prevNx" = prev_nx, "prevNy" = prev_ny, "sliding" = sliding, "startPen" = start_pen,
        "bounciness" = bounciness, "friction" = friction,
    );
    known.extend(plain_keys!(names
        "prevHitT" = prev_hit_t, "prevNx" = prev_nx, "prevNy" = prev_ny, "sliding" = sliding, "startPen" = start_pen,
        "bounciness" = bounciness, "friction" = friction,
    ));
}
