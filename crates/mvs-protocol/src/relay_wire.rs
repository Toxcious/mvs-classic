//! The relay's wire (protocol/relay-wire.ts): between a player's net thread and a relay (`mvs-relay`, or the core
//! inside a direct host). It carries lobbies, searches, invites, and packets between two players on a reliable channel
//! (0) and an unreliable one (1).
//! - TCP: one connection per player: frames of a 4-byte big-endian length (of what follows it), a type byte and a
//!   body: [`FT_JSON`] a control message, UTF-8 JSON `{ t, ... }` ([`ClientMessage`] / [`ServerMessage`]);
//!   [`FT_DATA`] a channel 0 packet: a 4-byte peer handle, then the packet; [`FT_MATCH`] a channel 1 packet when UDP
//!   does not get through: a 4-byte peer handle, then the packet.
//! - UDP: channel 1: the 8-byte session token (the welcome's), then the body ([`read_udp_body`]): a kind byte, then
//!   for [`UDP_MATCH`] a 4-byte peer handle and the packet; for [`UDP_PROBE`] one flag byte, 1 when the client has had
//!   a [`UDP_PROBE_ACK`] in the last [`UDP_HEARD_MS`] (the client's datagram that tells the relay its UDP address);
//!   [`UDP_PROBE_ACK`] nothing. The relay sends a client channel 1 by UDP only while that client's last probe is under
//!   [`UDP_FRESH_MS`] old, had its flag set and came from the TCP connection's IP ([`normal_address`]); else as
//!   [`FT_MATCH`] on TCP. A datagram from another IP than its token's connection is dropped.
//! - Sealed (wire 7, [`crate::relay_seal`]): a TCP frame's type byte and body are sealed and its tag follows them (the
//!   length counts it); a datagram's body is sealed behind the token and a counter, its tag last.
//!
//! The connection's start (wire 7, the port's own from here on): hello (the client's key, a nonce and an X25519 key
//! of its own, `kx`) -> challenge (the relay's key, nonce and `kx`, and the relay's signature) -> prove (the client's
//! signature) -> welcome (the client's id, the UDP token and port) or refused { why }. Both signatures cover the
//! transcript T = client key (32) || relay key (32) || client nonce (32) || relay nonce (32) || client kx (32) ||
//! relay kx (32), behind a context of the signer's side ([`handshake_bytes`]), so neither signature passes as the
//! other, a prove made for one relay is refused by another, and the exchange's keys are the ones the signers sent.
//! Each side then has the X25519 secret of the two kx, and the two keys of the connection derived from it and T
//! ([`crate::relay_seal`]): what the relay sends after its challenge and what the client sends after its prove is
//! sealed with them; the hello, the challenge and a refusal before the challenge are not. Ids are `r` + the first 32
//! hex digits of the SHA-256 of the Ed25519 public key ([`key_id`]). A key that is of small order, not canonical, or
//! not a point is refused ([`bad_public_key`]); a kx whose secret is all zero (a point of small order) too.
//!
//! Peer handles: each side numbers the peers it sends to on its own, increasing, and never uses a number twice in one
//! connection (peer { handle, id } before the first packet to that id, on TCP). A handle is freed when its link closes
//! (closePeer or peerClosed, sent or received), so at most [`MAX_PEER_HANDLES`] are live each way. A packet on a freed
//! or unknown handle is dropped. The first channel 1 packet on a new handle goes on TCP behind its peer message. Requests
//! carry an id; the answer (res) carries the same one.
//!
//! Hosted sessions (wire 5): the relay runs host cores (`mvs_host`) itself: one queue core ([`QUEUE_HOST`]) and one
//! core per lobby made with `host` ([`host_id`] of the lobby). A player links to a host as to any peer, on channel 0
//! with the `p2p_frame` rules (FRAME_OPEN knocks, a message split into pieces, FRAME_BYE ends it); a lobby's host takes
//! its members only, the queue's anyone. The cores' matches are refereed on the relay ([`referee_id`] of the host id).
//!
//! Types, constants and pure functions only: no sockets.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The wire's version: a relay or a client of another one is refused with the reason.
pub const RELAY_WIRE_VERSION: u32 = 7;
/// (wire 6) A hosted lobby's short code: this many characters of [`SHORT_ALPHABET`], given at its create and kept by
/// the relay for the lobby's life; a join may name the lobby by it.
pub const SHORT_LEN: usize = 6;
/// The short code's characters: lower case letters and digits without the look-alikes (i, l, o, 0, 1); 31^6, about
/// 887 million codes.
pub const SHORT_ALPHABET: &[u8; 31] = b"abcdefghjkmnpqrstuvwxyz23456789";
/// The port a relay or a direct host listens on unless told otherwise, for TCP and UDP.
pub const RELAY_PORT: u16 = 7777;
/// The most bytes a TCP frame's length may say (type byte and body): a longer one ends the connection.
pub const MAX_FRAME: usize = 1 << 16;
/// What each side's signature covers before the transcript (the NUL included).
pub const SIGN_RELAY: &[u8] = b"mvs-relay-v2:relay\0";
pub const SIGN_CLIENT: &[u8] = b"mvs-relay-v2:client\0";
/// The most peer handles live at once in each direction of one connection.
pub const MAX_PEER_HANDLES: usize = 256;
/// A probe's flag says the client heard an acknowledgement this recently (ms).
pub const UDP_HEARD_MS: f64 = 4000.0;
/// The relay sends by UDP while the client's last probe (flag set) is this recent (ms).
pub const UDP_FRESH_MS: f64 = 3000.0;
/// A client that has taken nothing by UDP for this long (ms) sends its channel 1 over TCP.
pub const UDP_LATE_MS: f64 = 300.0;
/// The client sends a UDP probe this often (ms).
pub const UDP_PROBE_MS: f64 = 1000.0;
pub const NONCE_BYTES: usize = 32;
pub const TOKEN_BYTES: usize = 8;
/// The client pings this often (ms); a connection that hears nothing for [`SILENT_MS`] is dropped by either side.
pub const PING_MS: f64 = 5000.0;
pub const SILENT_MS: f64 = 20000.0;
/// The hello and prove must come within this of the connection's start (ms).
pub const HELLO_MS: f64 = 10000.0;

pub const FT_JSON: u8 = 1;
pub const FT_DATA: u8 = 2;
pub const FT_MATCH: u8 = 3;

pub const UDP_MATCH: u8 = 1;
pub const UDP_PROBE: u8 = 2;
pub const UDP_PROBE_ACK: u8 = 3;

/// A listed lobby's `info`, at most this many bytes of UTF-8.
pub const RELAY_INFO_BYTES: usize = 1000;
/// A listed lobby's `code`, at most this many bytes of UTF-8.
pub const RELAY_CODE_BYTES: usize = 64;
/// The most players one 'who' gives.
pub const WHO_LIMIT: u32 = 200;
/// The id of the relay's queue core: a queuer links to it ([`is_host_id`]).
pub const QUEUE_HOST: &str = "queue";

// ---- the JSON control messages (field names as the TS writes them)

/// `RelayListed`: a listed lobby's attributes (a relay of before `code` drops the field, and a client reads a found
/// lobby without one as '').
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    pub at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// `RelayPresence` (wire 3): a player's presence. `busy` in a match, a queue or a pre-match; `listed` the online list
/// shows them; `invites` false: an invite to them is answered as one to an id that is not online; `status` (the
/// port's own, in wire 7 with no new version) what they are doing, for the online list's line under the name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presence {
    pub name: String,
    pub icon: String,
    pub busy: bool,
    pub listed: bool,
    pub invites: bool,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "status_or_none")]
    pub status: Option<PresenceStatus>,
}

/// `RelayWho`: a player of a 'who' answer, never an address. `status` as their presence gave it (None: none given,
/// a relay of before it, or a status this build does not know).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Who {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub busy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "status_or_none")]
    pub status: Option<PresenceStatus>,
}

/// (the port's own) What a player is doing, as the online list shows it under their name. On the wire a short word
/// ([`PresenceStatus::id`]); the field is optional both ways and an unknown word is read as no status, so a relay or a
/// client of before it, or of a later build with more of them, still takes the presence and the list. The list is
/// pulled ('who'), never pushed: the status costs a few bytes per row per pull.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PresenceStatus {
    /// In the menus, in no lobby, queue or match (a party's member too).
    #[serde(rename = "menus")]
    Menus,
    /// In a custom lobby, before its match.
    #[serde(rename = "lobby")]
    Lobby,
    /// On the queue's screen, searching or about to.
    #[serde(rename = "queue")]
    Queue,
    /// In a match, from its pre-match to its end (a queue set's end screen too), by its shape.
    #[serde(rename = "1v1")]
    OneVOne,
    #[serde(rename = "2v2")]
    TwoVTwo,
    #[serde(rename = "ffa")]
    Ffa,
    /// A match of another shape (a custom lobby's two against one).
    #[serde(rename = "match")]
    Match,
}

impl PresenceStatus {
    pub const ALL: [PresenceStatus; 7] = [
        PresenceStatus::Menus,
        PresenceStatus::Lobby,
        PresenceStatus::Queue,
        PresenceStatus::OneVOne,
        PresenceStatus::TwoVTwo,
        PresenceStatus::Ffa,
        PresenceStatus::Match,
    ];

    /// The wire word.
    pub fn id(self) -> &'static str {
        match self {
            PresenceStatus::Menus => "menus",
            PresenceStatus::Lobby => "lobby",
            PresenceStatus::Queue => "queue",
            PresenceStatus::OneVOne => "1v1",
            PresenceStatus::TwoVTwo => "2v2",
            PresenceStatus::Ffa => "ffa",
            PresenceStatus::Match => "match",
        }
    }

    /// A wire word read back (None: not one of this build's).
    pub fn of(s: &str) -> Option<PresenceStatus> {
        PresenceStatus::ALL.into_iter().find(|p| p.id() == s)
    }

    /// Not free to join: anything but the menus.
    pub fn busy(self) -> bool {
        self != PresenceStatus::Menus
    }
}

/// A status field read leniently: a word this build does not know, or a value of another type, is no status.
fn status_or_none<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<PresenceStatus>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str().and_then(PresenceStatus::of))
}

/// `RelayRequest`: the lobby calls a client makes, tagged by `op`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Request {
    /// `host`: the relay hosts the lobby's session (a host core of its own, [`host_id`] of the lobby; the answer's
    /// `host`); false: a party's lobby, which only carries packets.
    #[serde(rename = "create")]
    Create {
        bucket: String,
        max: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        listed: Option<Listed>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        host: bool,
    },
    #[serde(rename = "search")]
    Search {
        bucket: String,
        limit: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full: Option<bool>,
    },
    /// `listed` null: found by its id alone.
    #[serde(rename = "list")]
    List { lobby: String, listed: Option<Listed> },
    /// `lobby`: its id, or (wire 6) a hosted lobby's short code.
    #[serde(rename = "join")]
    Join { lobby: String },
    #[serde(rename = "leave")]
    Leave { lobby: String },
    #[serde(rename = "kick")]
    Kick { lobby: String, member: String },
    #[serde(rename = "invite")]
    Invite { to: String, payload: String },
    /// (wire 3) what the online list shows of this player: a connection's first presence makes them known, each later
    /// one replaces it; the connection's end takes it out.
    #[serde(rename = "presence")]
    Presence { presence: Presence },
    /// (wire 3) the listed players but the asker, at most `limit` ([`WHO_LIMIT`]), by name.
    #[serde(rename = "who")]
    Who { limit: u32 },
}

/// A lobby a search found (`RelayAnswers.search`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FoundLobby {
    pub id: String,
    pub owner: String,
    pub members: u32,
    pub max: u32,
    pub at: f64,
    pub info: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// `RelayAnswers.create`. `host`: the id of the lobby's host core ([`host_id`]) when the create asked for one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Created {
    pub lobby: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// (wire 6) The hosted lobby's short code ([`SHORT_LEN`] characters of [`SHORT_ALPHABET`]); None for a lobby with
    /// no host core.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
}

/// `RelayAnswers.search`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub lobbies: Vec<FoundLobby>,
}

/// `RelayAnswers.join`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Joined {
    pub owner: String,
    pub members: Vec<String>,
    /// (wire 6) The lobby's id when the join named it by its short code (None: it named the id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lobby: Option<String>,
}

/// `RelayAnswers.who`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhoAnswer {
    pub players: Vec<Who>,
}

/// `RelayClientMessage`, tagged by `t`. Keys, nonces, signatures and tokens are standard base64 with padding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum ClientMessage {
    #[serde(rename = "hello")]
    /// The resume rule (hello and welcome): both sides keep a ring of the channel-0 data frames they sent, as (peer
    /// id, bytes). `got` is the count of channel-0 data frames this client took on its seat (None: a fresh sign-in).
    /// Each side resends from the other's count; a ring that no longer covers the gap fails the resume (treated as
    /// None).
    Hello {
        v: u32,
        key: String,
        nonce: String,
        /// (wire 7) The client's X25519 public key for this connection.
        kx: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        got: Option<u64>,
    },
    #[serde(rename = "prove")]
    Prove { sig: String },
    #[serde(rename = "req")]
    Req {
        id: i64,
        #[serde(flatten)]
        req: Request,
    },
    #[serde(rename = "peer")]
    Peer { handle: u32, id: String },
    #[serde(rename = "closePeer")]
    ClosePeer { id: String },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "pong")]
    Pong,
    /// A leave on purpose: the seat is not kept for a resume.
    #[serde(rename = "bye")]
    Bye,
}

/// `RelayServerMessage`, tagged by `t`. The TS's two `res` shapes are one variant (`ok` true: `value`; false:
/// `error`), as are its two `lobby` ones (`members`, or `closed`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum ServerMessage {
    #[serde(rename = "challenge")]
    Challenge {
        v: u32,
        nonce: String,
        key: String,
        /// (wire 7) The relay's X25519 public key for this connection.
        kx: String,
        sig: String,
    },
    #[serde(rename = "welcome")]
    /// `resumed` is the count of channel-0 data frames the relay took from the seat (None: a new seat, and the old
    /// lobbies and links are gone); see the resume rule at [`ClientMessage::Hello`]. `proto` (the port's own, no
    /// wire version of its own: an older relay sends none, an older client reads past it) is the game protocol
    /// (`version::PROTOCOL`) of the cores the relay hosts.
    Welcome {
        id: String,
        token: String,
        udp: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resumed: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        proto: Option<u32>,
    },
    #[serde(rename = "refused")]
    Refused { why: String },
    #[serde(rename = "res")]
    Res {
        id: i64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    #[serde(rename = "lobby")]
    Lobby {
        lobby: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        members: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        closed: Option<String>,
    },
    #[serde(rename = "peer")]
    Peer { handle: u32, id: String },
    #[serde(rename = "peerClosed")]
    PeerClosed { id: String },
    #[serde(rename = "invite")]
    Invite { from: String, payload: String },
    /// To the owner of a lobby the relay hosts: its core's listing changed (`info` the listing's text while the lobby
    /// is public, None while it is private): what a LAN host announces.
    #[serde(rename = "listing")]
    Listing {
        lobby: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        info: Option<String>,
    },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "pong")]
    Pong,
}

/// `refereeId`: the id of the referee of a host core's match on a relay, `owner` the core's host id (a packet address
/// only; never a player's: a player's id is `r` + hex).
pub fn referee_id(owner: &str, r#match: u32) -> String {
    format!("{owner}.m{match}")
}

/// The id of the host core of a lobby the relay hosts (a packet address only).
pub fn host_id(lobby: &str) -> String {
    format!("{lobby}.host")
}

/// Whether an id is a host core's: [`QUEUE_HOST`] or a lobby's [`host_id`] (a lobby id of letters and digits). A
/// referee's id is not one.
pub fn is_host_id(id: &str) -> bool {
    id == QUEUE_HOST
        || id.strip_suffix(".host").is_some_and(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric()))
}

// ---- frames

/// A frame's length past [`MAX_FRAME`] (`frame` throws).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("relay-wire: frame too long")]
pub struct FrameTooLong;

/// `frame`: a frame's bytes.
pub fn frame(ty: u8, body: &[u8]) -> Result<Vec<u8>, FrameTooLong> {
    if body.len() + 1 > MAX_FRAME {
        return Err(FrameTooLong);
    }
    let mut out = Vec::with_capacity(5 + body.len());
    // (the length is under MAX_FRAME, 1 << 16)
    out.extend_from_slice(&u32::try_from(body.len() + 1).map_err(|_| FrameTooLong)?.to_be_bytes());
    out.push(ty);
    out.extend_from_slice(body);
    Ok(out)
}

/// `jsonFrame`: a control message's frame (`JSON.stringify` of it, as UTF-8).
pub fn json_frame<M: Serialize>(m: &M) -> Result<Vec<u8>, FrameTooLong> {
    frame(FT_JSON, &serde_json::to_vec(m).expect("a control message serialises"))
}

/// `packetFrame`: a packet frame ([`FT_DATA`] or [`FT_MATCH`]) for a peer handle.
pub fn packet_frame(ty: u8, handle: u32, bytes: &[u8]) -> Result<Vec<u8>, FrameTooLong> {
    let mut body = Vec::with_capacity(4 + bytes.len());
    body.extend_from_slice(&handle.to_be_bytes());
    body.extend_from_slice(bytes);
    frame(ty, &body)
}

/// A packet body's handle and bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    pub handle: u32,
    pub bytes: &'a [u8],
}

/// `readPacket`: `None` when the body is shorter than a handle.
pub fn read_packet(body: &[u8]) -> Option<Packet<'_>> {
    let (head, bytes) = body.split_first_chunk::<4>()?;
    Some(Packet { handle: u32::from_be_bytes(*head), bytes })
}

/// `readJson`: a JSON body as an object with a string `t`; `None` when it is not UTF-8 JSON of that shape. The typed
/// messages are read from it with `serde_json::from_value`, so a caller can answer a request whose fields are wrong.
pub fn read_json(body: &[u8]) -> Option<serde_json::Map<String, serde_json::Value>> {
    // (TextDecoder drops a leading byte order mark)
    let body = body.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(body);
    match serde_json::from_slice::<serde_json::Value>(body).ok()? {
        serde_json::Value::Object(o) if o.get("t").is_some_and(serde_json::Value::is_string) => Some(o),
        _ => None,
    }
}

/// One frame of a TCP stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub ty: u8,
    pub body: Vec<u8>,
}

/// `FrameReader`: splits a TCP stream into frames: push each chunk, take the frames it completed. A length over
/// [`MAX_FRAME`], or of 0, makes `bad` true and nothing more is taken (the caller ends the connection).
#[derive(Clone, Debug, Default)]
pub struct FrameReader {
    pub bad: bool,
    // The bytes not yet taken are buf[start..end].
    buf: Vec<u8>,
    start: usize,
    end: usize,
}

impl FrameReader {
    pub fn new() -> FrameReader {
        FrameReader::default()
    }

    /// The frames `chunk` completed, in order ([`FrameReader::feed`], then [`FrameReader::next`] until it has none).
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Frame> {
        self.feed(chunk);
        let mut out = Vec::new();
        while let Some(f) = self.next() {
            out.push(f);
        }
        out
    }

    /// Takes a chunk of the stream in; its frames come out of [`FrameReader::next`].
    pub fn feed(&mut self, chunk: &[u8]) {
        if self.bad {
            return;
        }
        // (the taken bytes go before the new ones come in: each byte is copied once more at most)
        self.buf.drain(..self.start);
        self.end -= self.start;
        self.start = 0;
        self.buf.truncate(self.end);
        self.buf.extend_from_slice(chunk);
        self.end = self.buf.len();
    }

    /// The next whole frame fed, if any. A bad length sets `bad` and drops what is held. One frame at a time, so that
    /// the reader of the stream may change at a frame (the sealed wire after the start: [`FrameReader::take_rest`]).
    #[allow(clippy::should_implement_trait, reason = "a reader fed between calls, not an iterator")]
    pub fn next(&mut self) -> Option<Frame> {
        if self.bad || self.end - self.start < 4 {
            return None;
        }
        let at = self.start;
        let n = u32::from_be_bytes([self.buf[at], self.buf[at + 1], self.buf[at + 2], self.buf[at + 3]]) as usize;
        if !(1..=MAX_FRAME).contains(&n) {
            self.bad = true;
            self.buf = Vec::new();
            self.start = 0;
            self.end = 0;
            return None;
        }
        if self.end - at - 4 < n {
            return None;
        }
        let f = Frame { ty: self.buf[at + 4], body: self.buf[at + 5..at + 4 + n].to_vec() };
        self.start = at + 4 + n;
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
            self.buf.clear();
        }
        Some(f)
    }

    /// The bytes fed but not yet taken as a frame, handed over (to the reader that goes on with the stream); the
    /// reader holds nothing after it.
    pub fn take_rest(&mut self) -> Vec<u8> {
        let rest = self.buf[self.start..self.end].to_vec();
        self.buf.clear();
        self.start = 0;
        self.end = 0;
        rest
    }

    /// Bytes held for a frame not yet complete.
    pub fn pending(&self) -> usize {
        self.end - self.start
    }
}

// ---- UDP

/// `udpDatagram`: the token, the kind, and for [`UDP_MATCH`] the handle and packet (a probe's flag byte is 0: see
/// [`probe_datagram`]).
pub fn udp_datagram(token: &[u8; TOKEN_BYTES], kind: u8, handle: u32, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(TOKEN_BYTES + 5 + bytes.len());
    out.extend_from_slice(token);
    out.push(kind);
    if kind == UDP_MATCH {
        out.extend_from_slice(&handle.to_be_bytes());
        out.extend_from_slice(bytes);
    } else if kind == UDP_PROBE {
        out.push(0);
    }
    out
}

/// `probeDatagram`: a [`UDP_PROBE`]; `heard` is the flag (an acknowledgement came in the last [`UDP_HEARD_MS`]).
pub fn probe_datagram(token: &[u8; TOKEN_BYTES], heard: bool) -> Vec<u8> {
    let mut out = udp_datagram(token, UDP_PROBE, 0, &[]);
    out[TOKEN_BYTES + 1] = u8::from(heard);
    out
}

/// A datagram's parts (`heard`: a probe's flag; `handle` 0 and `bytes` empty but for a [`UDP_MATCH`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Datagram<'a> {
    pub token: [u8; TOKEN_BYTES],
    pub kind: u8,
    pub handle: u32,
    pub bytes: &'a [u8],
    pub heard: bool,
}

/// `readUdp`: `None` when the datagram is too short for its kind.
pub fn read_udp(d: &[u8]) -> Option<Datagram<'_>> {
    let (token, rest) = d.split_first_chunk::<TOKEN_BYTES>()?;
    let mut out = read_udp_body(rest)?;
    out.token = *token;
    Some(out)
}

/// A datagram's body (the kind and what follows it: what a sealed datagram opens to) read; its `token` is zeros.
/// `None` when it is too short for its kind.
pub fn read_udp_body(body: &[u8]) -> Option<Datagram<'_>> {
    let (&kind, rest) = body.split_first()?;
    let token = [0u8; TOKEN_BYTES];
    if kind == UDP_PROBE {
        let &flag = rest.first()?;
        return Some(Datagram { token, kind, handle: 0, bytes: &[], heard: flag == 1 });
    }
    if kind != UDP_MATCH {
        return Some(Datagram { token, kind, handle: 0, bytes: &[], heard: false });
    }
    let (handle, bytes) = rest.split_first_chunk::<4>()?;
    Some(Datagram { token, kind, handle: u32::from_be_bytes(*handle), bytes, heard: false })
}

// ---- the handshake and the checks

/// The signer's side of the handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Relay,
    Client,
}

/// The handshake's transcript: raw keys, nonces and the two X25519 public keys (kx).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transcript {
    pub client_key: [u8; 32],
    pub relay_key: [u8; 32],
    pub client_nonce: [u8; NONCE_BYTES],
    pub relay_nonce: [u8; NONCE_BYTES],
    pub client_kx: [u8; 32],
    pub relay_kx: [u8; 32],
}

/// The transcript's bytes (192).
pub const TRANSCRIPT_BYTES: usize = 32 * 4 + NONCE_BYTES * 2;

impl Transcript {
    /// T = client key || relay key || client nonce || relay nonce || client kx || relay kx.
    pub fn bytes(&self) -> [u8; TRANSCRIPT_BYTES] {
        let mut out = [0u8; TRANSCRIPT_BYTES];
        let mut at = 0;
        for part in [
            &self.client_key[..],
            &self.relay_key,
            &self.client_nonce,
            &self.relay_nonce,
            &self.client_kx,
            &self.relay_kx,
        ] {
            out[at..at + part.len()].copy_from_slice(part);
            at += part.len();
        }
        out
    }
}

/// `handshakeBytes`: the bytes one side signs in the start: its context ([`SIGN_RELAY`] / [`SIGN_CLIENT`]), then
/// T ([`Transcript::bytes`]).
pub fn handshake_bytes(side: Side, t: &Transcript) -> Vec<u8> {
    let ctx = match side {
        Side::Relay => SIGN_RELAY,
        Side::Client => SIGN_CLIENT,
    };
    let mut out = Vec::with_capacity(ctx.len() + TRANSCRIPT_BYTES);
    out.extend_from_slice(ctx);
    out.extend_from_slice(&t.bytes());
    out
}

/// `keyId`: the id of a raw Ed25519 public key: `r` + the first 32 hex digits of its SHA-256.
pub fn key_id(raw: &[u8]) -> String {
    let d = Sha256::digest(raw);
    let mut s = String::with_capacity(33);
    s.push('r');
    for b in &d[..16] {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Whether a text is a [`key_id`]: `r` and 32 lowercase hex digits.
pub fn is_key_id(s: &str) -> bool {
    s.len() == 33 && s.starts_with('r') && s.bytes().skip(1).all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A relay the player has said yes to: its address, and the id of the key it proved when one was seen (None: an
/// address saved before the ids were kept, trusted on its first use and given the id it proves then).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownRelay {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// `badPublicKey`: whether bytes are no usable Ed25519 public key: not 32 bytes, y not below p (not canonical), a
/// point of small order (the identity among them; any sign bit), x = 0 with the sign bit set, or no point at all (x^2
/// not a square). RFC 8032 5.1.3.
///
/// The point checks are ed25519-dalek's (decompression and `is_weak`). It takes a y of p or more as the y mod p, so
/// canonicity is checked here first. Where the TS names x = 0 with the sign bit set, dalek accepts the decoding, but
/// that is y = 1 or p - 1, which are small-order points and refused either way.
pub fn bad_public_key(raw: &[u8]) -> bool {
    let Ok(raw) = <&[u8; 32]>::try_from(raw) else { return true };
    // y (the low 255 bits, little-endian) is p - 1 at the most: p = 2^255 - 19
    let y_top_ones = raw[31] & 0x7f == 0x7f && raw[1..31].iter().all(|&b| b == 0xff);
    if y_top_ones && raw[0] >= 0xed {
        return true;
    }
    match ed25519_dalek::VerifyingKey::from_bytes(raw) {
        Ok(k) => k.is_weak(),
        Err(_) => true,
    }
}

/// JS `\s`.
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// `/^\d{1,3}(\.\d{1,3}){3}$/` as its four numbers.
fn dotted(s: &str) -> Option<[u32; 4]> {
    let mut it = s.split('.');
    let mut out = [0u32; 4];
    for o in &mut out {
        let p = it.next()?;
        if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *o = p.parse().ok()?;
    }
    it.next().is_none().then_some(out)
}

/// `normalAddress`: an IP address as one text for comparing: an IPv4 one (also IPv4-mapped IPv6, any spelling)
/// dotted, an IPv6 one as eight groups of four lowercase hex digits, its zone dropped; `None` when it is no IP address.
pub fn normal_address(a: &str) -> Option<String> {
    let t = a.trim_matches(js_space).to_lowercase();
    // (the zone: `replace(/%.*$/, '')`, which starts at the first % with no line break after it)
    let mut s = t.as_str();
    for (at, c) in t.char_indices() {
        if c == '%' && !t[at..].contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            s = &t[..at];
            break;
        }
    }
    if let Some(d) = dotted(s) {
        return d.iter().all(|&x| x <= 255).then(|| d.map(|x| x.to_string()).join("."));
    }
    let g = ipv6_groups(s)?;
    if g[..5].iter().all(|&x| x == 0) && g[5] == 0xffff {
        return Some(format!("{}.{}.{}.{}", g[6] >> 8, g[6] & 255, g[7] >> 8, g[7] & 255));
    }
    Some(g.map(|x| format!("{x:04x}")).join(":"))
}

/// `ipv6Groups`: an IPv6 text's eight 16-bit groups (an IPv4 tail as the last two); `None` when it is not one.
pub fn ipv6_groups(s: &str) -> Option<[u16; 8]> {
    if s.is_empty()
        || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b':' | b'.'))
        || s.matches("::").count() > 1
    {
        return None;
    }
    let mut tail: Vec<u16> = Vec::new();
    let mut head = s;
    // (an IPv4 tail: what follows the last ':' when it is four dotted numbers)
    if let Some(colon) = s.rfind(':')
        && let Some(b) = dotted(&s[colon + 1..])
    {
        if b.iter().any(|&x| x > 255) {
            return None;
        }
        let w = |hi: u32, lo: u32| u16::try_from((hi << 8) | lo).ok();
        tail = vec![w(b[0], b[1])?, w(b[2], b[3])?];
        head = &s[..colon + 1];
        // (the ':' before the tail goes, but not the second of a '::')
        if !head.ends_with("::") {
            head = &head[..head.len() - 1];
        }
    }
    let want = 8 - tail.len();
    let part =
        |t: &str| -> Vec<String> { if t.is_empty() { Vec::new() } else { t.split(':').map(str::to_owned).collect() } };
    let groups: Vec<String> = if head.contains("::") {
        let mut it = head.split("::");
        let (l, r) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
        let (left, right) = (part(l), part(r));
        if left.len() + right.len() >= want {
            return None;
        }
        let zeros = std::iter::repeat_n("0".to_owned(), want - left.len() - right.len());
        left.into_iter().chain(zeros).chain(right).collect()
    } else {
        part(head)
    };
    if groups.len() != want
        || groups.iter().any(|x| x.is_empty() || x.len() > 4 || !x.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return None;
    }
    let mut out = [0u16; 8];
    for (o, g) in
        out.iter_mut().zip(groups.iter().map(|x| u16::from_str_radix(x, 16).ok()).chain(tail.iter().map(|&x| Some(x))))
    {
        *o = g?;
    }
    Some(out)
}

/// A relay address's parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub host: String,
    pub port: u16,
}

/// JS `Number(text)` for the text after a colon: `Some(n)` of a number or NaN-free reading, `None` for NaN.
fn js_number(t: &str) -> Option<f64> {
    if t.is_empty() {
        return Some(0.0);
    }
    for (p, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if t.len() >= 2 && t[..2].eq_ignore_ascii_case(p) {
            return u128::from_str_radix(&t[2..], radix).ok().map(|n| n as f64);
        }
    }
    // (Number reads Infinity and the decimal forms; Rust's parser also takes "inf" and "nan", neither an integer)
    t.parse::<f64>().ok().filter(|v| !v.is_nan())
}

/// `parseAddress`: a relay address `host[:port]` (an IPv6 literal in brackets when it has a port) as its parts;
/// `None` when it is not one. The port is [`RELAY_PORT`] when none is given.
pub fn parse_address(text: &str) -> Option<Address> {
    let s = text.trim_matches(js_space);
    if s.is_empty() || s.encode_utf16().count() > 255 || s.chars().any(js_space) {
        return None;
    }
    let hexish = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.');
    let mut host = s;
    let mut port = f64::from(RELAY_PORT);
    // `^\[([0-9a-fA-F:.]+)\](?::(\d+))?$`
    let v6 = s.strip_prefix('[').and_then(|r| r.split_once(']')).filter(|(inner, _)| hexish(inner)).and_then(
        |(inner, rest)| match rest.strip_prefix(':') {
            None if rest.is_empty() => Some((inner, None)),
            Some(d) if !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()) => Some((inner, Some(d))),
            _ => None,
        },
    );
    if let Some((inner, digits)) = v6 {
        host = inner;
        if let Some(d) = digits {
            port = js_number(d)?;
        }
    } else if s.matches(':').count() == 1 {
        let at = s.rfind(':')?;
        host = &s[..at];
        port = js_number(&s[at + 1..]).unwrap_or(f64::NAN);
    } else if s.contains(':') && !hexish(s) {
        return None;
    }
    if host.is_empty() || port.is_nan() || port.fract() != 0.0 || !(1.0..=65535.0).contains(&port) {
        return None;
    }
    if !s.contains('[') && !hexish(host) && !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
        return None;
    }
    // (checked to 1 ..= 65535 above)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "an integer in 1 ..= 65535")]
    let port = port as u16;
    Some(Address { host: host.to_owned(), port })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_is_optional_and_an_unknown_one_is_none() {
        let p =
            |s: &str| serde_json::from_str::<Who>(&format!(r#"{{"id":"r1","name":"A","icon":"","busy":false{s}}}"#));
        assert_eq!(p("").unwrap().status, None);
        assert_eq!(p(r#","status":"ffa""#).unwrap().status, Some(PresenceStatus::Ffa));
        assert_eq!(p(r#","status":"spectating""#).unwrap().status, None);
        assert_eq!(p(r#","status":7"#).unwrap().status, None);
        for s in PresenceStatus::ALL {
            assert_eq!(PresenceStatus::of(s.id()), Some(s));
            assert_eq!(serde_json::to_value(s).unwrap(), serde_json::json!(s.id()));
        }
        let w = Who { id: "r1".into(), name: "A".into(), icon: String::new(), busy: false, status: None };
        assert!(!serde_json::to_string(&w).unwrap().contains("status"), "none is left out");
    }
}
