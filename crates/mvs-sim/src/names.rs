//! Interned names: the strings the state holds (a montage's, an anim's, a sound event's, a buff class's, a tag)
//! are a [`Name`] into the match's [`Names`], made at bind from the data and the code's own literals. A name in the
//! state costs four bytes, compares as an integer and copies without allocating.

use crate::state::{StateHash, StateHasher};
use indexmap::IndexSet;

/// An interned string of the match's [`Names`]. `Name::NONE` stands for `''` / `null` where one is held.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
pub struct Name(pub u32);

impl Name {
    /// The empty name, interned first.
    pub const NONE: Name = Name(0);

    pub fn is_none(self) -> bool {
        self == Name::NONE
    }
}

impl StateHash for Name {
    #[inline]
    fn hash_state(&self, h: &mut StateHasher) {
        h.write_u32(self.0);
    }
}

/// The match's string table. Filled at bind and read-only during the match (the World holds it behind an `Arc`),
/// so a step never interns: a string built at run time (`'proj' + id`) is a typed key instead.
#[derive(Clone, Debug)]
pub struct Names {
    set: IndexSet<Box<str>>,
}

impl Default for Names {
    fn default() -> Self {
        let mut set = IndexSet::with_capacity(CODE.len() + 1);
        set.insert(Box::from(""));
        for s in CODE {
            set.insert(Box::from(*s));
        }
        Names { set }
    }
}

include!(concat!(env!("OUT_DIR"), "/code_names.rs"));

/// The `Name` of one of the code's literals, at compile time: the build script lists every literal the `name!`
/// macro is given in `CODE`, which every `Names` interns first, so the id is the literal's place there. A string
/// `CODE` lacks fails the build (the const evaluation panics).
pub const fn code_id(s: &str) -> Name {
    let s = s.as_bytes();
    let mut i = 0;
    while i < CODE.len() {
        let c = CODE[i].as_bytes();
        if c.len() == s.len() {
            let mut j = 0;
            while j < s.len() && c[j] == s[j] {
                j += 1;
            }
            if j == s.len() {
                #[allow(clippy::cast_possible_truncation)]
                return Name(i as u32 + 1);
            }
        }
        i += 1;
    }
    panic!("a name literal the build script did not collect")
}

/// The `Name` of a string the code itself holds (an event, class, socket or tag name written as a literal),
/// as a constant: the macro takes one string literal, no escapes. The build script collects every use, so a literal
/// is interned in every match whether or not the data holds it.
#[macro_export]
macro_rules! name {
    ($s:literal) => {{
        const N: $crate::names::Name = $crate::names::code_id($s);
        N
    }};
}

/// The string of a `name!` literal's [`Name`] (the same in every match), `None` for a name the data brought in:
/// what holds a literal's name with no [`Names`] at hand reads it here.
pub fn code_str(n: Name) -> Option<&'static str> {
    CODE.get((n.0 as usize).checked_sub(1)?).copied()
}

fn name_at(i: usize) -> Name {
    Name(u32::try_from(i).expect("fewer than u32::MAX names"))
}

impl Names {
    pub fn intern(&mut self, s: &str) -> Name {
        if let Some(i) = self.set.get_index_of(s) {
            return name_at(i);
        }
        let (i, _) = self.set.insert_full(Box::from(s));
        name_at(i)
    }

    /// The name of `s` if it was interned at bind.
    pub fn get(&self, s: &str) -> Option<Name> {
        self.set.get_index_of(s).map(name_at)
    }

    pub fn str(&self, n: Name) -> &str {
        self.set.get_index(n.0 as usize).map_or("", |s| s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_literal_is_interned_at_its_compile_time_id() {
        let names = Names::default();
        let n = crate::name!("SmallForceFeedbackRumbleEffect");
        assert_eq!(names.get("SmallForceFeedbackRumbleEffect"), Some(n));
        assert_eq!(names.str(n), "SmallForceFeedbackRumbleEffect");
        assert_eq!(names.get(""), Some(Name::NONE));
    }
}
