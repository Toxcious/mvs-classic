//! What the net service uses of a backend: the lobbies, the links to other players and their
//! packets, the invites, the presence and the lobbies a relay hosts. Epic Online Services, a relay and LAN each
//! implement it; `memory` implements it in one process for the tests. Ids are the backend's player and lobby ids, as
//! text.
//!
//! The service drives a backend from its own thread: a call that waits for an answer returns an `Ask` at once and its
//! answer comes back from `poll` as `BackendEvent::Answered`, with every other event, in the order the backend made
//! them.

pub mod eos;
pub mod memory;

use indexmap::IndexSet;

pub use mvs_protocol::relay_wire::{Listed, Presence as RelayPresence, Who};

/// Which backend: the relay, Epic Online Services, or LAN (a direct host's own and its joiners').
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Relay,
    Epic,
    Lan,
}

impl BackendKind {
    /// The kind's id ('relay', 'epic', 'lan').
    pub fn id(self) -> &'static str {
        match self {
            BackendKind::Relay => "relay",
            BackendKind::Epic => "epic",
            BackendKind::Lan => "lan",
        }
    }
}

/// The P2P channels: messages on a reliable, ordered one, a match's packets on an unreliable, unordered one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    Messages = 0,
    Match = 1,
}

/// EOS_P2P_MAX_PACKET_SIZE: the most bytes one P2P packet carries.
pub const P2P_MAX_PACKET: usize = 1170;

/// EOS_ERelayControl as sessions ask for it: Force (EOS_RC_ForceRelays) every link through Epic's relays, so no peer
/// sees this PC's address; Allow (EOS_RC_AllowRelays) a direct link where the two PCs' routers let EOS make one,
/// Epic's relays otherwise. A link is direct only when both sides allow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayMode {
    Allow,
    Force,
}

/// A listed lobby a search found: its owner, how many members it has of its `max`, its `at` (ms since 1970: when
/// its player began to queue, or made the lobby; 0 none), its `info` text (a custom lobby's listing, anyone's text
/// until read) and its `code` (the code its host wrote, anyone's text until read; '' none).
#[derive(Clone, Debug, PartialEq)]
pub struct FoundLobby {
    pub id: String,
    pub owner: String,
    pub members: u32,
    pub max: u32,
    pub at: f64,
    pub info: String,
    pub code: String,
}

/// The most bytes of UTF-8 of a listed lobby's `info`.
pub const INFO_BYTES: usize = 1000;
/// The most bytes of a listed lobby's `code`.
pub const CODE_BYTES: usize = 64;

/// A change to a lobby this player is in: who is in it now, or that it is gone for this player (destroyed by its
/// owner, this player kicked or left, or the link to the backend lost).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyChange {
    Members { lobby: String, members: Vec<String> },
    Closed { lobby: String, why: String },
}

/// A call whose answer comes later (`BackendEvent::Answered`), by its number on this backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ask(pub u32);

/// What a call came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// The call is done (set_listed, leave_lobby, invite, a presence's set, shutdown).
    Done,
    /// create_lobby: the lobby's id.
    Created(String),
    /// host_lobby: the lobby's id and its host core's (`relay_wire::host_id`), which its members link to; `short` the
    /// relay's short code of it (`relay_wire::Created::short`; None: the relay gave none).
    Hosted { lobby: String, host: String, short: Option<String> },
    /// search_lobbies.
    Found(Vec<FoundLobby>),
    /// join_lobby: its owner and its members; `lobby` the lobby's id when the join named it by its short code
    /// (`relay_wire::Joined::lobby`).
    Joined { owner: String, members: Vec<String>, lobby: Option<String> },
    /// A presence's who.
    Who(Vec<Who>),
}

/// What a backend tells the service, in the order it happened.
#[derive(Clone, Debug, PartialEq)]
pub enum BackendEvent {
    Lobby(LobbyChange),
    /// A packet from another player, in the order they came.
    Packet {
        from: String,
        channel: Channel,
        bytes: Vec<u8>,
    },
    /// The link to another player is closed, by either side or by the backend (gone silent).
    PeerClosed(String),
    /// A link to a peer stands (relayed or direct), or (None) the link that stood is closed.
    Link {
        peer: String,
        relayed: Option<bool>,
    },
    /// A custom invite from another player.
    Invite {
        from: String,
        payload: String,
    },
    /// The answer to a call: its value, or the backend's reason in words (EOS's result name, the relay's refusal).
    Answered(Ask, Result<Answer, String>),
    /// The listing of a lobby this player owns and the backend hosts changed (`info` its text while public, None
    /// while private): what a LAN host announces.
    Listing {
        lobby: String,
        info: Option<String>,
    },
    /// A backend loaded signing in (EOS) is signed in: its `me` and its calls hold from here on.
    SignedIn,
    /// The sign-in was lost and could not be refreshed, or (a backend loaded signing in) it failed: online is off
    /// until the program starts again.
    SignedOut(String),
}

/// A signed-in backend. Every call is made on the service's thread.
pub trait Backend: Send {
    /// This player's id on the backend.
    fn me(&self) -> &str;
    fn kind(&self) -> BackendKind;
    /// A relay's sign-in: the game protocol of the cores it hosts, as its welcome said (None: it did not say, or not a
    /// relay).
    fn relay_proto(&self) -> Option<u32> {
        None
    }
    /// A lobby of `max` members owned by this player, no host migration, joined by its id; answered `Created`.
    /// `listed`: searches find it (public), with its attributes; otherwise invite-only, found by its id alone.
    fn create_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Ask;
    /// A lobby as `create_lobby` makes it whose session the backend hosts (a relay's host core, wire 5): answered
    /// `Hosted`, its owner a member like the others. None: this backend hosts no lobby (Epic: the owner's process
    /// hosts it).
    fn host_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Option<Ask> {
        let _ = (bucket, max, listed);
        None
    }
    /// The listed lobbies of `bucket` with a free place (`full`: and those without), at most `limit`, none of this
    /// player's own; answered `Found`.
    fn search_lobbies(&mut self, bucket: &str, limit: u32, full: bool) -> Ask;
    /// The owner lists its lobby with these attributes, or (None) takes it out of the searches; answered `Done`.
    fn set_listed(&mut self, id: &str, listed: Option<Listed>) -> Ask;
    /// Joins a lobby by its id; answered `Joined` (or the backend's reason: full, not found, ...).
    fn join_lobby(&mut self, id: &str) -> Ask;
    /// Leaves a lobby (its owner destroys it); answered `Done`.
    fn leave_lobby(&mut self, id: &str) -> Ask;
    /// The owner takes a member out.
    fn kick(&mut self, lobby: &str, member: &str);
    /// One packet (at most P2P_MAX_PACKET bytes) to another player; false when the backend refused it (its queue is
    /// full, no such player). The first send opens the link, which the other side accepts when its accept set holds
    /// this player.
    fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool;
    /// Ends the link to another player.
    fn close_peer(&mut self, peer: &str);
    /// Whether the link to a peer goes through relays (Epic's, as EOS said when it stood; a relay's and LAN's: true /
    /// false always); None while there is no link.
    fn relayed(&self, peer: &str) -> Option<bool>;
    /// The peers with a link that stands.
    fn linked_peers(&self) -> Vec<String>;
    /// The relay control of the links the backend opens from now on (a link that stands keeps the one it was opened
    /// under). Force from the sign-in. A relay and LAN do nothing.
    fn set_relay_mode(&mut self, mode: RelayMode) -> Result<(), String>;
    /// Who may open a link to this player (the members of its lobbies): a request from anyone else is refused.
    fn set_accept(&mut self, ids: IndexSet<String>);
    /// A custom invite with `payload` (less than 500 bytes of text) to another player; answered `Done`.
    fn invite(&mut self, to: &str, payload: &str) -> Ask;
    /// The online list (a relay of wire 3 alone; EOS and LAN have none).
    fn presence(&mut self) -> Option<&mut dyn Presence>;
    /// The smallest round trip to the backend's server measured so far (ms; a relay's ping and UDP probe), which a
    /// queue join carries; None: none measured, or a backend with no such server.
    fn rtt(&self) -> Option<f64> {
        None
    }
    /// The UDP link to the backend's server (a relay's: its probes acknowledged); None while not known yet, and for
    /// a backend with no such link (Epic, LAN, the tests' memory backend). Some(false): the relay is reached by TCP
    /// alone, which plays slower.
    fn udp(&self) -> Option<bool> {
        None
    }
    /// The next event the backend has by `now` (the service's clock, ms), None when it has none.
    fn poll(&mut self, now: f64) -> Option<BackendEvent>;
    /// When `poll` has something to do next (the service's clock, ms); None: only after a call. EOS sends and takes
    /// packets only inside the ticks its polls make.
    fn next_due(&self) -> Option<f64> {
        None
    }
    /// Leaves every lobby, releases the platform; answered `Done`.
    fn shutdown(&mut self) -> Ask;
}

/// A backend's online list.
pub trait Presence {
    /// Publishes this player's presence; answered `Done`.
    fn set(&mut self, p: RelayPresence) -> Ask;
    /// The listed players, at most `limit`; answered `Who`.
    fn who(&mut self, limit: u32) -> Ask;
}
