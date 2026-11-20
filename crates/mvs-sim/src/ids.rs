//! Generation-checked arena ids: what a reference to an object is held as (a fighter's `mostRecentAttacker`, a
//! projectile's owner, a buff's `by`) is an [`Id`] into the [`Arena`] of its family. A stale id (its slot freed and
//! reused) fails the generation check and reads as gone, where a plain reference would still hold the dead object:
//! code that reads a dead object's fields (a projectile's owner after its pawn left) keeps its own copy of what it
//! reads.
//!
//! An arena iterates in insertion order, as an array does: `live` lists the slots in the order they were added,
//! a removal keeps the order of the rest, and a reused slot goes at the end of `live`.

use crate::State;
use crate::state::{StateHash, StateHasher};
use std::marker::PhantomData;

/// An id of a `T` in its arena: a slot index and the generation it was handed out at.
pub struct Id<T> {
    pub index: u32,
    pub generation: u32,
    _of: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub const fn new(index: u32, generation: u32) -> Self {
        Id { index, generation, _of: PhantomData }
    }
}
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> {
    fn eq(&self, o: &Self) -> bool {
        self.index == o.index && self.generation == o.generation
    }
}
impl<T> Eq for Id<T> {}
impl<T> std::hash::Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (self.index, self.generation).hash(h);
    }
}
impl<T> std::fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}g{}", self.index, self.generation)
    }
}
impl<T> StateHash for Id<T> {
    fn hash_state(&self, h: &mut StateHasher) {
        h.write_u64((u64::from(self.generation) << 32) | u64::from(self.index));
    }
}

#[derive(State, Debug)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// The `T`s of one family, by [`Id`].
#[derive(State, Debug)]
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    /// Free slot indices, the last freed taken first.
    free: Vec<u32>,
    /// The live slots in insertion order: the iteration order.
    live: Vec<u32>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena { slots: Vec::new(), free: Vec::new(), live: Vec::new() }
    }
}

impl<T> Arena<T> {
    pub fn with_capacity(n: usize) -> Self {
        Arena { slots: Vec::with_capacity(n), free: Vec::with_capacity(n), live: Vec::with_capacity(n) }
    }

    /// Adds `v` at the end of the iteration order.
    pub fn insert(&mut self, v: T) -> Id<T> {
        let index = match self.free.pop() {
            Some(i) => {
                let s = &mut self.slots[i as usize];
                s.value = Some(v);
                i
            }
            None => {
                let i = u32::try_from(self.slots.len()).expect("arena of more than u32::MAX slots");
                self.slots.push(Slot { generation: 0, value: Some(v) });
                i
            }
        };
        self.live.push(index);
        Id::new(index, self.slots[index as usize].generation)
    }

    pub fn get(&self, id: Id<T>) -> Option<&T> {
        let s = self.slots.get(id.index as usize)?;
        if s.generation == id.generation { s.value.as_ref() } else { None }
    }

    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        let s = self.slots.get_mut(id.index as usize)?;
        if s.generation == id.generation { s.value.as_mut() } else { None }
    }

    pub fn contains(&self, id: Id<T>) -> bool {
        self.get(id).is_some()
    }

    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// The live ids in iteration order.
    pub fn ids(&self) -> impl Iterator<Item = Id<T>> + '_ {
        self.live.iter().map(|&i| Id::new(i, self.slots[i as usize].generation))
    }

    /// The id at place `n` of the iteration order.
    pub fn nth(&self, n: usize) -> Option<Id<T>> {
        let i = *self.live.get(n)?;
        Some(Id::new(i, self.slots[i as usize].generation))
    }

    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &T)> + '_ {
        self.live.iter().map(|&i| {
            let s = &self.slots[i as usize];
            (Id::new(i, s.generation), s.value.as_ref().expect("a live slot holds a value"))
        })
    }

    /// Puts `v` in the live slot of `id` in place of its value, keeping the id (its generation too: an id still held
    /// to the old value reads the new one), and moves the slot to the end of the iteration order, as a new insert
    /// goes. A stale `id` is a bug of the caller.
    pub fn replace(&mut self, id: Id<T>, v: T) {
        let s = self.slots.get_mut(id.index as usize).filter(|s| s.generation == id.generation && s.value.is_some());
        let s = s.expect("a live arena id to replace");
        s.value = Some(v);
        self.live.retain(|&i| i != id.index);
        self.live.push(id.index);
    }

    /// Removes every value `keep` says no to, keeping the order of the rest.
    pub fn retain(&mut self, mut keep: impl FnMut(Id<T>, &T) -> bool) {
        let slots = &mut self.slots;
        let free = &mut self.free;
        self.live.retain(|&i| {
            let s = &mut slots[i as usize];
            let id = Id::new(i, s.generation);
            if keep(id, s.value.as_ref().expect("a live slot holds a value")) {
                return true;
            }
            s.value = None;
            s.generation = s.generation.wrapping_add(1);
            free.push(i);
            false
        });
    }

    pub fn clear(&mut self) {
        self.retain(|_, _| false);
    }
}
