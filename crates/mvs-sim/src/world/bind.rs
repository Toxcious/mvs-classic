//! The World's string table at the bind: every string of the match's data (each value and each map key) and of the
//! set-up is interned before the first step (after the code's own `name!` literals), so a step only looks names up.
//! Each is interned with its lower-cased form behind it (a run-time `toLowerCase()` of a data string: an
//! equip tag, a box tag or socket, a cooldown or injection tag, a class name), so that form is a name too.

use crate::names::Names;
use mvs_data::GameData;
use serde::Serialize;
use serde::ser;

/// Interns every string of `data` into `names`, in the data's order (the stage's first, then the
/// characters' by key, then the tables'): the ids follow the data, so two Worlds of the same data agree.
pub fn intern_all(names: &mut Names, data: &GameData) {
    let mut c = Collect { names };
    walk(&mut c, &data.stage, "stage");
    walk(&mut c, &data.stage_key, "stage_key");
    walk(&mut c, &data.default_character, "default_character");
    for (k, ch) in &data.chars {
        intern(c.names, k);
        walk(&mut c, &**ch, k);
    }
    walk(&mut c, &data.items, "items");
    walk(&mut c, &data.stage_manifest, "stage_manifest");
    walk(&mut c, &data.spawn_areas, "spawn_areas");
    walk(&mut c, &data.perks, "perks");
    walk(&mut c, &data.ring_outs, "ring_outs");
    walk(&mut c, &data.announcer, "announcer");
    walk(&mut c, &data.arcade, "arcade");
    walk(&mut c, &data.mutators, "mutators");
    walk(&mut c, &data.bot_trees, "bot_trees");
    walk(&mut c, &data.tutorials, "tutorials");
}

/// One part of the data through the collector. A part it cannot walk (a serde shape it does not take, or a type's
/// own error) fails the bind: a string left out would be looked up and missed in a step.
fn walk<T: Serialize + ?Sized>(c: &mut Collect<'_>, v: &T, what: &str) {
    if let Err(e) = v.serialize(c) {
        panic!("binding the data's strings: {what}: {e}");
    }
}

/// Interns the set-up's own strings (the players' characters, perks, taunts, ring-outs, banners, skins, the
/// mutators, a mode's actor and cast), which the data need not hold.
pub fn intern_setup(names: &mut Names, setup: &crate::setup::MatchSetup) {
    for p in &setup.players {
        for s in p.character.iter().chain(&p.perks).chain(&p.taunts).chain(&p.ring_out).chain(&p.banner).chain(&p.skin)
        {
            intern(names, s);
        }
    }
    for s in &setup.mutators {
        intern(names, s);
    }
    if let Some(crate::setup::MatchMode::Tutorial { actor, cast }) = &setup.mode {
        intern(names, actor);
        for s in cast {
            intern(names, s);
        }
    }
}

/// Interns `s`, then its lower case (JavaScript's `toLowerCase`: the Unicode full mapping, as `str::to_lowercase`) when
/// it differs.
fn intern(names: &mut Names, s: &str) {
    names.intern(s);
    let low = s.to_lowercase();
    if low != s {
        names.intern(&low);
    }
}

/// A serializer that writes nothing and interns every string it meets (a struct's field names are not data).
struct Collect<'a> {
    names: &'a mut Names,
}

/// What a part of the data the collector cannot walk reports (a type's own `ser::Error::custom`).
#[derive(Debug)]
struct WalkError(String);

impl std::fmt::Display for WalkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WalkError {}

impl ser::Error for WalkError {
    fn custom<T: std::fmt::Display>(m: T) -> Self {
        WalkError(m.to_string())
    }
}

type R = Result<(), WalkError>;

impl<'a, 'b> ser::Serializer for &'b mut Collect<'a> {
    type Ok = ();
    type Error = WalkError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, _: bool) -> R {
        Ok(())
    }
    fn serialize_i8(self, _: i8) -> R {
        Ok(())
    }
    fn serialize_i16(self, _: i16) -> R {
        Ok(())
    }
    fn serialize_i32(self, _: i32) -> R {
        Ok(())
    }
    fn serialize_i64(self, _: i64) -> R {
        Ok(())
    }
    fn serialize_u8(self, _: u8) -> R {
        Ok(())
    }
    fn serialize_u16(self, _: u16) -> R {
        Ok(())
    }
    fn serialize_u32(self, _: u32) -> R {
        Ok(())
    }
    fn serialize_u64(self, _: u64) -> R {
        Ok(())
    }
    fn serialize_f32(self, _: f32) -> R {
        Ok(())
    }
    fn serialize_f64(self, _: f64) -> R {
        Ok(())
    }
    fn serialize_char(self, _: char) -> R {
        Ok(())
    }
    fn serialize_str(self, v: &str) -> R {
        intern(self.names, v);
        Ok(())
    }
    fn serialize_bytes(self, _: &[u8]) -> R {
        Ok(())
    }
    fn serialize_none(self) -> R {
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_unit(self) -> R {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> R {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, v: &'static str) -> R {
        intern(self.names, v);
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(self, _: &'static str, _: u32, _: &'static str, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, WalkError> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, WalkError> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, WalkError> {
        Ok(self)
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self, WalkError> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, WalkError> {
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, WalkError> {
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self, WalkError> {
        Ok(self)
    }
}

impl ser::SerializeSeq for &mut Collect<'_> {
    type Ok = ();
    type Error = WalkError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeTuple for &mut Collect<'_> {
    type Ok = ();
    type Error = WalkError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for &mut Collect<'_> {
    type Ok = ();
    type Error = WalkError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for &mut Collect<'_> {
    type Ok = ();
    type Error = WalkError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeMap for &mut Collect<'_> {
    type Ok = ();
    type Error = WalkError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, k: &T) -> R {
        k.serialize(&mut **self)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeStruct for &mut Collect<'_> {
    type Ok = ();
    type Error = WalkError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, _: &'static str, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut Collect<'_> {
    type Ok = ();
    type Error = WalkError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, _: &'static str, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_data_string_is_interned_with_its_lower_case() {
        let mut names = Names::default();
        let mut c = Collect { names: &mut names };
        let _ = vec!["ZqRockMesh".to_string(), "zqjerrymesh".to_string()].serialize(&mut c);
        let a = names.get("ZqRockMesh").expect("the original");
        let b = names.get("zqrockmesh").expect("its lower case");
        assert_ne!(a, b);
        assert!(names.get("zqjerrymesh").is_some());
    }
}
