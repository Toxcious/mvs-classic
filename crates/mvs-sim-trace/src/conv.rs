//! The two conversions between a World's values and the record: [`Rec`] for a typed row (a value to its record
//! field and back), [`Plain`] for a `rest` value (a value to the oracle writer's plain value: its
//! `stateOf`). Each loads in place: what the record does not name stays as it was.
//!
//! The struct macros list a struct's mapping once, the writer and the loader both expanded from it.

use std::hash::Hash;

use indexmap::{IndexMap, IndexSet};
use mvs_sim::Real;
use mvs_sim::actors::ActorRef;
use mvs_sim::events::NotifyRef;
use mvs_sim::fighter::FighterId;
use mvs_sim::names::Name;
use mvs_trace::{Obj, Value};

use crate::cx::{Lx, NotifyId, Wx};

/// A value written as a typed record field `R`.
pub trait Rec<R> {
    fn put(&self, x: &Wx) -> R;
    /// Writes `r` into the value in place.
    fn load(&mut self, r: &R, l: &mut Lx);
    /// A new value of `r` (a list's or an option's element the World does not have yet); `None`, named, where the
    /// value cannot be made from the record alone.
    fn make(r: &R, l: &mut Lx) -> Option<Self>
    where
        Self: Sized;
}

/// A value written as a plain value of a `rest` bag (or a typed row of kind VALUE).
pub trait Plain {
    fn to_value(&self, x: &Wx) -> Value;
    fn load_value(&mut self, v: &Value, l: &mut Lx);
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self>
    where
        Self: Sized;
}

/// A value the oracle leaves undefined (its key absent) when unset: an option's `None`, a flag's `false`.
pub trait Unset {
    fn unset(&self) -> bool;
    fn clear(&mut self);
}

impl<T> Unset for Option<T> {
    fn unset(&self) -> bool {
        self.is_none()
    }
    fn clear(&mut self) {
        *self = None;
    }
}

impl<T> Unset for Vec<T> {
    fn unset(&self) -> bool {
        self.is_empty()
    }
    fn clear(&mut self) {
        Vec::clear(self);
    }
}

impl Unset for bool {
    fn unset(&self) -> bool {
        !*self
    }
    fn clear(&mut self) {
        *self = false;
    }
}

/// `make` of a type with a `Default`: the default, then the record loaded into it.
pub fn via_default<T: Default, R>(r: &R, l: &mut Lx, load: impl FnOnce(&mut T, &R, &mut Lx)) -> Option<T> {
    let mut t = T::default();
    load(&mut t, r, l);
    Some(t)
}

/// A whole number of the record as an `i32`, else named and `None`.
pub fn int(v: f64, l: &mut Lx) -> Option<i32> {
    if v == 0.0 && v.is_sign_negative() {
        // (an i32 has no -0: the oracle wrote a float here)
        l.miss("-0 is not an i32");
        None
    } else if v.fract() == 0.0 && v >= f64::from(i32::MIN) && v <= f64::from(i32::MAX) {
        #[allow(clippy::cast_possible_truncation)] // whole and in range, checked above
        Some(v as i32)
    } else {
        l.miss(format_args!("{v} is not an i32"));
        None
    }
}

pub(crate) fn uint(v: f64, max: u32, l: &mut Lx) -> Option<u32> {
    if v.fract() == 0.0 && v >= 0.0 && v <= f64::from(max) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // whole and in range, checked above
        Some(v as u32)
    } else {
        l.miss(format_args!("{v} is not a u32 up to {max}"));
        None
    }
}

// ------------------------------------------------------------------ typed rows: scalars

impl Rec<f64> for Real {
    fn put(&self, _: &Wx) -> f64 {
        f64::from(*self)
    }
    fn load(&mut self, r: &f64, _: &mut Lx) {
        *self = mvs_sim::math::narrow(*r);
    }
    fn make(r: &f64, _: &mut Lx) -> Option<Self> {
        Some(mvs_sim::math::narrow(*r))
    }
}

/// A double the World keeps (the build's clock, the collision's and the anim's doubles at their boundary).
impl Rec<f64> for f64 {
    fn put(&self, _: &Wx) -> f64 {
        *self
    }
    fn load(&mut self, r: &f64, _: &mut Lx) {
        *self = *r;
    }
    fn make(r: &f64, _: &mut Lx) -> Option<Self> {
        Some(*r)
    }
}

impl Rec<f64> for i32 {
    fn put(&self, _: &Wx) -> f64 {
        f64::from(*self)
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        if let Some(v) = int(*r, l) {
            *self = v;
        }
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        int(*r, l)
    }
}

impl Rec<f64> for u32 {
    fn put(&self, _: &Wx) -> f64 {
        f64::from(*self)
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        if let Some(v) = uint(*r, u32::MAX, l) {
            *self = v;
        }
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        uint(*r, u32::MAX, l)
    }
}

impl Rec<f64> for u8 {
    fn put(&self, _: &Wx) -> f64 {
        f64::from(*self)
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        if let Some(v) = Self::make(r, l) {
            *self = v;
        }
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        uint(*r, 255, l).map(|v| u8::try_from(v).expect("checked"))
    }
}

impl Rec<bool> for bool {
    fn put(&self, _: &Wx) -> bool {
        *self
    }
    fn load(&mut self, r: &bool, _: &mut Lx) {
        *self = *r;
    }
    fn make(r: &bool, _: &mut Lx) -> Option<Self> {
        Some(*r)
    }
}

// Deviation: an oracle flag that is undefined until set (`stun`, `allyHit`, `noWalkOffLedges`) is a Rust `bool`; its
// `false` is written as undefined, and an oracle `false` loads as `false`.
impl Rec<Option<bool>> for bool {
    fn put(&self, _: &Wx) -> Option<bool> {
        self.then_some(true)
    }
    fn load(&mut self, r: &Option<bool>, _: &mut Lx) {
        *self = r.unwrap_or(false);
    }
    fn make(r: &Option<bool>, _: &mut Lx) -> Option<Self> {
        Some(r.unwrap_or(false))
    }
}

impl Rec<String> for Name {
    fn put(&self, x: &Wx) -> String {
        x.str(*self).to_owned()
    }
    fn load(&mut self, r: &String, l: &mut Lx) {
        *self = l.name(r);
    }
    fn make(r: &String, l: &mut Lx) -> Option<Self> {
        Some(l.name(r))
    }
}

impl<T: Rec<R>, R> Rec<Option<R>> for Option<T> {
    fn put(&self, x: &Wx) -> Option<R> {
        self.as_ref().map(|v| v.put(x))
    }
    fn load(&mut self, r: &Option<R>, l: &mut Lx) {
        match (self.as_mut(), r) {
            (_, None) => *self = None,
            (Some(v), Some(r)) => v.load(r, l),
            (None, Some(r)) => *self = T::make(r, l),
        }
    }
    fn make(r: &Option<R>, l: &mut Lx) -> Option<Self> {
        match r {
            None => Some(None),
            Some(r) => T::make(r, l).map(Some),
        }
    }
}

impl<T: Rec<R>, R> Rec<Vec<R>> for Vec<T> {
    fn put(&self, x: &Wx) -> Vec<R> {
        self.iter().map(|v| v.put(x)).collect()
    }
    fn load(&mut self, r: &Vec<R>, l: &mut Lx) {
        self.truncate(r.len());
        for (i, r) in r.iter().enumerate() {
            l.at(&format!("[{i}]"), |l| {
                if let Some(v) = self.get_mut(i) {
                    v.load(r, l);
                } else if let Some(v) = T::make(r, l) {
                    self.push(v);
                }
            });
            if self.len() <= i {
                // (an element that could not be made: the list stops short, named)
                return;
            }
        }
    }
    fn make(r: &Vec<R>, l: &mut Lx) -> Option<Self> {
        let mut v = Vec::new();
        v.load(r, l);
        Some(v)
    }
}

impl Rec<Vec<f64>> for IndexSet<i32> {
    fn put(&self, _: &Wx) -> Vec<f64> {
        self.iter().map(|&v| f64::from(v)).collect()
    }
    fn load(&mut self, r: &Vec<f64>, l: &mut Lx) {
        self.clear();
        for &v in r {
            if let Some(v) = int(v, l) {
                self.insert(v);
            }
        }
    }
    fn make(r: &Vec<f64>, l: &mut Lx) -> Option<Self> {
        via_default(r, l, Self::load)
    }
}

impl Rec<Vec<f64>> for IndexSet<i64> {
    fn put(&self, _: &Wx) -> Vec<f64> {
        #[allow(clippy::cast_precision_loss)] // a key the oracle holds as a number: under 2^53
        self.iter().map(|&v| v as f64).collect()
    }
    fn load(&mut self, r: &Vec<f64>, l: &mut Lx) {
        self.clear();
        for &v in r {
            if v.fract() == 0.0 && v.abs() < 9.007_199_254_740_992e15 {
                #[allow(clippy::cast_possible_truncation)] // whole and under 2^53, checked above
                self.insert(v as i64);
            } else {
                l.miss(format_args!("{v} is not a whole number"));
            }
        }
    }
    fn make(r: &Vec<f64>, l: &mut Lx) -> Option<Self> {
        via_default(r, l, Self::load)
    }
}

/// A fighter by its oracle id (`ID(f)`: -1 none).
impl Rec<f64> for Option<FighterId> {
    fn put(&self, x: &Wx) -> f64 {
        self.map_or(-1.0, |id| f64::from(x.fid(id)))
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        *self = l.fighter(*r);
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        Some(l.fighter(*r))
    }
}

/// An actor by its oracle id alone (`ID(a)`, `spawnedActors[].id`); loaded from the first list holding that id.
impl Rec<f64> for ActorRef {
    fn put(&self, x: &Wx) -> f64 {
        x.aref(*self).1
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        if let Some(v) = Self::make(r, l) {
            *self = v;
        }
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        // Unverified: the oracle id is the actor's own (`seq`); a projectile and a world actor share the actor
        // sequence, an item has its own, so an item and another actor can hold one id. The loader takes the
        // projectile, then the world actor, then the item.
        for family in ["projectile", "actors", "item", "fighter"] {
            let found = match family {
                "projectile" => l.projectiles.iter().any(|(t, _)| f64::from(*t) == *r),
                "actors" => l.actors.iter().any(|(t, _)| f64::from(*t) == *r),
                "item" => l.items.iter().any(|(t, _)| f64::from(*t) == *r),
                _ => l.fighters.iter().any(|(t, _)| f64::from(*t) == *r),
            };
            if found {
                return l.aref(family, *r);
            }
        }
        l.miss(format_args!("no actor of id {r}"));
        None
    }
}

impl Rec<f64> for Option<ActorRef> {
    fn put(&self, x: &Wx) -> f64 {
        self.map_or(-1.0, |r| x.aref(r).1)
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        *self = if *r < 0.0 { None } else { ActorRef::make(r, l) };
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        Some(if *r < 0.0 { None } else { ActorRef::make(r, l) })
    }
}

impl Rec<mvs_trace::RefRecord> for ActorRef {
    fn put(&self, x: &Wx) -> mvs_trace::RefRecord {
        let (family, id) = x.aref(*self);
        mvs_trace::RefRecord { family: family.to_owned(), id }
    }
    fn load(&mut self, r: &mvs_trace::RefRecord, l: &mut Lx) {
        if let Some(v) = Self::make(r, l) {
            *self = v;
        }
    }
    fn make(r: &mvs_trace::RefRecord, l: &mut Lx) -> Option<Self> {
        l.aref(&r.family, r.id)
    }
}

impl Rec<mvs_trace::RefRecord> for FighterId {
    fn put(&self, x: &Wx) -> mvs_trace::RefRecord {
        mvs_trace::RefRecord { family: "fighter".to_owned(), id: f64::from(x.fid(*self)) }
    }
    fn load(&mut self, r: &mvs_trace::RefRecord, l: &mut Lx) {
        if let Some(v) = Self::make(r, l) {
            *self = v;
        }
    }
    fn make(r: &mvs_trace::RefRecord, l: &mut Lx) -> Option<Self> {
        if r.family != "fighter" {
            l.miss(format_args!("a {} where a fighter is held", r.family));
            return None;
        }
        l.fighter(r.id)
    }
}

/// A projectile held by a character state (Reindog's tether): a live one, or one gone from the list (`spawned`).
impl Rec<mvs_trace::RefRecord> for mvs_sim::actors::ProjectileId {
    fn put(&self, x: &Wx) -> mvs_trace::RefRecord {
        ActorRef::Projectile(*self).put(x)
    }
    fn load(&mut self, r: &mvs_trace::RefRecord, l: &mut Lx) {
        if let Some(v) = Self::make(r, l) {
            *self = v;
        }
    }
    fn make(r: &mvs_trace::RefRecord, l: &mut Lx) -> Option<Self> {
        match l.aref(&r.family, r.id)? {
            ActorRef::Projectile(p) => Some(p),
            _ => {
                l.miss(format_args!("a {} where a projectile is held", r.family));
                None
            }
        }
    }
}

/// A fighter held by its oracle id alone (`ID(f)` in a list: Stripe's tracker's `exitBound`).
impl Rec<f64> for FighterId {
    fn put(&self, x: &Wx) -> f64 {
        f64::from(x.fid(*self))
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        if let Some(v) = Self::make(r, l) {
            *self = v;
        }
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        l.fighter(*r)
    }
}

/// A projectile held by its oracle id alone (a list of ids: Stripe's saws, his tracker's `endBound`); loaded from the
/// live lists ([`ActorRef`]'s rule).
impl Rec<f64> for mvs_sim::actors::ProjectileId {
    fn put(&self, x: &Wx) -> f64 {
        ActorRef::Projectile(*self).put(x)
    }
    fn load(&mut self, r: &f64, l: &mut Lx) {
        if let Some(v) = Self::make(r, l) {
            *self = v;
        }
    }
    fn make(r: &f64, l: &mut Lx) -> Option<Self> {
        match ActorRef::make(r, l)? {
            ActorRef::Projectile(p) => Some(p),
            a => {
                l.miss(format_args!("{a:?} where a projectile is held"));
                None
            }
        }
    }
}

impl Rec<mvs_trace::NotifyRecord> for NotifyRef {
    fn put(&self, x: &Wx) -> mvs_trace::NotifyRecord {
        match x.notify(*self) {
            Some(n) => mvs_trace::NotifyRecord {
                class: n.cls.clone(),
                inst: n.inst.clone(),
                t: f64::from_bits(n.t),
                track: f64::from(n.track),
                seq: n.seq.clone(),
                slot: n.slot.clone(),
            },
            None => mvs_trace::NotifyRecord::default(),
        }
    }
    fn load(&mut self, r: &mvs_trace::NotifyRecord, l: &mut Lx) {
        if let Some(id) = notify_id(r, l)
            && let Some(n) = l.notify(&id, Some(*self))
        {
            *self = n;
        }
    }
    fn make(r: &mvs_trace::NotifyRecord, l: &mut Lx) -> Option<Self> {
        let id = notify_id(r, l)?;
        l.notify(&id, None)
    }
}

pub(crate) fn notify_id(r: &mvs_trace::NotifyRecord, l: &mut Lx) -> Option<NotifyId> {
    Some(NotifyId {
        cls: r.class.clone(),
        inst: r.inst.clone(),
        t: r.t.to_bits(),
        track: int(r.track, l)?,
        seq: r.seq.clone(),
        slot: r.slot.clone(),
    })
}

// ------------------------------------------------------------------ plain values: scalars and containers

/// The value as an object, else named.
pub fn as_obj<'v>(v: &'v Value, l: &mut Lx) -> Option<&'v Obj> {
    match v {
        Value::Obj(o) => Some(o),
        _ => {
            l.miss(format_args!("{v:?} is not an object"));
            None
        }
    }
}

pub(crate) fn as_list<'v>(v: &'v Value, l: &mut Lx) -> Option<&'v [Value]> {
    match v {
        Value::List(a) => Some(a),
        _ => {
            l.miss(format_args!("{v:?} is not a list"));
            None
        }
    }
}

/// The list of a `{<tag>: [...]}` wrapper (a Map's `map`, a Set's `set`).
fn wrapped<'v>(v: &'v Value, tag: &str, l: &mut Lx) -> Option<&'v [Value]> {
    let o = as_obj(v, l)?;
    match o.get(tag) {
        Some(inner) if o.len() == 1 => as_list(inner, l),
        _ => {
            l.miss(format_args!("{v:?} is not a {{{tag}: [...]}}"));
            None
        }
    }
}

pub fn num(v: &Value, l: &mut Lx) -> Option<f64> {
    match v {
        Value::Num(n) => Some(*n),
        _ => {
            l.miss(format_args!("{v:?} is not a number"));
            None
        }
    }
}

pub fn text<'v>(v: &'v Value, l: &mut Lx) -> Option<&'v str> {
    match v {
        Value::Str(s) => Some(s),
        _ => {
            l.miss(format_args!("{v:?} is not a string"));
            None
        }
    }
}

/// Names each key of `o` the struct's mapping does not hold (a value the loader would lose).
pub fn check_keys(o: &Obj, known: &[&str], l: &mut Lx) {
    for (k, _) in o.entries() {
        if !known.contains(&k.as_str()) {
            l.miss(format_args!("key {k} has no Rust field"));
        }
    }
}

/// A list of the World's float32 numbers as the record's doubles (each widens exactly).
pub(crate) fn wide(v: &[Real]) -> Vec<f64> {
    v.iter().map(|&x| f64::from(x)).collect()
}

/// [`wide`]'s inverse: the record's doubles narrowed into the World's list.
pub(crate) fn narrow_into(s: &mut Vec<Real>, v: &[f64]) {
    s.clear();
    s.extend(v.iter().map(|&x| mvs_sim::math::narrow(x)));
}

pub fn obj(entries: Vec<(&str, Value)>) -> Value {
    Value::Obj(Obj::new(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()))
}

impl Plain for Real {
    fn to_value(&self, _: &Wx) -> Value {
        Value::Num(f64::from(*self))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(n) = num(v, l) {
            *self = mvs_sim::math::narrow(n);
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        num(v, l).map(mvs_sim::math::narrow)
    }
}

impl Plain for f64 {
    fn to_value(&self, _: &Wx) -> Value {
        Value::Num(*self)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(n) = num(v, l) {
            *self = n;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        num(v, l)
    }
}

impl Plain for i32 {
    fn to_value(&self, _: &Wx) -> Value {
        Value::Num(f64::from(*self))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(n) = Self::from_value(v, l) {
            *self = n;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        int(num(v, l)?, l)
    }
}

impl Plain for u8 {
    fn to_value(&self, _: &Wx) -> Value {
        Value::Num(f64::from(*self))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(n) = Self::from_value(v, l) {
            *self = n;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let n = num(v, l)?;
        <u8 as Rec<f64>>::make(&n, l)
    }
}

impl Plain for bool {
    fn to_value(&self, _: &Wx) -> Value {
        Value::Bool(*self)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(b) = Self::from_value(v, l) {
            *self = b;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        match v {
            Value::Bool(b) => Some(*b),
            _ => {
                l.miss(format_args!("{v:?} is not a boolean"));
                None
            }
        }
    }
}

impl Plain for Name {
    fn to_value(&self, x: &Wx) -> Value {
        Value::Str(x.str(*self).to_owned())
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(n) = Self::from_value(v, l) {
            *self = n;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let s = text(v, l)?;
        Some(l.name(s))
    }
}

impl Plain for String {
    fn to_value(&self, _x: &Wx) -> Value {
        Value::Str(self.clone())
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(s) = text(v, l) {
            s.clone_into(self);
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        text(v, l).map(str::to_owned)
    }
}

impl<T: Plain> Plain for Option<T> {
    fn to_value(&self, x: &Wx) -> Value {
        self.as_ref().map_or(Value::Null, |v| v.to_value(x))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        match (self.as_mut(), v) {
            (_, Value::Null) => *self = None,
            (Some(s), v) => s.load_value(v, l),
            (None, v) => *self = T::from_value(v, l),
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        match v {
            Value::Null => Some(None),
            v => T::from_value(v, l).map(Some),
        }
    }
}

impl<T: Plain> Plain for Vec<T> {
    fn to_value(&self, x: &Wx) -> Value {
        Value::List(self.iter().map(|v| v.to_value(x)).collect())
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(a) = as_list(v, l) else { return };
        self.truncate(a.len());
        for (i, e) in a.iter().enumerate() {
            l.at(&format!("[{i}]"), |l| {
                if let Some(s) = self.get_mut(i) {
                    s.load_value(e, l);
                } else if let Some(s) = T::from_value(e, l) {
                    self.push(s);
                }
            });
            if self.len() <= i {
                return;
            }
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut s = Vec::new();
        s.load_value(v, l);
        Some(s)
    }
}

impl<T: Plain, const N: usize> Plain for [T; N] {
    fn to_value(&self, x: &Wx) -> Value {
        Value::List(self.iter().map(|v| v.to_value(x)).collect())
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(a) = as_list(v, l) else { return };
        if a.len() != N {
            l.miss(format_args!("a list of {} where {N} are held", a.len()));
            return;
        }
        for (s, e) in self.iter_mut().zip(a) {
            s.load_value(e, l);
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let a = as_list(v, l)?;
        let items: Vec<T> = a.iter().map(|e| T::from_value(e, l)).collect::<Option<_>>()?;
        items.try_into().ok()
    }
}

/// A Set: `{set: [...]}` in its order.
impl<T: Plain + Hash + Eq> Plain for IndexSet<T> {
    fn to_value(&self, x: &Wx) -> Value {
        obj(vec![("set", Value::List(self.iter().map(|v| v.to_value(x)).collect()))])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(a) = wrapped(v, "set", l) else { return };
        self.clear();
        for e in a {
            if let Some(e) = T::from_value(e, l) {
                self.insert(e);
            }
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut s = IndexSet::new();
        s.load_value(v, l);
        Some(s)
    }
}

/// A Map: `{map: [[key, value], ...]}` in its order; a value already under its key is loaded in place.
impl<K: Plain + Hash + Eq, V: Plain> Plain for IndexMap<K, V> {
    fn to_value(&self, x: &Wx) -> Value {
        let entries = self.iter().map(|(k, v)| Value::List(vec![k.to_value(x), v.to_value(x)])).collect();
        obj(vec![("map", Value::List(entries))])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(a) = wrapped(v, "map", l) else { return };
        let mut old = std::mem::take(self);
        for e in a {
            let Some([k, v]) = as_list(e, l).and_then(|p| <&[Value; 2]>::try_from(p).ok()) else {
                l.miss("a map entry that is not a [key, value] pair");
                continue;
            };
            let Some(k) = K::from_value(k, l) else { continue };
            let val = match old.shift_remove(&k) {
                Some(mut val) => {
                    val.load_value(v, l);
                    Some(val)
                }
                None => V::from_value(v, l),
            };
            if let Some(val) = val {
                self.insert(k, val);
            }
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut s = IndexMap::new();
        s.load_value(v, l);
        Some(s)
    }
}

/// A sim object by reference (`refs` of `setTick`): `{ref: <family>, id}`.
fn put_ref(family: &str, id: f64) -> Value {
    obj(vec![("ref", Value::Str(family.to_owned())), ("id", Value::Num(id))])
}

fn load_ref(v: &Value, l: &mut Lx) -> Option<ActorRef> {
    let o = as_obj(v, l)?;
    // A mortar aim gone from its list is written whole (see `to_value`).
    if o.get("ref").is_none() && matches!(o.get("cls"), Some(Value::Str(c)) if c.contains("MortarAim")) {
        return crate::morty::mortar::mortar_whole_load(o, l).map(ActorRef::Mortar);
    }
    // An item gone from its list (`item_whole`: its `holder` and `hitActors` keys) is written whole too.
    if o.get("ref").is_none() && o.get("holder").is_some() && o.get("hitActors").is_some() {
        return crate::actors::item_whole_load(o, l).map(ActorRef::Item);
    }
    // A world actor gone from its list is written whole too.
    if o.get("ref").is_none() && o.get("cls").is_some() && o.get("owner").is_some() {
        return crate::actors::actor_whole_load(o, l).map(ActorRef::Actor);
    }
    let (Some(Value::Str(family)), Some(Value::Num(id))) = (o.get("ref"), o.get("id")) else {
        l.miss(format_args!("{v:?} is not a reference"));
        return None;
    };
    l.aref(family, *id)
}

impl Plain for ActorRef {
    fn to_value(&self, x: &Wx) -> Value {
        // A mortar aim gone from its list is in no family of `setTick`: the oracle writes the object whole.
        if let ActorRef::Mortar(m) = *self
            && x.w.actors.mortars.get(m).is_none()
            && let Some((_, a)) = x.w.actors.gone_mortars.iter().find(|(g, _)| *g == m)
        {
            return crate::morty::mortar::mortar_whole(a, x);
        }
        // (one already being written whole, reached again through its own hit set, is written by reference)
        let outer = x.whole.borrow().contains(self);
        if let ActorRef::Actor(g) = *self
            && !outer
            && x.w.actors.actors.get(g).is_none()
            && let Some((_, a)) = x.w.actors.gone_actors.iter().find(|(id, _)| *id == g)
        {
            x.whole.borrow_mut().push(*self);
            let v = crate::actors::actor_whole(a, x);
            x.whole.borrow_mut().pop();
            return v;
        }
        // So is an item (a dead one a hit set still holds).
        if let ActorRef::Item(g) = *self
            && !outer
            && x.w.actors.items.get(g).is_none()
            && let Some((_, i)) = x.w.actors.gone_items.iter().find(|(id, _)| *id == g)
        {
            x.whole.borrow_mut().push(*self);
            let v = crate::actors::item_whole(i, x);
            x.whole.borrow_mut().pop();
            return v;
        }
        let (family, id) = x.aref(*self);
        put_ref(family, id)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(r) = load_ref(v, l) {
            *self = r;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        load_ref(v, l)
    }
}

impl Plain for FighterId {
    fn to_value(&self, x: &Wx) -> Value {
        put_ref("fighter", f64::from(x.fid(*self)))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(r) = Self::from_value(v, l) {
            *self = r;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        match load_ref(v, l)? {
            ActorRef::Fighter(id) => Some(id),
            _ => {
                l.miss("an actor where a fighter is held");
                None
            }
        }
    }
}

/// A notify of the data by its identity (`notifyId`): `{notify, inst, t, track}` and `seq` / `slot` when defined.
impl Plain for NotifyRef {
    fn to_value(&self, x: &Wx) -> Value {
        let Some(n) = x.notify(*self) else { return Value::Null };
        let mut e = vec![
            ("notify", Value::Str(n.cls.clone())),
            ("inst", n.inst.clone().map_or(Value::Null, Value::Str)),
            ("t", Value::Num(f64::from_bits(n.t))),
            ("track", Value::Num(f64::from(n.track))),
        ];
        if let Some(s) = &n.seq {
            e.push(("seq", Value::Str(s.clone())));
        }
        if let Some(s) = &n.slot {
            e.push(("slot", Value::Str(s.clone())));
        }
        obj(e)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(id) = plain_notify_id(v, l)
            && let Some(n) = l.notify(&id, Some(*self))
        {
            *self = n;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let id = plain_notify_id(v, l)?;
        l.notify(&id, None)
    }
}

fn plain_notify_id(v: &Value, l: &mut Lx) -> Option<NotifyId> {
    let o = as_obj(v, l)?;
    let s = |k: &str| match o.get(k) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let (Some(cls), Some(Value::Num(t)), Some(Value::Num(track))) = (s("notify"), o.get("t"), o.get("track")) else {
        l.miss(format_args!("{v:?} is not a notify identity"));
        return None;
    };
    Some(NotifyId { cls, inst: s("inst"), t: t.to_bits(), track: int(*track, l)?, seq: s("seq"), slot: s("slot") })
}

/// A pair as a two-element list.
// Unverified: the oracle shape of the pairs the Rust holds as tuples (`openPose` / `copyPose`, a montage and a time);
// the traces hold none set.
impl<A: Plain, B: Plain> Plain for (A, B) {
    fn to_value(&self, x: &Wx) -> Value {
        Value::List(vec![self.0.to_value(x), self.1.to_value(x)])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(p) = Self::from_value(v, l) {
            *self = p;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        match as_list(v, l)? {
            [a, b] => Some((A::from_value(a, l)?, B::from_value(b, l)?)),
            _ => {
                l.miss("a pair that is not two values");
                None
            }
        }
    }
}

// ------------------------------------------------------------------ the struct macros

/// A struct written as a record struct: `record_field: rust.path, ...`, each by its `Rec`.
macro_rules! rec_rows {
    (put $s:ident, $x:ident, $r:ident; $($rf:ident : $($p:ident).+),* $(,)?) => {
        $( $r.$rf = $crate::conv::Rec::put(&$s.$($p).+, $x); )*
    };
    (load $s:ident, $r:ident, $l:ident; $($rf:ident : $($p:ident).+),* $(,)?) => {
        $( $l.at(stringify!($rf), |l| $crate::conv::Rec::load(&mut $s.$($p).+, &$r.$rf, l)); )*
    };
}
pub(crate) use rec_rows;

/// A `Rec` impl of a struct with a `Default` whose record fields are its fields one for one.
macro_rules! rec_struct {
    ($ty:ty => $rec:ty { $($rf:ident : $($p:ident).+),* $(,)? }) => {
        impl $crate::conv::Rec<$rec> for $ty {
            fn put(&self, x: &$crate::cx::Wx) -> $rec {
                let mut r = <$rec>::default();
                $crate::conv::rec_rows!(put self, x, r; $($rf : $($p).+),*);
                r
            }
            fn load(&mut self, r: &$rec, l: &mut $crate::cx::Lx) {
                $crate::conv::rec_rows!(load self, r, l; $($rf : $($p).+),*);
            }
            fn make(r: &$rec, l: &mut $crate::cx::Lx) -> Option<Self> {
                $crate::conv::via_default(r, l, Self::load)
            }
        }
    };
}
pub(crate) use rec_struct;

/// One key of a plain struct: `=` always written (an unset option as null), `~` left out while unset, `^` a pitch
/// the World holds in degrees and the record in radians, always written ([`deg_put`]).
macro_rules! plain_key {
    (put $o:ident, $x:ident, $k:literal, ^, $e:expr) => {
        $o.push(($k.to_owned(), $crate::conv::deg_put($e)))
    };
    (load $ob:ident, $l:ident, $k:literal, ^, $e:expr) => {
        match $ob.get($k) {
            Some(v) => $l.at($k, |l| $crate::conv::deg_load(&mut $e, v, l)),
            None => $l.miss(format_args!("key {} absent", $k)),
        }
    };
    (load_sparse $ob:ident, $l:ident, $k:literal, ^, $e:expr) => {
        if let Some(v) = $ob.get($k) {
            $l.at($k, |l| $crate::conv::deg_load(&mut $e, v, l))
        }
    };
    (absent $ob:ident, $k:literal, ^) => {
        $ob.get($k).is_none()
    };
    (put $o:ident, $x:ident, $k:literal, =, $e:expr) => {
        $o.push(($k.to_owned(), $crate::conv::Plain::to_value(&$e, $x)))
    };
    (put $o:ident, $x:ident, $k:literal, ~, $e:expr) => {
        if !$crate::conv::Unset::unset(&$e) {
            $o.push(($k.to_owned(), $crate::conv::Plain::to_value(&$e, $x)))
        }
    };
    (load $ob:ident, $l:ident, $k:literal, =, $e:expr) => {
        match $ob.get($k) {
            Some(v) => $l.at($k, |l| $crate::conv::Plain::load_value(&mut $e, v, l)),
            None => $l.miss(format_args!("key {} absent", $k)),
        }
    };
    // A sparse object: a member absent is left as it was (neither a miss nor cleared).
    (load_sparse $ob:ident, $l:ident, $k:literal, $m:tt, $e:expr) => {
        if let Some(v) = $ob.get($k) {
            $l.at($k, |l| $crate::conv::Plain::load_value(&mut $e, v, l))
        }
    };
    // Whether a member the struct always writes (`=`) is absent from the object.
    (absent $ob:ident, $k:literal, =) => {
        $ob.get($k).is_none()
    };
    (absent $ob:ident, $k:literal, ~) => {
        false
    };
    (load $ob:ident, $l:ident, $k:literal, ~, $e:expr) => {
        match $ob.get($k) {
            Some(v) => $l.at($k, |l| $crate::conv::Plain::load_value(&mut $e, v, l)),
            None => $crate::conv::Unset::clear(&mut $e),
        }
    };
    // `#`: an `Option` of a Map the oracle holds as a plain object keyed by number (`Record<number, V>`), absent unset.
    (put $o:ident, $x:ident, $k:literal, #, $e:expr) => {
        if let Some(m) = &$e {
            $o.push(($k.to_owned(), $crate::conv::int_keyed_put(m, $x)))
        }
    };
    (load $ob:ident, $l:ident, $k:literal, #, $e:expr) => {
        match $ob.get($k) {
            Some(v) => $l.at($k, |l| $crate::conv::int_keyed_load(&mut $e, v, l)),
            None => $e = None,
        }
    };
}
pub(crate) use plain_key;

/// A pitch the World holds in degrees (FRotator's) as the record's radians (the oracle's unit): the double product of
/// the widened float32 and pi / 180, which [`deg_load`] turns back exactly (the product's error, under 2^-52 of it, is
/// far below half a float32 ulp of the degrees, so the narrowing lands on the same float32).
pub(crate) fn deg_put(d: Real) -> Value {
    Value::Num(f64::from(d) * (std::f64::consts::PI / 180.0))
}

/// [`deg_put`]'s inverse.
pub(crate) fn deg_load(d: &mut Real, v: &Value, l: &mut Lx) {
    if let Some(r) = num(v, l) {
        *d = deg_of(r);
    }
}

/// The record's radians as the World's degrees ([`deg_put`]'s inverse on the number).
pub(crate) fn deg_of(r: f64) -> Real {
    mvs_sim::math::narrow(r * (180.0 / std::f64::consts::PI))
}

/// A Map keyed by number as the oracle's plain object: `{"0": value}`, the keys in the object's own order.
pub(crate) fn int_keyed_put<V: Plain>(m: &IndexMap<i32, V>, x: &Wx) -> Value {
    Value::Obj(Obj::new(m.iter().map(|(k, v)| (k.to_string(), v.to_value(x))).collect()))
}

/// [`int_keyed_put`]'s inverse; a value already under its key is loaded in place.
pub(crate) fn int_keyed_load<V: Plain>(m: &mut Option<IndexMap<i32, V>>, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    let mut old = m.take().unwrap_or_default();
    let mut out = IndexMap::new();
    for (ks, v) in ob.entries() {
        let Ok(k) = ks.parse::<i32>() else {
            l.miss(format_args!("key {ks} is not a number"));
            continue;
        };
        let val = match old.shift_remove(&k) {
            Some(mut val) => {
                val.load_value(v, l);
                Some(val)
            }
            None => V::from_value(v, l),
        };
        if let Some(val) = val {
            out.insert(k, val);
        }
    }
    *m = Some(out);
}

/// The keys of a plain struct, written into `$o` (a `Vec<(String, Value)>`) and loaded from the object `$ob`.
macro_rules! plain_keys {
    (put $s:ident, $x:ident, $o:ident; $($k:literal $m:tt $($p:ident).+),* $(,)?) => {
        $( $crate::conv::plain_key!(put $o, $x, $k, $m, $s.$($p).+); )*
    };
    (load $s:ident, $ob:ident, $l:ident; $($k:literal $m:tt $($p:ident).+),* $(,)?) => {
        $( $crate::conv::plain_key!(load $ob, $l, $k, $m, $s.$($p).+); )*
    };
    (load_sparse $s:ident, $ob:ident, $l:ident; $($k:literal $m:tt $($p:ident).+),* $(,)?) => {
        $( $crate::conv::plain_key!(load_sparse $ob, $l, $k, $m, $s.$($p).+); )*
    };
    (absent $ob:ident; $($k:literal $m:tt $($p:ident).+),* $(,)?) => {
        false $(|| $crate::conv::plain_key!(absent $ob, $k, $m))*
    };
    (names $($k:literal $m:tt $($p:ident).+),* $(,)?) => {
        &[$($k),*]
    };
}
pub(crate) use plain_keys;

/// A `Plain` impl of a struct with a `Default`, as an object of the listed keys.
macro_rules! plain_struct {
    ($ty:ty { $($k:literal $m:tt $($p:ident).+),* $(,)? }) => {
        $crate::conv::plain_struct!($ty { $($k $m $($p).+),* } make |v, l| $crate::conv::via_default(v, l, <$ty as $crate::conv::Plain>::load_value));
    };
    // `make`: how a value is made where the type has no default.
    ($ty:ty { $($k:literal $m:tt $($p:ident).+),* $(,)? } make $make:expr) => {
        impl $crate::conv::Plain for $ty {
            fn to_value(&self, x: &$crate::cx::Wx) -> mvs_trace::Value {
                let mut o: Vec<(String, mvs_trace::Value)> = Vec::new();
                $crate::conv::plain_keys!(put self, x, o; $($k $m $($p).+),*);
                mvs_trace::Value::Obj(mvs_trace::Obj::new(o))
            }
            fn load_value(&mut self, v: &mvs_trace::Value, l: &mut $crate::cx::Lx) {
                let Some(ob) = $crate::conv::as_obj(v, l) else { return };
                $crate::conv::plain_keys!(load self, ob, l; $($k $m $($p).+),*);
                $crate::conv::check_keys(ob, $crate::conv::plain_keys!(names $($k $m $($p).+),*), l);
            }
            fn from_value(v: &mvs_trace::Value, l: &mut $crate::cx::Lx) -> Option<Self> {
                let make: fn(&mvs_trace::Value, &mut $crate::cx::Lx) -> Option<Self> = $make;
                make(v, l)
            }
        }
    };
}
pub(crate) use plain_struct;

/// `Plain` of a type the traces never hold a value of: written as null and named, never loaded.
macro_rules! opaque {
    ($($ty:ty),* $(,)?) => {$(
        impl $crate::conv::Plain for $ty {
            fn to_value(&self, x: &$crate::cx::Wx) -> mvs_trace::Value {
                x.unmapped(concat!("a ", stringify!($ty), " (no rule to write it)"));
                mvs_trace::Value::Null
            }
            fn load_value(&mut self, _: &mvs_trace::Value, l: &mut $crate::cx::Lx) {
                l.miss(concat!("a ", stringify!($ty), " (no rule to load it)"));
            }
            fn from_value(_: &mvs_trace::Value, l: &mut $crate::cx::Lx) -> Option<Self> {
                l.miss(concat!("a ", stringify!($ty), " (no rule to make it)"));
                None
            }
        }
    )*};
}
pub(crate) use opaque;

/// `Rec<String>` / `Plain` of a fieldless enum by its oracle string, from a table of (variant, name).
macro_rules! named_enum {
    ($ty:ty, $what:literal, [$($v:expr => $n:literal),* $(,)?]) => {
        impl $crate::conv::Rec<String> for $ty {
            fn put(&self, x: &$crate::cx::Wx) -> String {
                match self {
                    $(v if *v == $v => $n,)*
                    _ => {
                        // (a variant added since this table was written)
                        x.unmapped(concat!("a ", $what, " with no name in the table"));
                        "?"
                    }
                }
                .to_owned()
            }
            fn load(&mut self, r: &String, l: &mut $crate::cx::Lx) {
                if let Some(v) = Self::make(r, l) {
                    *self = v;
                }
            }
            fn make(r: &String, l: &mut $crate::cx::Lx) -> Option<Self> {
                match r.as_str() {
                    $($n => Some($v),)*
                    _ => {
                        l.miss(format_args!("{r:?} is not a {}", $what));
                        None
                    }
                }
            }
        }
        impl $crate::conv::Plain for $ty {
            fn to_value(&self, x: &$crate::cx::Wx) -> mvs_trace::Value {
                mvs_trace::Value::Str($crate::conv::Rec::<String>::put(self, x))
            }
            fn load_value(&mut self, v: &mvs_trace::Value, l: &mut $crate::cx::Lx) {
                if let Some(v) = <Self as $crate::conv::Plain>::from_value(v, l) {
                    *self = v;
                }
            }
            fn from_value(v: &mvs_trace::Value, l: &mut $crate::cx::Lx) -> Option<Self> {
                let s = $crate::conv::text(v, l)?.to_owned();
                <Self as $crate::conv::Rec<String>>::make(&s, l)
            }
        }
    };
}
pub(crate) use named_enum;
