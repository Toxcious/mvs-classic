//! Online play in the program: the game's client of the net service (
//! it fills the menus' `OnlineView` and runs their `OnlineCmd`s), the menus' online parts, and the dev runs' online
//! match by the command line (`dev`).

pub mod client;
pub mod desync_report;
pub mod dev;
pub mod fake_lobbies;
pub mod fake_queue;
pub mod gate;
pub mod glue;
pub mod hub;
pub mod link;
pub mod menus;
pub mod net_match;
pub mod port;
pub mod pre_match;
pub mod profile;
pub mod queue;
pub mod queue_end;
pub mod recent;
pub mod tables;

pub use dev::{OnlineOptions, OnlineTick, connect, settle};

use mvs_sim::ai::bt::BtBot;
use mvs_sim::{InputState, World};

/// An online match's tick source: the dev runs' relay match (`--relay`), or a match of the menus' session.
#[derive(Debug)]
pub enum Online {
    Dev(Box<dev::DevOnline>),
    Net(Box<net_match::NetMatch>),
}

impl Online {
    /// The stage the match loaded (the desync report's header).
    pub fn set_stage_key(&mut self, key: &str) {
        match self {
            Online::Dev(o) => o.set_stage_key(key),
            Online::Net(o) => o.set_stage_key(key),
        }
    }

    /// The match reached its own end (its results).
    pub fn ended(&mut self) {
        match self {
            Online::Dev(o) => o.ended(),
            Online::Net(o) => o.ended(),
        }
    }

    /// Why the match stopped early, once.
    pub fn note(&mut self) -> Option<String> {
        match self {
            Online::Dev(o) => o.note(),
            Online::Net(o) => o.note(),
        }
    }

    /// The start gate, each frame from the load to the first tick: true once the match runs.
    pub fn gate(&mut self, world: &World) -> crate::Result<bool> {
        match self {
            Online::Dev(o) => o.gate(world),
            Online::Net(o) => o.gate(world),
        }
    }

    /// One 60 Hz tick of the match.
    pub fn tick(&mut self, w: &mut World, bots: &mut [Option<BtBot>], device: InputState) -> OnlineTick {
        match self {
            Online::Dev(o) => o.tick(w, bots, device),
            Online::Net(o) => o.tick(w, bots, device),
        }
    }

    /// (the port's own) The slots shown away, and the seconds left while this player's own link is down (None: it
    /// is not).
    pub fn away(&self) -> (Vec<usize>, Option<u32>) {
        match self {
            Online::Dev(_) => (Vec::new(), None),
            Online::Net(o) => o.away(),
        }
    }

    /// The match chat's tables: this player's slot, each slot's player id and name (None: a dev relay match, no
    /// chat).
    pub fn chat_table(&self) -> Option<(usize, Vec<String>, Vec<String>)> {
        match self {
            Online::Dev(_) => None,
            Online::Net(o) => Some(o.chat_table()),
        }
    }

    /// The match chats that came since the last call (sender id, to whom, text as sent).
    pub fn take_chats(&mut self) -> Vec<(String, mvs_protocol::messages::ChatTo, String)> {
        match self {
            Online::Dev(_) => Vec::new(),
            Online::Net(o) => o.take_chats(),
        }
    }

    /// A line of this player's match chat (MatchChat's Submit::Send), sent.
    pub fn send_chat(&mut self, text: &str, all: bool) {
        if let Online::Net(o) = self {
            o.send_chat(text, all);
        }
    }

    /// (the port's own) The ping line's number, ms (None: none measured yet).
    pub fn ping(&self) -> Option<f64> {
        match self {
            Online::Dev(o) => o.ping(),
            Online::Net(o) => o.ping(),
        }
    }

    /// The match is over for this player.
    pub fn finish(self) {
        match self {
            Online::Dev(o) => o.finish(),
            Online::Net(o) => o.finish(),
        }
    }
}
