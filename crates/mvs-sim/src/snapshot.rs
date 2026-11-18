//! The rollback ring (and the net layer's saves): a fixed count of slots, each a whole
//! world, made once by cloning the world at the match's start. A save copies the world into the tick's slot by
//! `clone_from`, which reuses the slot's own allocations, so a save in a match whose shapes have settled (the same
//! fighters, buffs and actors as the slot held) allocates nothing; a load copies back the same way.
//!
//! The fields a type marks `#[state(skip)]` (the event buffer, scratch buffers, the static data)
//! are not copied either way.

/// `N` saved states, the one of tick `t` in slot `t % N`.
pub struct SnapshotRing<T: Clone> {
    slots: Vec<T>,
    ticks: Vec<Option<u32>>,
}

impl<T: Clone> SnapshotRing<T> {
    /// A ring of `n` slots, each a clone of `like` (the shape the saves will copy into).
    pub fn new(n: usize, like: &T) -> Self {
        assert!(n > 0, "a ring of no slots");
        SnapshotRing { slots: (0..n).map(|_| like.clone()).collect(), ticks: vec![None; n] }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn slot(&self, tick: u32) -> usize {
        tick as usize % self.slots.len()
    }

    /// Saves `state` as tick `tick`'s, over the slot's older tick.
    pub fn save(&mut self, tick: u32, state: &T) {
        let i = self.slot(tick);
        self.slots[i].clone_from(state);
        self.ticks[i] = Some(tick);
    }

    /// Copies tick `tick`'s state into `state`; false (and `state` untouched) when the ring no longer holds it.
    pub fn load(&self, tick: u32, state: &mut T) -> bool {
        let i = self.slot(tick);
        if self.ticks[i] != Some(tick) {
            return false;
        }
        state.clone_from(&self.slots[i]);
        true
    }

    /// The saved state of `tick`, if the ring holds it.
    pub fn get(&self, tick: u32) -> Option<&T> {
        let i = self.slot(tick);
        (self.ticks[i] == Some(tick)).then(|| &self.slots[i])
    }
}
