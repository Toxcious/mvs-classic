//! A bot's blackboard (UBlackboardComponent) over the keys of BotData: a value per key, by
//! the key's place in the data's list. Unset Object and Vector keys read null (Vector: FAISystem::InvalidLocation).

use crate::Real;
use crate::actors::ActorRef;
use mvs_data::tables::BlackboardKey;
use std::sync::Arc;

/// A key: its place in the blackboard's key list.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BbKey(pub u8);

/// A key's type (UBlackboardKeyType_*): the ones the runtime reads, `Other` for the rest (Name, Class, String,
/// Rotator), which read null until set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BbType {
    Bool,
    Float,
    Int,
    Enum,
    Object,
    Vector,
    Other,
}

impl BbType {
    fn of(s: &str) -> BbType {
        match s {
            "Bool" => BbType::Bool,
            "Float" => BbType::Float,
            "Int" => BbType::Int,
            "Enum" => BbType::Enum,
            "Object" => BbType::Object,
            "Vector" => BbType::Vector,
            _ => BbType::Other,
        }
    }
}

/// `BbVec`: a Vector key's value, in UE world axes (X, Y side, Z up).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct BbVec {
    pub x: Real,
    pub y: Real,
    pub z: Real,
}

/// A key's value. `Null` is a null (an unset Object or Vector key, an Object key whose actor was destroyed).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BbVal {
    Null,
    Bool(bool),
    /// A Float, Int or Enum key's value.
    Num(Real),
    Obj(ActorRef),
    Vec(BbVec),
}

impl BbVal {
    /// `Blackboard.initial`: false, 0, or null.
    pub fn initial(t: BbType) -> BbVal {
        match t {
            BbType::Bool => BbVal::Bool(false),
            BbType::Float | BbType::Int | BbType::Enum => BbVal::Num(0.0),
            _ => BbVal::Null,
        }
    }
}

/// `Blackboard`: the values of BotData's keys. The key list is the data's, shared by every bot of the match.
#[derive(Clone, Debug)]
pub struct Blackboard {
    keys: Arc<[(Box<str>, BbType)]>,
    v: Vec<BbVal>,
}

impl Default for Blackboard {
    /// A blackboard with no key (a placeholder: `BtBot::default`).
    fn default() -> Self {
        Blackboard { keys: Arc::from(Vec::new()), v: Vec::new() }
    }
}

impl Blackboard {
    pub fn new(keys: &[BlackboardKey]) -> Blackboard {
        assert!(keys.len() <= 256, "BotData has {} keys: more than a BbKey holds", keys.len());
        let keys: Arc<[(Box<str>, BbType)]> =
            keys.iter().map(|k| (Box::from(k.name.as_str()), BbType::of(&k.ty))).collect();
        let mut b = Blackboard { keys, v: Vec::new() };
        b.reset();
        b
    }

    /// Every key back to its initial value; observers hear of nothing.
    pub fn reset(&mut self) {
        self.v.clear();
        self.v.extend(self.keys.iter().map(|k| BbVal::initial(k.1)));
    }

    /// The key of a name (`None` for a name the data does not hold, 'None' among them).
    pub fn key(&self, name: &str) -> Option<BbKey> {
        let i = self.keys.iter().position(|k| &*k.0 == name)?;
        #[allow(clippy::cast_possible_truncation, reason = "new holds at most 256 keys")]
        Some(BbKey(i as u8))
    }

    /// The key's name.
    pub fn name(&self, k: BbKey) -> &str {
        &self.keys[usize::from(k.0)].0
    }

    /// `type(key)`: `None` for no key.
    pub fn ty(&self, k: Option<BbKey>) -> Option<BbType> {
        k.map(|k| self.keys[usize::from(k.0)].1)
    }

    /// `get(key)`: `Null` for no key.
    pub fn get(&self, k: Option<BbKey>) -> BbVal {
        k.map_or(BbVal::Null, |k| self.v[usize::from(k.0)])
    }

    /// `obj(key)`: the actor an Object key holds.
    pub fn obj(&self, k: Option<BbKey>) -> Option<ActorRef> {
        match self.get(k) {
            BbVal::Obj(a) => Some(a),
            _ => None,
        }
    }

    /// `vec(key)`: a Vector key's value, `None` while unset.
    pub fn vec(&self, k: Option<BbKey>) -> Option<BbVec> {
        match self.get(k) {
            BbVal::Vec(v) => Some(v),
            _ => None,
        }
    }

    /// `num(key)`: 0 for no key or a value that is no number.
    pub fn num(&self, k: Option<BbKey>) -> Real {
        match self.get(k) {
            BbVal::Num(n) => n,
            _ => 0.0,
        }
    }

    /// `bool(key)`: the value is true.
    pub fn bool(&self, k: Option<BbKey>) -> bool {
        self.get(k) == BbVal::Bool(true)
    }

    /// The write of `set(key, v)` without the observers: true when the value changed (a Vector compares by its
    /// members, the rest by `===`: an actor by its reference, a number by value). The tree's observers are told by
    /// the caller ([`crate::ai::cx::Cx::set`], the runtime's own writes).
    pub fn put(&mut self, k: BbKey, v: BbVal) -> bool {
        let old = &mut self.v[usize::from(k.0)];
        if *old == v {
            return false;
        }
        *old = v;
        true
    }

    /// `actorsDestroyed`: the actors the Object keys held were destroyed; the keys are weak pointers and read null
    /// from then on. No observer hears of it.
    pub fn actors_destroyed(&mut self) {
        for (v, k) in self.v.iter_mut().zip(self.keys.iter()) {
            if k.1 == BbType::Object {
                *v = BbVal::Null;
            }
        }
    }
}
