//! The host core: the rules of one online session, run by the player who hosts it: a custom lobby or
//! a queue lobby, the pre-match, a queue set's votes and rematch, a match's start gate, its end and who leaves it. It
//! rules no input and sees none: it tells the match's referee when a match goes, who leaves it and when it ends. No
//! game data, no file and no socket: players come as links, the time as the `now` each call is given, and the core's
//! own deadlines run in `Core::pump`.

pub mod bucket;
pub mod core;
pub mod draws;
pub mod link;
mod lobby;
mod path;
mod play;
mod prematch;
mod queue;
pub mod rand;
pub mod timers;

pub use crate::core::Core;
pub use crate::link::{ConnKey, Connection, MatchReferee, RefereeHuman, RefereeStart, STEADY_CATCH_UP};
pub use crate::rand::{HostRand, SystemRand};

use mvs_protocol::match_wire::MAX_MATCH_TICKS;
pub use mvs_protocol::messages::HostWhy;
use mvs_protocol::queue_rules::{PM_PERKS_MS, PM_PICK_MS, PREMATCH_MS, REMATCH_MS, RESULT_MS, VOTE_MS};

/// A custom lobby (the host's player makes it and leads it; the others join it) or a queue lobby (its members queue,
/// and the queue's rules make their match), or the relay's open queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionKind {
    Custom,
    Queue,
    /// The relay's queue core: nobody hosts it (`HostOptions::host` ""): every join is a full ticket, no guest is let
    /// go when a host stops waiting, and no leave ends the session; its matches are many at once.
    OpenQueue,
}

/// The times the rules wait, in ms (`match_ticks` in ticks): the build's by default, shorter in the tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostTimes {
    /// The ticks a match is played for at the most (MAX_MATCH_TICKS, 2 hours).
    pub match_ticks: u32,
    /// The start gate: a match whose peers have not all reported their level loaded by then is ended for all of
    /// them, the late ones named as leavers.
    pub load_ms: f64,
    /// A found queue match starts at the latest this long after queue.found.
    pub queue_start_ms: f64,
    pub pick_ms: f64,
    pub perks_ms: f64,
    pub result_ms: f64,
    pub vote_ms: f64,
    pub rematch_ms: f64,
}

pub const HOST_TIMES: HostTimes = HostTimes {
    match_ticks: MAX_MATCH_TICKS,
    load_ms: 300000.0,
    queue_start_ms: PREMATCH_MS,
    pick_ms: PM_PICK_MS,
    perks_ms: PM_PERKS_MS,
    result_ms: RESULT_MS,
    vote_ms: VOTE_MS,
    rematch_ms: REMATCH_MS,
};

impl Default for HostTimes {
    fn default() -> HostTimes {
        HOST_TIMES
    }
}

/// A log line's sink (the console's line with the time).
pub type HostLog = Box<dyn FnMut(&str) + Send>;

/// How a session is made.
pub struct HostOptions {
    pub kind: SessionKind,
    /// The hosting player's id: their link's close (or, in a custom session, their lobby.leave) ends the session.
    pub host: String,
    /// The name in the `server` message (None: "MVS Classic").
    pub name: Option<String>,
    /// A custom session's lobby id (lobby.state's `id`, what a joiner's lobby.join names); None: a random one.
    pub lobby_id: Option<String>,
    pub times: HostTimes,
    /// Who referees the session's matches (None: a match goes with no referee, the tests' sessions).
    pub referee: Option<Box<dyn MatchReferee>>,
    /// The core's draws (ids, seeds, maps, bot picks and gaps).
    pub rand: Box<dyn HostRand>,
    pub log: Option<HostLog>,
    /// The core measures its matches' paths (`mvs_protocol::path`) and tells each 2+ human match its path: the
    /// relay server's cores alone (a hosting app's, a LAN direct host's and the tapes' never).
    pub paths: bool,
}

/// A player of the session who said hello with a name that was free. `busy`: in a match, the queue, a pre-match or
/// a queue set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerInfo {
    pub id: String,
    pub name: String,
    pub busy: bool,
}

/// What the core tells its owner, drained by `Core::take_events` after each call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEvent {
    /// The session is over (the host left, or `end` was called): every link is closed.
    End(HostWhy),
    /// The custom lobby's leader kicked a member (its lobby.kicked sent first): the owner takes it out of the
    /// backend's lobby too. The core keeps the link until the owner closes it.
    Kicked(String),
    /// A queue session's match is found (queue.found sent): its humans' ids.
    Found(Vec<String>),
    /// A custom session's listing changed: its text while the lobby is public, None while it is private or there is
    /// none. Sent at the end of the step that changed it.
    Listing(Option<String>),
}
