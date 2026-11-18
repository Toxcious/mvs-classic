//! The reads the hook bodies make of a data property bag (a notify's `p`, a class default object), as JavaScript reads
//! a `Record<string, any>`: an absent member and a member of another type both read as absent, and a test of a member
//! (`if (p.X)`) is JavaScript truthiness.

use crate::Real;
use crate::combat::buffs::{BuffRef, FighterBuff};
use crate::names::Name;
use crate::world::World;
use mvs_data::value::{Props, Value};

/// JavaScript truthiness of a value (`!!v`).
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Num(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        Value::Arr(_) | Value::Obj(_) | Value::F32s(_) => true,
    }
}

/// `p[k]` read as a number (`p[k] ?? undefined` of a numeric member).
pub fn num(p: &Props, k: &str) -> Option<Real> {
    p.get(k).and_then(Value::as_f64).map(crate::math::narrow)
}

/// `p[k] ?? d` of a numeric member.
pub fn num_or(p: &Props, k: &str, d: Real) -> Real {
    num(p, k).unwrap_or(d)
}

/// `!!p[k]`.
pub fn flag(p: &Props, k: &str) -> bool {
    p.get(k).is_some_and(truthy)
}

/// `p[k] ?? d` of a boolean member: `undefined` and `null` take the default, any other value its truthiness.
pub fn flag_or(p: &Props, k: &str, d: bool) -> bool {
    match p.get(k) {
        None | Some(Value::Null) => d,
        Some(v) => truthy(v),
    }
}

/// `p[k]` read as a string.
pub fn text<'a>(p: &'a Props, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

/// `p[k]` read as an object.
pub fn obj<'a>(p: &'a Props, k: &str) -> Option<&'a Props> {
    p.get(k).and_then(Value::as_obj)
}

/// `p[k] ?? []` of an array member.
pub fn arr<'a>(p: &'a Props, k: &str) -> &'a [Value] {
    p.get(k).and_then(Value::as_arr).unwrap_or(&[])
}

/// `p[k]?.[sub]` of an object member read as a number.
pub fn num_in(p: &Props, k: &str, sub: &str) -> Option<Real> {
    obj(p, k).and_then(|o| num(o, sub))
}

/// The objects of an array member (`p[k] ?? []`, its rows).
pub fn rows(p: &Props, k: &str) -> Vec<Props> {
    arr(p, k).iter().filter_map(Value::as_obj).cloned().collect()
}

/// A string the data names, as the match's interned [`Name`] (a literal of the code is `name!`): the data's strings
/// are interned at bind, so a miss is a string the data lacks; it reads as `Name::NONE`, as `''` does.
pub fn name(w: &World, s: &str) -> Name {
    w.names.get(s).unwrap_or(Name::NONE)
}

/// `p[k]` of a string member as a name (`None` when absent or not interned).
pub fn name_of(w: &World, p: &Props, k: &str) -> Option<Name> {
    text(p, k).and_then(|s| w.names.get(s))
}

/// The fighter buff a [`BuffRef`] names, if it is still in its list.
pub fn buff(w: &World, b: BuffRef) -> Option<&FighterBuff> {
    w.fighters.get(b.fighter)?.buffs.buffs.iter().find(|e| e.serial == b.serial)
}

/// The fighter buff a [`BuffRef`] names, to write.
pub fn buff_mut(w: &mut World, b: BuffRef) -> Option<&mut FighterBuff> {
    w.fighters.get_mut(b.fighter)?.buffs.buffs.iter_mut().find(|e| e.serial == b.serial)
}

/// `w.all`: the match's pawns (the minions too) where a mode made them, else the players.
pub fn all_fighters(w: &World) -> Vec<crate::fighter::FighterId> {
    w.pawns.clone().unwrap_or_else(|| w.players.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthiness_is_js() {
        assert!(!truthy(&Value::Null));
        assert!(!truthy(&Value::Num(0.0)));
        assert!(!truthy(&Value::Num(f64::NAN)));
        assert!(truthy(&Value::Num(-1.0)));
        assert!(!truthy(&Value::Str(String::new())));
        assert!(truthy(&Value::Arr(Vec::new())));
        let mut p = Props::new();
        p.insert("a".into(), Value::Null);
        assert!(flag_or(&p, "a", true));
        assert!(flag_or(&p, "b", true));
        p.insert("a".into(), Value::Num(0.0));
        assert!(!flag_or(&p, "a", true));
    }
}
