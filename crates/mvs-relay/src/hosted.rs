//! The host cores a relay runs itself (online plan section 4): one queue core and one core per lobby made with `host`
//! (`relay_wire::host_id`). A player links to a core as to a peer, by the `p2p_frame` rules on channel 0; the cores'
//! matches are refereed in [`crate::core`].
//!
//! A core is never called from inside one of its own calls: what its links and its referee do in a call is queued in
//! its [`Outbox`] and carried out, in order, after the call returns (then the next call, if that made one). A panic in
//! a core's call ends that core alone.
//!
//! A link is the relay's on its own side: it is opened by a knock (`FRAME_OPEN`), carries the core's messages split
//! into channel-0 pieces from the host's id, and ends by a bye either way (`FRAME_BYE`, `p2p_frame::host_bye` from the
//! core), the player's closePeer, its leave of the host's lobby or its seat's end. One connection's frames come in the
//! order they were sent, so a knock after a bye is a new link (no relink wait, as `mvs-net`'s Hosting keeps over EOS).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use indexmap::IndexMap;
use mvs_host::{
    ConnKey, Connection, HOST_TIMES, HostEvent, HostRand, HostTimes, HostWhy, MatchReferee, RefereeStart, SystemRand,
};
use mvs_protocol::p2p_frame::MessageJoiner;
use mvs_protocol::relay_wire::referee_id;

/// What a relay's host cores are (`server::Options::hosted`, `Core::new`).
pub struct HostedOptions {
    /// The relay runs a queue core ([`mvs_protocol::relay_wire::QUEUE_HOST`]); false: a direct host's, which hosts
    /// its lobby alone.
    pub queue: bool,
    pub times: HostTimes,
    /// Each new core's draws.
    pub rand: Box<dyn FnMut() -> Box<dyn HostRand>>,
    /// The cores measure their matches' paths (`HostOptions::paths`): the relay server's.
    pub paths: bool,
}

impl HostedOptions {
    /// The build's times and the system's draws; a queue's relay (the relay server's) measures paths.
    pub fn system(queue: bool) -> HostedOptions {
        HostedOptions { queue, times: HOST_TIMES, rand: Box::new(|| Box::new(SystemRand)), paths: queue }
    }
}

impl std::fmt::Debug for HostedOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostedOptions")
            .field("queue", &self.queue)
            .field("times", &self.times)
            .field("paths", &self.paths)
            .finish()
    }
}

/// What a host core's links and referee did in one of its calls, carried out after it, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum HostOut {
    /// A message to a player (split into channel-0 pieces from the host's id).
    Send { to: String, text: String },
    /// The core closed a player's link: its bye (`p2p_frame::host_bye`) sent, the link let go.
    Close { to: String, why: HostWhy },
    /// A match goes: its referee opened on the relay under `referee_id(host, match_id)`.
    Open { m: RefereeStart },
    /// A human left a match: its stream ends at the ruled line.
    Left { match_id: u32, slot: u32 },
    /// The match is over for the core: its referee goes.
    End { match_id: u32 },
    /// A line of the core's log.
    Log(String),
}

/// A core's queued outputs (its links and referee are `Send`, so the queue is behind a lock no other thread takes).
pub type Outbox = Arc<Mutex<Vec<HostOut>>>;

/// The outbox, whatever a panic left in it.
pub fn outbox(o: &Outbox) -> MutexGuard<'_, Vec<HostOut>> {
    o.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A player's link to a host core, as the core holds it.
pub struct RelayConn {
    pub id: String,
    pub name: String,
    pub out: Outbox,
    /// Closed by the core (a second close does nothing).
    pub closed: bool,
}

impl Connection for RelayConn {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn send(&mut self, text: &str) -> Result<(), String> {
        if !self.closed {
            outbox(&self.out).push(HostOut::Send { to: self.id.clone(), text: text.to_owned() });
        }
        Ok(())
    }

    fn close(&mut self, why: HostWhy) {
        if !self.closed {
            self.closed = true;
            outbox(&self.out).push(HostOut::Close { to: self.id.clone(), why });
        }
    }
}

/// The referee of a host core's matches: the relay's own, by its host id.
pub struct RelayReferee {
    pub host: String,
    pub out: Outbox,
    /// The matches of every core the relay referees or is about to (one started is counted at once, before its
    /// `HostOut::Open` is carried out; `Core` counts one ended), shared by all its cores, and the relay's `matches`.
    pub running: Arc<AtomicUsize>,
    pub limit: usize,
}

impl RelayReferee {
    /// One more match may be found or started: the matches running, those `pending` of the asking core (found, not
    /// started yet) and it under the relay's limit.
    pub fn has_room(&self, pending: usize) -> bool {
        self.running.load(Ordering::Relaxed) + pending < self.limit
    }
}

impl MatchReferee for RelayReferee {
    fn start(&mut self, m: &RefereeStart, _now: f64) -> String {
        self.running.fetch_add(1, Ordering::Relaxed);
        outbox(&self.out).push(HostOut::Open { m: m.clone() });
        referee_id(&self.host, m.match_id)
    }

    fn left(&mut self, match_id: u32, slot: u32) {
        outbox(&self.out).push(HostOut::Left { match_id, slot });
    }

    fn end(&mut self, match_id: u32) {
        outbox(&self.out).push(HostOut::End { match_id });
    }

    fn room(&self, pending: usize) -> bool {
        self.has_room(pending)
    }
}

/// A player's link to a host core as the relay keeps it: the core's key for it, its channel-0 joiner, and whether it
/// is over (the core closed it, its bye sent: what the player sends on it is dropped until its closePeer or a new
/// knock, which opens a new link).
pub struct HostLink {
    pub key: ConnKey,
    pub joiner: MessageJoiner,
    pub done: bool,
}

/// What a core's call left to carry out, in order: its outputs, then its events.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Out(HostOut),
    Event(HostEvent),
}

/// One host core the relay runs. `core` is None while one of its calls runs, and once it ended (or panicked); `lobby`
/// its lobby (None: the queue's, or a test's).
pub struct Hosted {
    pub core: Option<mvs_host::Core>,
    pub out: Outbox,
    /// The links by player id.
    pub links: IndexMap<String, HostLink>,
    pub lobby: Option<String>,
    /// What its calls left to carry out, and whether that is under way (a call made meanwhile adds to it).
    pub steps: VecDeque<Step>,
    pub draining: bool,
    /// The core is over (its End, or a panic): it goes once its steps are carried out.
    pub over: bool,
    /// A lobby's members with no link, by when the relay kicks them ([`crate::core::HOST_KNOCK_WAIT_MS`]).
    pub unlinked: IndexMap<String, f64>,
    /// When each player's last link to it ended ([`crate::core::HOST_RELINK_MS`]).
    pub left_at: IndexMap<String, f64>,
}

impl Hosted {
    pub fn new(core: mvs_host::Core, out: Outbox, lobby: Option<String>) -> Hosted {
        Hosted {
            core: Some(core),
            out,
            links: IndexMap::new(),
            lobby,
            steps: VecDeque::new(),
            draining: false,
            over: false,
            unlinked: IndexMap::new(),
            left_at: IndexMap::new(),
        }
    }
}
