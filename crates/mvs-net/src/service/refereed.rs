//! The bookkeeping of a match this process referees: its Referee with its
//! humans' ids and slots, and what one of its steps put out. A hosting's referee and a path's in-app referee share it.

use indexmap::IndexMap;

use mvs_protocol::referee::{Referee, RefereeOut, SlotNews};

/// A match this process referees: its Referee, its humans' ids by slot and their slots by id (a leaver out of both).
pub(crate) struct Refereed {
    pub(crate) referee: Referee,
    pub(crate) ids: IndexMap<u32, String>,
    pub(crate) slots: IndexMap<String, u32>,
}

/// What a Referee's step put out, in order.
#[derive(Default)]
pub(crate) struct Outs(pub(crate) Vec<RefOut>);

pub(crate) enum RefOut {
    Send(usize, Vec<u8>),
    News(usize, SlotNews),
}

impl RefereeOut for Outs {
    fn send(&mut self, slot: usize, bytes: &[u8]) {
        self.0.push(RefOut::Send(slot, bytes.to_vec()));
    }
    fn news(&mut self, slot: usize, n: SlotNews) {
        self.0.push(RefOut::News(slot, n));
    }
}
