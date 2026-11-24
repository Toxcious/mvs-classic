//! The relay client: one TCP connection to a relay for the control messages and channel 0,
//! UDP for channel 1, all of it on a net thread of its own with std sockets. The game thread holds a [`RelayClient`]:
//! its calls go to the thread as commands, what the relay says comes back as [`RelayEvent`]s, and a match's packets
//! go through a [`RelayMatchLink`] (the session's [`MatchLink`]).
//!   the start   the address resolved, hello with this player's key and a nonce; the relay's challenge must carry its
//!               key's signature over the transcript (`relay_wire::handshake_bytes`), then prove (our signature over
//!               it) and its welcome (our id, the UDP token and port). All of it within HELLO_MS of the start (the
//!               name's lookup is the system's, not under the deadline); a refused { why } or another wire version
//!               fails with the reason. Both sides send an X25519 key of their own (kx) in it: what the relay sends
//!               after its challenge and what we send after our prove are sealed with the keys of the two
//!               (`relay_seal`), TCP frames and datagrams; a frame that does not open ends the connection, a datagram
//!               that does not open (or came before) is dropped.
//!   lobbies     each call a req with an id, answered by the res of that id (`request_ms` at the most); the relay's
//!               lobby, invite and peerClosed messages are [`RelayEvent`]s
//!   packets     peer handles per direction: we name each peer we send to by a handle of ours (peer { handle, id }
//!               before its first packet; increasing, never one twice), the relay names each sender to us by one of
//!               its own; both are freed when the link closes, and a packet on a handle not live is dropped. Channel 0
//!               goes on TCP (FT_DATA) at MESSAGE_PACE. Channel 1 goes on UDP while the relay acknowledges our probes
//!               (one every `udp_probe_ms`, flagged while acknowledgements come; none for `udp_lost_ms`: a
//!               firewall), else on TCP (FT_MATCH), and back to UDP at the next acknowledgement; a new handle's first
//!               packet always on TCP. A datagram counts only from the TCP connection's address (`normal_address`)
//!               and the relay's UDP port.
//!   links       opened by the first packet either way; a packet from a peer with no link is taken when the peer is a
//!               member of a lobby this player is in, else dropped and the link closed (closePeer), at most once a
//!               second per peer. A referee's and a host core's packets are taken as they come (their ids are the
//!               relay's own: `relay_wire::is_host_id`).
//!   matches     the relay's host cores' matches are refereed on the relay ([`RelayClient::join_match`] to its
//!               referee's id). Or this thread referees one itself ([`RelayClient::host_match`]: a `Referee` pumped
//!               every REFEREE_PUMP_MS and after each packet it takes; its humans' news
//!               are [`RelayEvent::Slot`]), its humans' packets taken by their sender's id on channel 1.
//!   keepalive   a ping every PING_MS; the relay silent for SILENT_MS is a drop
//!   in a match  while this client sends channel 1 (a session sends each frame, also while it waits) or a route is set;
//!               an empty channel-1 send is a beat of a match that goes on Epic (`service::path`): nothing goes out
//!   resume      a drop (the socket closed or failed, the relay silent) or, in a match, no channel 1 taken for
//!               RESUME_TRY_MS and a ping then unanswered for STALL_PING_MS opens a second connection beside the first,
//!               its hello counting the channel-0 frames taken; its welcome takes the seat over: the sockets swapped,
//!               the handles named anew, channel 0 resent from the relay's count (`resume.rs`)
//!   both ways   in a match, no UDP acknowledgement for UDP_PROBE_MS + UDP_LATE_MS sends channel 1 on UDP and TCP both
//!               until an acknowledgement comes or UDP is given up
//!   faults      (dev) a cut, loss, delay and jitter put on this client's own link ([`NetFaults`], `faults`)
//! The connection's end (a drop not resumed within RESUME_MS, a welcome with no seat kept, refused by the relay, a bad
//! frame) is a [`RelayEvent::SignedOut`] with the reason; [`RelayClient::shutdown`] ends it with a bye and without one.
//!
//! The client is a thread of
//! the game's process, its calls blocking (`request`) or answered as events (the `Backend` calls: `RelayEvent::Answer`,
//! in the order the relay's messages came) or fire-and-forget (packets: a send the thread refuses is dropped as a lost
//! one).
//!   presence    a relay chosen by its address keeps this player's presence and gives the online list; the newest
//!               asked goes at most once per PRESENCE_EVERY_MS, a refused one's (over the relay's rate) again at the
//!               pace up to PRESENCE_TRIES times
//!   direct      a client with a host key (a direct host's joiner) accepts only the host whose key the code names, and
//!               has no presence and no referee of the relay's

pub(crate) mod backend;
mod faults;
pub mod key;
mod resume;

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use indexmap::{IndexMap, IndexSet};
use mvs_protocol::match_wire::{MAX_PACKET, MAX_SLOTS, packet_fits};
use mvs_protocol::referee::{Referee, RefereeOut, SlotNews};
use mvs_protocol::relay_seal::{
    DatagramOpener, KX_BYTES, KxSecret, MAX_SEALED_DATAGRAM, SealedReader, TxSeal, datagram_token, derive, shared,
    split,
};
use mvs_protocol::relay_wire::Presence as WirePresence;
use mvs_protocol::relay_wire::ServerMessage as WireMessage;
use mvs_protocol::relay_wire::{
    ClientMessage, FT_DATA, FT_JSON, FT_MATCH, Frame, FrameReader, HELLO_MS, Joined, MAX_FRAME, MAX_PEER_HANDLES,
    NONCE_BYTES, PING_MS, RELAY_WIRE_VERSION, Request, SILENT_MS, Side, TOKEN_BYTES, Transcript, UDP_HEARD_MS,
    UDP_LATE_MS, UDP_MATCH, UDP_PROBE, UDP_PROBE_ACK, UDP_PROBE_MS, handshake_bytes, is_host_id, json_frame,
    normal_address, parse_address, read_json, read_packet, read_udp_body,
};
use serde_json::{Map, Value};

use self::faults::Faults;
use self::key::{RelayKey, key_fingerprint, key_id, verify_sig};
use self::resume::{Attempt, Ring};
use super::MatchLink;
use crate::service::REFEREE_PUMP_MS;

/// The channel of a lobby's messages (reliable, in order: TCP).
pub const CHANNEL_MESSAGES: u8 = 0;
/// The channel of a match's packets (lossy: UDP, else TCP).
pub const CHANNEL_MATCH: u8 = 1;
/// EOS_P2P_MAX_PACKET_SIZE: the most bytes one packet carries.
pub const P2P_MAX_PACKET: usize = MAX_PACKET;
/// Channel 0 goes at most this fast, 3/4 of the relay's default limit (128 KiB a second with two seconds at once),
/// past which the relay drops the sender: a burst waits in the client instead (MESSAGE_QUEUE_BYTES at the most).
/// Unverified: the port's own margin; a relay configured below its default limit is not known to the client.
pub const MESSAGE_PACE: f64 = 96.0 * 1024.0;
/// At most this many bytes of channel 0 wait for the pace: a send past it is refused.
const MESSAGE_QUEUE_BYTES: usize = 1 << 20;
/// At most this many bytes wait to go out on the TCP connection: a send over it is refused.
const MAX_QUEUED: usize = 1 << 20;
/// A refused peer's packets close its link again at most this often (ms).
const REFUSE_EVERY_MS: f64 = 1000.0;
/// The net thread looks at its sockets and commands this often.
const LOOP_SLEEP: Duration = Duration::from_millis(1);
/// The connection's end at a shutdown: the relay's close waited for this long at the most (ms).
const SHUTDOWN_MS: u64 = 500;
/// A presence goes at most this often: a relay's presence pace (1000 ms by default) and some room for the way there.
/// Unverified: the port's own number.
pub const PRESENCE_EVERY_MS: f64 = 1100.0;
/// A presence the relay refuses over its rate is sent again at the pace this many times in all, then given up.
pub const PRESENCE_TRIES: u32 = 3;
/// The start of a sign-out's reason when the relay refused this connection, its reason after it.
const DROPPED: &str = "the relay dropped this player: ";

/// A probe unacknowledged this long (ms) is taken as lost for the round trip's timing: far past any route's, and
/// a UDP link silent that long is given up (udp_lost_ms) anyway.
const PROBE_FORGET_MS: f64 = 10000.0;

/// The shared round trip's bits while none is measured.
const NO_RTT: u64 = u64::MAX;

/// The shared UDP link's state: not known yet (no acknowledgement, the first udp_lost_ms not over), off, on.
const UDP_NOT_KNOWN: u8 = 0;
const UDP_OFF: u8 = 1;
const UDP_ON: u8 = 2;

/// The relay's refusal of a connection whose seat a second connection took over.
const RESUMED_ELSEWHERE: &str = "resumed elsewhere";

/// A sign-out because another connection under this key took the seat (the relay's "resumed elsewhere", "signed in
/// elsewhere"): final, not signed in again, or two copies of one key would take the seat from each other.
pub fn taken_elsewhere(why: &str) -> bool {
    why.strip_prefix(DROPPED).is_some_and(|w| w.contains("elsewhere"))
}

/// The start's refusal of a relay that proved another key than the pinned one ([`RelayError::OtherKey`]), up to its
/// id.
const OTHER_KEY: &str = "the relay at that address proved another key than the one expected (relay id ";

/// The relay id a start's failure names when the relay proved another key than the one expected
/// ([`RelayError::OtherKey`]'s text, read back from the sign-in's reason); None for any other failure.
pub fn other_key(why: &str) -> Option<&str> {
    let at = why.find(OTHER_KEY)? + OTHER_KEY.len();
    let rest = &why[at..];
    rest.find(')').map(|end| &rest[..end])
}

/// A client that sent channel 1 this recently is in a match (ms). Unverified: the port's own number (a session sends
/// each frame, also while it waits).
const MATCH_ON_MS: f64 = 1000.0;
/// A match that has taken no channel 1 for RESUME_TRY_MS pings the relay: an answer within this long (ms) is a healthy
/// connection (the peers are the silent ones), none a stalled one, which a second connection takes over.
/// Unverified: the port's own number.
const STALL_PING_MS: f64 = 1000.0;

/// What the relay answers a call over its rate.
const LIMIT_EXCEEDED: &str = "EOS_LimitExceeded";

/// The client's waits (ms). Unverified: the port's own numbers, but HELLO_MS, PING_MS and SILENT_MS (the wire's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClientTimes {
    /// The connect, hello, challenge, prove and welcome, at the most.
    pub hello_ms: f64,
    /// A lobby call's answer, at the most.
    pub request_ms: f64,
    pub ping_ms: f64,
    pub silent_ms: f64,
    /// A UDP probe goes this often, while connected.
    pub udp_probe_ms: f64,
    /// No probe acknowledged for this long: channel 1 goes on TCP, and the probes' flag says so to the relay.
    pub udp_lost_ms: f64,
    /// Channel 0's pace, bytes a second (and as many at once): [`MESSAGE_PACE`].
    pub message_bytes_per_sec: f64,
    /// The launcher's probe waits this long for a UDP acknowledgement.
    pub probe_udp_ms: f64,
}

pub const CLIENT_TIMES: ClientTimes = ClientTimes {
    hello_ms: HELLO_MS,
    request_ms: 10000.0,
    ping_ms: PING_MS,
    silent_ms: SILENT_MS,
    udp_probe_ms: UDP_PROBE_MS,
    udp_lost_ms: UDP_HEARD_MS,
    message_bytes_per_sec: MESSAGE_PACE,
    probe_udp_ms: 1500.0,
};

/// The faults a client puts on its own connection (dev switch, for testing a drop and a bad link; none by default).
/// `cut_at_s` counts from the online match's go (the first channel-1 packet this client sends, or match route). A cut
/// holds the TCP connection still both ways (as a network that stops) and drops every datagram; loss drops datagrams
/// (probes and their acknowledgements among them) either way; each datagram waits `delay_ms` plus up to `jitter_ms`
/// either way (they may pass each other), the TCP stream's bytes as long but in order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetFaults {
    /// A cut of this many seconds, both ways.
    pub cut_s: f64,
    /// The cut starts this many seconds after the online match's go.
    pub cut_at_s: f64,
    /// The share of packets dropped (0 to 1).
    pub loss: f64,
    pub delay_ms: f64,
    pub jitter_ms: f64,
}

/// How a client connects beside its address and key.
#[derive(Clone, Debug)]
pub struct ClientOptions {
    pub times: ClientTimes,
    /// The hello's `app`: the product and its version.
    pub app: String,
    /// (tests) UDP neither goes nor comes, as behind a firewall: channel 1 goes on TCP.
    pub udp_blocked: bool,
    /// The clock every `now` of the client counts from (a hosted match's referee, its tick 0 among them).
    pub epoch: Instant,
    /// (dev) the faults of this client's link.
    pub faults: NetFaults,
}

impl ClientOptions {
    pub fn new(app: &str, epoch: Instant) -> ClientOptions {
        ClientOptions {
            times: CLIENT_TIMES,
            app: app.to_owned(),
            udp_blocked: false,
            epoch,
            faults: NetFaults::default(),
        }
    }
}

/// Why a start or a call failed, in the launcher's words.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RelayError {
    /// The start: the address, the connection, the handshake.
    #[error("{0}")]
    Start(String),
    /// The connection has ended (its reason).
    #[error("{0}")]
    Closed(String),
    /// The relay answered the call with an error.
    #[error("{0}")]
    Refused(String),
    /// The relay did not answer the call in time (it may still take it: the TCP stream is in order).
    #[error("the relay did not answer in time")]
    Timeout,
    /// The answer is not of the call's shape.
    #[error("the relay's answer to {0} is not one")]
    Answer(&'static str),
    /// The start: the relay proved a key whose id is not the one expected ([`StartWith::relay_id`]); `proved` its id.
    /// The text starts with [`OTHER_KEY`] ([`other_key`] reads the id back).
    #[error("the relay at that address proved another key than the one expected (relay id {proved})")]
    OtherKey { proved: String },
}

/// What the relay says to the game thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayEvent {
    /// A lobby this player is in: its members now, or why it closed.
    Lobby {
        lobby: String,
        members: Option<Vec<String>>,
        closed: Option<String>,
    },
    /// A packet of another player (a match's own go to its [`RelayMatchLink`] instead).
    Packet {
        from: String,
        channel: u8,
        bytes: Vec<u8>,
    },
    /// The other side ended the link to that peer.
    PeerClosed(String),
    Invite {
        from: String,
        payload: String,
    },
    /// News of a human of the match this thread referees itself ([`RelayClient::host_match`]).
    Slot {
        slot: u32,
        news: SlotNews,
    },
    /// The listing of a lobby this player owns and the relay hosts changed (relay-wire listing; None: private).
    Listing {
        lobby: String,
        info: Option<String>,
    },
    /// The answer to a call asked with a tag, in the order of the messages around it.
    Answer {
        tag: u32,
        result: Result<Value, RelayError>,
    },
    /// The connection has ended, and why.
    SignedOut(String),
}

/// A finished start: the socket (its frames after the welcome in `rest`) and what the relay said.
struct Welcome {
    tcp: TcpStream,
    /// The relay's frames, opened (sealed from the frame after the challenge on).
    reader: SealedReader,
    rest: Vec<Frame>,
    version: u32,
    /// The relay's key (raw, base64).
    relay_key: String,
    id: String,
    token: [u8; TOKEN_BYTES],
    /// Where datagrams go: the relay's address as resolved and its UDP port.
    udp_to: SocketAddr,
    /// The TCP connection's peer address (`normal_address`): what a datagram from the relay must come from.
    peer_ip: String,
    /// The last datagram source found to be the relay's (`peer_ip` and the UDP port): the next from it is not looked
    /// at again as text.
    udp_from: Option<SocketAddr>,
    /// Hello to challenge (ms).
    ms: f64,
    /// The welcome's count of the channel-0 frames the relay took from the seat (None: a new seat).
    resumed: Option<u64>,
    /// The game protocol of the cores the relay hosts, as its welcome said (None: it did not say, or not as a whole
    /// number).
    proto: Option<u32>,
    /// What this client sends with after its prove, frames and datagrams (relay_seal), derived at the challenge.
    tx: TxSeal,
    /// What the relay's datagrams are opened with.
    datagrams: DatagramOpener,
}

fn b64_exact<const N: usize>(v: Option<&Value>) -> Option<[u8; N]> {
    let b = B64.decode(v?.as_str()?).ok()?;
    b.try_into().ok()
}

/// What a socket error says, in the launcher's words where it is a common one.
fn socket_why(e: &std::io::Error) -> String {
    match e.kind() {
        ErrorKind::ConnectionRefused => {
            "nothing answers at that address (is the relay running there, its port open?)".to_owned()
        }
        ErrorKind::TimedOut | ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable => {
            "that address cannot be reached".to_owned()
        }
        ErrorKind::ConnectionReset => "the relay ended the connection".to_owned(),
        _ => e.to_string(),
    }
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr, RelayError> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    let not_found = || RelayError::Start(format!("the name {host} was not found"));
    (host, port).to_socket_addrs().map_err(|_| not_found())?.next().ok_or_else(not_found)
}

/// What a start may be given beside its address and key.
#[derive(Clone, Default)]
pub struct StartWith {
    /// A direct host's key fingerprint (16 hex digits): the relay must prove a key that is it.
    pub host_key: Option<String>,
    /// The id of the key the relay must prove (`relay_wire::key_id`): another one fails the start as
    /// [`RelayError::OtherKey`]. None: any key.
    pub relay_id: Option<String>,
    /// Asked once the relay has proved its key (and it is the host key's), before this side proves its own: false ends
    /// the start, unsigned in (a direct join races a host's addresses and lets one of them sign in).
    pub claim: Option<std::sync::Arc<dyn Fn() -> bool + Send + Sync>>,
    /// Set to end the start: it fails as cancelled.
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl std::fmt::Debug for StartWith {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartWith")
            .field("host_key", &self.host_key)
            .field("relay_id", &self.relay_id)
            .finish_non_exhaustive()
    }
}

/// The connection's start: the address resolved, connected, hello, challenge, prove and welcome.
/// `got`: a resume's count of the channel-0 frames taken on the seat (None: a fresh sign-in).
fn start(
    address: &str,
    key: &RelayKey,
    o: &ClientOptions,
    x: &StartWith,
    got: Option<u64>,
) -> Result<Welcome, RelayError> {
    let fail = |why: &str| RelayError::Start(why.to_owned());
    let parts = parse_address(address)
        .ok_or_else(|| fail("that is not a relay address (a name or an IP address, then :port when it is not 7777)"))?;
    let begun = Instant::now();
    let deadline = begun + Duration::from_secs_f64(o.times.hello_ms / 1000.0);
    let at = resolve(&parts.host, parts.port)?;
    let left = || deadline.saturating_duration_since(Instant::now());
    let cancelled = || x.cancel.as_ref().is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed));
    if cancelled() {
        return Err(fail("cancelled"));
    }
    if left().is_zero() {
        return Err(RelayError::Start(format!("the name {} did not resolve in time", parts.host)));
    }
    // (with a cancel, the connect runs on a thread of its own so that a cancel is seen at once; the thread ends with
    // its own timeout)
    let connected = match &x.cancel {
        None => TcpStream::connect_timeout(&at, left()),
        Some(c) => {
            let (tx, rx) = mpsc::channel();
            let wait = left();
            std::thread::spawn(move || {
                let _ = tx.send(TcpStream::connect_timeout(&at, wait));
            });
            loop {
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(r) => break r,
                    Err(mpsc::RecvTimeoutError::Timeout) if c.load(std::sync::atomic::Ordering::Relaxed) => {
                        return Err(fail("cancelled"));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Err(fail("that address did not answer")),
                }
            }
        }
    };
    let mut tcp = connected.map_err(|e| match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => fail("that address did not answer"),
        _ => RelayError::Start(socket_why(&e)),
    })?;
    tcp.set_nodelay(true).map_err(|e| RelayError::Start(socket_why(&e)))?;
    let mut nonce = [0u8; NONCE_BYTES];
    rand::fill(&mut nonce);
    // (taken by the challenge's exchange)
    let mut kx = Some(KxSecret::fresh());
    let client_kx = kx.as_ref().map(KxSecret::public).unwrap_or_default();
    let send = |tcp: &mut TcpStream, m: &ClientMessage| -> Result<(), RelayError> {
        let f = json_frame(m).map_err(|e| RelayError::Start(e.to_string()))?;
        tcp.write_all(&f).map_err(|e| RelayError::Start(socket_why(&e)))
    };
    let hello = ClientMessage::Hello {
        v: RELAY_WIRE_VERSION,
        key: key.public_key(),
        nonce: B64.encode(nonce),
        kx: B64.encode(client_kx),
        app: Some(o.app.clone()),
        got,
    };
    send(&mut tcp, &hello)?;
    let sent_at = Instant::now();
    // (plain up to the challenge, sealed from the frame after it: `sealed` then reads the rest)
    let mut plain = FrameReader::new();
    let mut sealed: Option<SealedReader> = None;
    // the relay's wire version, its key (base64) and the hello's time to the challenge
    let mut challenge: Option<(u32, String, f64)> = None;
    let mut keys: Option<(TxSeal, DatagramOpener)> = None;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let wait = left();
        if wait.is_zero() {
            return Err(fail("the relay did not answer the hello"));
        }
        if cancelled() {
            return Err(fail("cancelled"));
        }
        // (in slices, so that a cancel is seen)
        tcp.set_read_timeout(Some(wait.min(Duration::from_millis(20))))
            .map_err(|e| RelayError::Start(socket_why(&e)))?;
        let n = match tcp.read(&mut buf) {
            Ok(0) => return Err(fail("the relay closed the connection")),
            Ok(n) => n,
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => continue,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(RelayError::Start(socket_why(&e))),
        };
        match &mut sealed {
            Some(r) => r.feed(&buf[..n]),
            None => plain.feed(&buf[..n]),
        }
        loop {
            let f = match &mut sealed {
                Some(r) => r.next(),
                None => plain.next(),
            };
            let Some(f) = f else {
                if plain.bad || sealed.as_ref().is_some_and(|r| r.bad) {
                    return Err(fail("the relay sent a bad frame"));
                }
                break;
            };
            let m = if f.ty == FT_JSON { read_json(&f.body) } else { None };
            let Some(m) = m else { return Err(fail("the relay answered out of turn")) };
            let t = m.get("t").and_then(Value::as_str).unwrap_or("");
            if t == "refused" {
                let why = m.get("why").and_then(Value::as_str).unwrap_or("no reason given");
                return Err(RelayError::Start(format!("the relay refused: {why}")));
            }
            if t == "challenge" && challenge.is_none() {
                let v = m.get("v").and_then(Value::as_f64);
                if v != Some(f64::from(RELAY_WIRE_VERSION)) {
                    let shown = m.get("v").map_or_else(|| "undefined".to_owned(), ToString::to_string);
                    return Err(RelayError::Start(format!(
                        "the relay speaks wire version {shown}, this app {RELAY_WIRE_VERSION}: one of the two needs an \
                         update"
                    )));
                }
                let theirs = b64_exact::<NONCE_BYTES>(m.get("nonce"));
                let relay_raw = b64_exact::<32>(m.get("key"));
                let relay_kx = b64_exact::<KX_BYTES>(m.get("kx"));
                let sig = m.get("sig").and_then(Value::as_str);
                let (Some(theirs), Some(relay_raw), Some(relay_kx), Some(sig)) = (theirs, relay_raw, relay_kx, sig)
                else {
                    return Err(fail("the relay did not prove its key"));
                };
                let tr = Transcript {
                    client_key: *key.raw(),
                    relay_key: relay_raw,
                    client_nonce: nonce,
                    relay_nonce: theirs,
                    client_kx,
                    relay_kx,
                };
                if !verify_sig(&relay_raw, &handshake_bytes(Side::Relay, &tr), sig) {
                    return Err(fail("the relay did not prove its key"));
                }
                // (a relay kx of small order: the exchange's secret would be all zero)
                let Some(secret) = kx.take().and_then(|k| shared(k, &relay_kx)) else {
                    return Err(fail("the relay did not prove its key"));
                };
                let relay_key = m.get("key").and_then(Value::as_str).unwrap_or_default().to_owned();
                if let Some(h) = &x.host_key
                    && key_fingerprint(&relay_raw) != h.to_lowercase()
                {
                    return Err(fail("the host at that address is not the one its code names"));
                }
                if let Some(want) = &x.relay_id
                    && key_id(&relay_raw) != *want
                {
                    return Err(RelayError::OtherKey { proved: key_id(&relay_raw) });
                }
                let (tx, opener, datagrams) = split(&derive(&secret, &tr), Side::Client);
                sealed = Some(SealedReader::new(opener, plain.take_rest()));
                keys = Some((tx, datagrams));
                if x.claim.as_ref().is_some_and(|c| !c()) {
                    return Err(fail("cancelled"));
                }
                challenge = Some((RELAY_WIRE_VERSION, relay_key, sent_at.elapsed().as_secs_f64() * 1000.0));
                send(&mut tcp, &ClientMessage::Prove { sig: key.sign(&handshake_bytes(Side::Client, &tr)) })?;
                continue;
            }
            if let ("welcome", Some((version, relay_key, ms)), Some(_)) = (t, &challenge, &keys) {
                if m.get("id").and_then(Value::as_str) != Some(key.id()) {
                    return Err(fail("the relay named this player by another id"));
                }
                let token = b64_exact::<TOKEN_BYTES>(m.get("token"));
                let udp =
                    m.get("udp").and_then(Value::as_f64).filter(|u| u.fract() == 0.0 && (1.0..=65535.0).contains(u));
                let (Some(token), Some(udp)) = (token, udp) else { return Err(fail("the relay sent a bad welcome")) };
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "checked to be a port above"
                )]
                let udp = udp as u16;
                let peer = tcp.peer_addr().map_or(at.ip(), |a| a.ip());
                let peer_ip = normal_address(&peer.to_string()).unwrap_or_else(|| at.ip().to_string());
                let mut reader = sealed.take().expect("a sealed reader with the keys");
                let mut rest = Vec::new();
                while let Some(f) = reader.next() {
                    rest.push(f);
                }
                if reader.bad {
                    return Err(fail("the relay sent a bad frame"));
                }
                let (tx, datagrams) = keys.take().expect("keys with the challenge");
                return Ok(Welcome {
                    reader,
                    rest,
                    version: *version,
                    relay_key: relay_key.clone(),
                    id: key.id().to_owned(),
                    token,
                    udp_to: SocketAddr::new(at.ip(), udp),
                    peer_ip,
                    udp_from: None,
                    ms: *ms,
                    resumed: m.get("resumed").and_then(Value::as_u64),
                    proto: welcome_proto(&m),
                    tx,
                    datagrams,
                    tcp,
                });
            }
            return Err(fail("the relay answered out of turn"));
        }
    }
}

/// The welcome's `proto`, leniently: None when it is missing or not a whole number that fits a u32 (an older relay
/// sends none).
fn welcome_proto(m: &Map<String, Value>) -> Option<u32> {
    let p = m
        .get("proto")
        .and_then(Value::as_f64)
        .filter(|p| p.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(p))?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "checked to be a whole u32 above")]
    Some(p as u32)
}

/// What the game thread asks of the net thread.
enum Cmd {
    Request {
        req: Request,
        reply: Option<Sender<Result<Value, RelayError>>>,
    },
    Send {
        to: String,
        channel: u8,
        bytes: Vec<u8>,
    },
    ClosePeer(String),
    /// A call whose answer comes back as `RelayEvent::Answer` of this tag.
    Ask {
        tag: u32,
        req: Request,
    },
    /// The presence to keep (the newest asked goes at the pace); its answer is `Answer` of this tag.
    Presence {
        tag: u32,
        p: WirePresence,
    },
    /// Who may open a link to this player (the service says): in place of the members of the lobbies.
    Accept(IndexSet<String>),
    /// A match's packets from now on: to and from `referee`, to the game on `to_game`; `hosted` this thread's own
    /// referee of the match.
    Match {
        referee: String,
        to_game: Sender<Vec<u8>>,
        hosted: Option<HostedMatch>,
    },
    /// A packet of the game's session to the match's referee.
    MatchPacket(Vec<u8>),
    /// The match is over: its packets are no more routed, a hosted referee goes.
    MatchEnd,
    /// (tests) the TCP connection ends as a network's drop would end it.
    CutSocket,
    /// (tests) a second connection starts now beside the first, which goes on meanwhile.
    Resume,
    Shutdown,
}

/// A call under way: its answer's way back, when it was sent, and what the thread itself does with the answer.
struct Pending {
    reply: Option<Sender<Result<Value, RelayError>>>,
    tag: Option<u32>,
    at: f64,
    what: Asked,
}

/// What the thread keeps of a call beside its answer.
enum Asked {
    Other,
    Create,
    Join(String),
    Leave(String),
    /// A presence: the tags of the sets it answers, and the presence itself (to send again).
    Presence(Vec<u32>, WirePresence),
}

/// What waits for channel 0's pace, made into its frame as it goes (its peer's handle then: a resume names them anew).
enum Paced {
    /// A packet to a peer (its peer message first when the peer has no handle yet).
    Data { to: String, bytes: Vec<u8> },
    /// A closePeer behind the peer's packets (cost 0); the peer's handle is freed as it goes.
    Close(String),
}

impl Paced {
    fn cost(&self) -> usize {
        match self {
            Paced::Data { bytes, .. } => bytes.len(),
            Paced::Close(_) => 0,
        }
    }
}

/// A match the net thread referees itself, as the game asks for it: its slot count (its bots' among them), its
/// humans (slot, id), its tick 0 on the client's clock.
#[derive(Clone, Debug)]
pub(crate) struct HostedMatch {
    slots: usize,
    humans: Vec<(usize, String)>,
    tick0: f64,
}

/// A match this thread referees.
struct Hosted {
    referee: Referee,
    /// By slot, its human's id; by id, its slot.
    ids: IndexMap<usize, String>,
    slots: IndexMap<String, usize>,
    pumped: f64,
    /// A call of the referee panicked (logged once a match: the pump runs every few ms).
    failed: bool,
}

/// What a referee's pump or receive sent.
#[derive(Default)]
struct Collect {
    sends: Vec<(usize, Vec<u8>)>,
    silent: Vec<(usize, SlotNews)>,
}

impl RefereeOut for Collect {
    fn send(&mut self, slot: usize, bytes: &[u8]) {
        self.sends.push((slot, bytes.to_vec()));
    }

    fn news(&mut self, slot: usize, n: SlotNews) {
        self.silent.push((slot, n));
    }
}

/// The match's packets' way: its referee's id, the game's end, and the referee when this thread is it.
struct Route {
    referee: String,
    to_game: Sender<Vec<u8>>,
    hosted: Option<Hosted>,
}

/// What a connection sent and refused (the checks' readout).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelayStats {
    pub messages: u64,
    pub matched: u64,
    pub udp: u64,
    pub tcp_match: u64,
    /// Channel 1 packets sent both ways (counted in `udp` and `tcp_match` too).
    pub both: u64,
    pub refused: u64,
    /// Second connections that took the seat over.
    pub resumes: u64,
    /// Cuts of the faults begun.
    pub cuts: u64,
}

/// The net thread's state: one signed-in connection.
struct Net {
    w: Welcome,
    t: ClientTimes,
    epoch: Instant,
    udp: Option<UdpSocket>,
    udp_blocked: bool,
    /// Bytes not yet written to the TCP connection.
    out: Vec<u8>,
    /// The datagram being sent, sealed (one buffer for every send: a datagram allocates only when the faults hold it).
    datagram: Vec<u8>,
    gone: Option<String>,
    ending: bool,
    events: Sender<RelayEvent>,
    cmds: Receiver<Cmd>,
    reqs: IndexMap<i64, Pending>,
    next_req: i64,
    /// Our live handles for the peers we send to (never a number twice), and the relay's for those that send to us.
    out_handles: IndexMap<String, u32>,
    next_handle: u32,
    in_handles: IndexMap<u32, String>,
    in_by_id: IndexMap<String, u32>,
    links: IndexSet<String>,
    refused_at: IndexMap<String, f64>,
    /// The lobbies this player is in and their members as last told (who may open a link to this player).
    lobbies: IndexMap<String, Vec<String>>,
    /// Who may open a link, as the service says (None: the members of the lobbies).
    accepted: Option<IndexSet<String>>,
    /// The presence asked and not sent yet, the tags of the sets waiting for it, when one was last sent, one under way
    /// and the tries of the newest.
    presence_wanted: Option<WirePresence>,
    presence_waiting: Vec<u32>,
    presence_sent_at: f64,
    presence_busy: bool,
    presence_tries: u32,
    last_heard: f64,
    last_ack: f64,
    last_ping: f64,
    last_probe: f64,
    /// The round trip's timing: the pings sent and not yet answered (TCP keeps them in order), the probes sent and not
    /// yet acknowledged in the order they went (an acknowledgement carries no number: each is taken for the oldest
    /// probe out, so a lost one makes the later times longer, never shorter; one out past PROBE_FORGET_MS is let
    /// go), the smallest round trip so far and the game thread's copy of it (f64 bits, NO_RTT none).
    pings: VecDeque<f64>,
    probes: VecDeque<f64>,
    rtt: f64,
    shared_rtt: Arc<AtomicU64>,
    /// When the UDP probes started, and the game thread's copy of the UDP link's state (UDP_NOT_KNOWN, UDP_OFF,
    /// UDP_ON).
    udp_since: f64,
    shared_udp: Arc<AtomicU8>,
    pending: VecDeque<Paced>,
    pending_bytes: usize,
    /// The closePeer frames of each peer that still wait for the pace (a count: two may wait).
    closing: IndexMap<String, u32>,
    pace_tokens: f64,
    pace_at: f64,
    route: Option<Route>,
    /// When this client last sent channel 1, and last took it in this match (None: nothing yet); the ping of a
    /// stall's check, when sent.
    played: f64,
    match_heard: Option<f64>,
    stall_ping: Option<f64>,
    /// A cut of the faults is under way.
    in_cut: bool,
    stats: RelayStats,
    /// The game thread's copy of `stats`.
    shared_stats: Arc<Mutex<RelayStats>>,
    /// What a second connection starts with: the address, the key, the options, the direct host's key.
    address: String,
    key: RelayKey,
    options: ClientOptions,
    host_key: Option<String>,
    /// The relay's key id a second connection is pinned to: the one the first was pinned to, else the one the first
    /// connection's relay proved (a resume never signs in to another key at that address).
    relay_id: Option<String>,
    /// The connection has dropped (when, and why): no reads or writes on it, a resume under way.
    down: Option<(f64, String)>,
    /// A second connection under way, and when the next may start.
    attempt: Option<Attempt>,
    next_try: f64,
    /// The channel-0 frames sent on the seat (the resume's ring) and taken.
    ring: Ring,
    taken: u64,
    /// The relay's resent frames still to skip after a resume (taken on the old connection after the hello).
    skip: u64,
    /// The calls made while the connection is down, sent once it is back.
    held: Vec<(i64, Vec<u8>)>,
    faults: Faults,
}

/// `REFEREE_ID`: a relay's referee's id (`referee_id`): its owner's id (a player's, or since wire 5 a host core's),
/// then `.m` and the match's number.
fn is_referee_id(id: &str) -> bool {
    let Some((owner, n)) = id.rsplit_once(".m") else { return false };
    let hex = owner.strip_prefix('r').unwrap_or("");
    let player = hex.len() == 32 && hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
    (player || is_host_id(owner)) && !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())
}

impl Net {
    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1000.0
    }

    fn run(mut self) {
        let rest = std::mem::take(&mut self.w.rest);
        for f in rest {
            if self.gone.is_none() {
                self.frame(f.ty, &f.body);
            }
        }
        self.probe();
        let mut buf = vec![0u8; 1 << 16];
        while self.gone.is_none() {
            loop {
                match self.cmds.try_recv() {
                    Ok(Cmd::Shutdown) | Err(TryRecvError::Disconnected) => return self.shutdown(),
                    Ok(c) => self.cmd(c),
                    Err(TryRecvError::Empty) => break,
                }
            }
            self.read_tcp(&mut buf);
            self.read_udp(&mut buf);
            self.faults_due();
            self.timers();
            self.flush();
            self.publish();
            if self.gone.is_some() {
                break;
            }
            std::thread::sleep(LOOP_SLEEP);
        }
        self.report();
    }

    fn publish(&self) {
        *self.shared_stats.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = self.stats;
    }

    fn report(&self) {
        let s = self.stats;
        self.publish();
        eprintln!(
            "relay: {} channel 0 packets, {} channel 1 ({} by UDP, {} by TCP, {} both), {} refused, {} resumes",
            s.messages, s.matched, s.udp, s.tcp_match, s.both, s.refused, s.resumes
        );
    }

    fn cmd(&mut self, c: Cmd) {
        match c {
            Cmd::Request { req, reply } => self.request(req, reply),
            Cmd::Send { to, channel, bytes } => {
                self.send(&to, channel, &bytes);
            }
            Cmd::ClosePeer(peer) => self.close_peer(&peer),
            Cmd::Ask { tag, req } => self.request_as(req, None, Some(tag), None),
            Cmd::Presence { tag, p } => self.set_presence(tag, p),
            Cmd::Accept(ids) => self.accepted = Some(ids),
            Cmd::Match { referee, to_game, hosted } => {
                let now = self.now();
                self.faults.go.get_or_insert(now);
                let hosted = hosted.map(|HostedMatch { slots, humans, tick0 }| Hosted {
                    referee: Referee::new(slots, &humans.iter().map(|h| h.0).collect::<Vec<_>>(), tick0),
                    slots: humans.iter().map(|(s, id)| (id.clone(), *s)).collect(),
                    ids: humans.into_iter().collect(),
                    pumped: now,
                    failed: false,
                });
                self.route = Some(Route { referee, to_game, hosted });
                self.match_heard = None;
            }
            Cmd::MatchPacket(bytes) => self.game_packet(bytes),
            Cmd::MatchEnd => {
                self.route = None;
                self.match_heard = None;
            }
            Cmd::CutSocket => {
                let _ = self.w.tcp.shutdown(Shutdown::Both);
            }
            Cmd::Resume => {
                if self.down.is_none() && self.attempt.is_none() {
                    let now = self.now();
                    self.begin_attempt(now);
                }
            }
            Cmd::Shutdown => unreachable!("taken in run"),
        }
    }

    // ---- the TCP connection

    /// The connection takes reads and writes: up, and not held by a cut.
    fn live(&self) -> bool {
        self.gone.is_none() && self.down.is_none() && !self.faults.cut(self.now())
    }

    /// A plain frame's bytes (`relay_wire::frame`) to the TCP connection, sealed: [`Net::write_frame`].
    fn write(&mut self, b: &[u8]) {
        if let Some((&ty, body)) = b.get(4..).and_then(<[u8]>::split_first) {
            self.write_frame(ty, &[body]);
        }
    }

    /// A frame of type `ty` whose body is `parts` to the TCP connection, sealed as it is made, so that the frames
    /// keep their counters' order also when the faults hold them (dropped while the connection is down: a resume
    /// resends what must not be lost; one too long for a frame is dropped).
    fn write_frame(&mut self, ty: u8, parts: &[&[u8]]) {
        if self.gone.is_some() || self.down.is_some() {
            return;
        }
        if parts.iter().map(|p| p.len()).sum::<usize>() + 1 > MAX_FRAME {
            return;
        }
        let now = self.now();
        let sealed = match self.faults.tcp_wait(now) {
            Some(at) => {
                let mut b = Vec::new();
                let r = self.w.tx.frame_into(&mut b, ty, parts);
                if r.is_ok() {
                    self.faults.tcp_out.push_back((at, b));
                }
                r.map(|()| false)
            }
            None => self.w.tx.frame_into(&mut self.out, ty, parts).map(|()| true),
        };
        match sealed {
            Ok(true) => self.flush(),
            Ok(false) => {}
            Err(_) => self.lose("this connection's sealing is spent"),
        }
    }

    fn flush(&mut self) {
        while !self.out.is_empty() && self.live() {
            match self.w.tcp.write(&self.out) {
                Ok(0) => return self.drop_link("the relay closed the connection"),
                Ok(n) => {
                    self.out.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return self.drop_link(&socket_why(&e)),
            }
        }
    }

    fn read_tcp(&mut self, buf: &mut [u8]) {
        while self.live() {
            let n = match self.w.tcp.read(buf) {
                Ok(0) => return self.drop_link("the relay closed the connection"),
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return self.drop_link(&socket_why(&e)),
            };
            let now = self.now();
            match self.faults.tcp_wait_in(now) {
                Some(at) => self.faults.tcp_in.push_back((at, buf[..n].to_vec())),
                None => self.take_bytes(&buf[..n]),
            }
        }
    }

    /// Bytes read off the TCP connection, into its frames.
    fn take_bytes(&mut self, bytes: &[u8]) {
        self.w.reader.feed(bytes);
        while self.gone.is_none() && self.down.is_none() {
            let Some(f) = self.w.reader.next() else { break };
            self.frame(f.ty, &f.body);
        }
        if self.w.reader.bad && self.gone.is_none() && self.down.is_none() {
            self.lose("the relay sent a bad frame");
        }
    }

    fn frame(&mut self, ty: u8, body: &[u8]) {
        self.last_heard = self.now();
        if ty == FT_DATA {
            // (resent by the relay, already taken and counted on the old connection: each frame counts once)
            if self.skip > 0 {
                self.skip -= 1;
                return;
            }
            self.taken += 1;
        }
        if ty == FT_DATA || ty == FT_MATCH {
            if let Some(p) = read_packet(body) {
                let channel = if ty == FT_DATA { CHANNEL_MESSAGES } else { CHANNEL_MATCH };
                self.packet(channel, p.handle, p.bytes);
            }
            return;
        }
        if ty != FT_JSON {
            return;
        }
        let Some(m) = read_json(body) else { return };
        let str_of = |k: &str| m.get(k).and_then(Value::as_str).map(str::to_owned);
        match m.get("t").and_then(Value::as_str).unwrap_or("") {
            "res" => self.answer(&m),
            "lobby" => {
                let Some(lobby) = str_of("lobby") else { return };
                let members = m
                    .get("members")
                    .and_then(Value::as_array)
                    .and_then(|a| a.iter().map(|v| v.as_str().map(str::to_owned)).collect::<Option<Vec<String>>>());
                if let Some(members) = members {
                    self.lobbies.insert(lobby.clone(), members.clone());
                    self.event(RelayEvent::Lobby { lobby, members: Some(members), closed: None });
                } else if let Some(closed) = str_of("closed") {
                    self.lobbies.shift_remove(&lobby);
                    self.event(RelayEvent::Lobby { lobby, members: None, closed: Some(closed) });
                }
            }
            "peer" => {
                let handle = m.get("handle").and_then(Value::as_u64).and_then(|h| u32::try_from(h).ok());
                let (Some(handle), Some(id)) = (handle, str_of("id")) else { return };
                if let Some(old) = self.in_by_id.get(&id) {
                    self.in_handles.shift_remove(old);
                }
                self.in_handles.insert(handle, id.clone());
                self.in_by_id.insert(id, handle);
            }
            "peerClosed" => {
                let Some(id) = str_of("id") else { return };
                self.links.shift_remove(&id);
                self.free_peer(&id);
                self.event(RelayEvent::PeerClosed(id));
            }
            "invite" => {
                if let (Some(from), Some(payload)) = (str_of("from"), str_of("payload")) {
                    self.event(RelayEvent::Invite { from, payload });
                }
            }
            "listing" => {
                if let Ok(WireMessage::Listing { lobby, info }) = serde_json::from_value(Value::Object(m.clone())) {
                    self.event(RelayEvent::Listing { lobby, info });
                }
            }
            "ping" => self.write_json(&ClientMessage::Pong),
            "pong" => {
                if let Some(at) = self.pings.pop_front() {
                    let now = self.now();
                    self.timed(now - at);
                }
            }
            "refused" => {
                let why = str_of("why").unwrap_or_else(|| "no reason given".to_owned());
                let why = format!("{DROPPED}{why}");
                // (a second connection under way has taken the seat over: its welcome is waited for)
                if self.attempt.is_some() && str_of("why").as_deref() == Some(RESUMED_ELSEWHERE) {
                    self.drop_link(&why)
                } else {
                    self.lose(&why)
                }
            }
            _ => {}
        }
    }

    fn write_json(&mut self, m: &ClientMessage) {
        if matches!(m, ClientMessage::Ping) && self.gone.is_none() && self.down.is_none() {
            let now = self.now();
            self.pings.push_back(now);
        }
        let body = serde_json::to_vec(m).expect("a control message serialises");
        self.write_frame(FT_JSON, &[&body]);
    }

    fn event(&self, e: RelayEvent) {
        let _ = self.events.send(e);
    }

    // ---- calls

    fn request(&mut self, req: Request, reply: Option<Sender<Result<Value, RelayError>>>) {
        self.request_as(req, reply, None, None);
    }

    /// A call sent: its answer to `reply`, or as an `Answer` event of `tag`; `what` is what the thread does with it
    /// beside (by the request's op when None).
    fn request_as(
        &mut self,
        req: Request,
        reply: Option<Sender<Result<Value, RelayError>>>,
        tag: Option<u32>,
        what: Option<Asked>,
    ) {
        if let Some(why) = &self.gone {
            if let Some(r) = reply {
                let _ = r.send(Err(RelayError::Closed(why.clone())));
            }
            if let Some(tag) = tag {
                self.event(RelayEvent::Answer { tag, result: Err(RelayError::Closed(why.clone())) });
            }
            return;
        }
        let what = what.unwrap_or_else(|| match &req {
            Request::Create { .. } => Asked::Create,
            Request::Join { lobby } => Asked::Join(lobby.clone()),
            Request::Leave { lobby } => Asked::Leave(lobby.clone()),
            _ => Asked::Other,
        });
        let id = self.next_req;
        self.next_req += 1;
        let at = self.now();
        self.reqs.insert(id, Pending { reply, tag, at, what });
        if self.down.is_some() {
            // (sent once the connection is back; its wait counts from now)
            if let Ok(f) = json_frame(&ClientMessage::Req { id, req }) {
                self.held.push((id, f));
            }
            return;
        }
        self.write_json(&ClientMessage::Req { id, req });
    }

    fn answer(&mut self, m: &Map<String, Value>) {
        let Some(id) = m.get("id").and_then(Value::as_i64) else { return };
        let Some(p) = self.reqs.shift_remove(&id) else { return };
        let r = if m.get("ok") == Some(&Value::Bool(true)) {
            Ok(m.get("value").cloned().unwrap_or(Value::Null))
        } else {
            let why = m.get("error").and_then(Value::as_str).unwrap_or("refused");
            Err(RelayError::Refused(why.to_owned()))
        };
        self.answered(p, r);
    }

    fn answered(&mut self, p: Pending, r: Result<Value, RelayError>) {
        match (&p.what, &r) {
            (Asked::Create, Ok(v)) => {
                if let Some(l) = v.get("lobby").and_then(Value::as_str) {
                    self.lobbies.insert(l.to_owned(), vec![self.w.id.clone()]);
                }
            }
            (Asked::Join(l), Ok(v)) => {
                let members = v
                    .get("members")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>());
                let mut members = members.unwrap_or_default();
                if let Some(o) = v.get("owner").and_then(Value::as_str)
                    && !members.iter().any(|m| m == o)
                {
                    members.push(o.to_owned());
                }
                self.lobbies.insert(l.clone(), members);
            }
            (Asked::Leave(l), _) => {
                self.lobbies.shift_remove(l);
            }
            (Asked::Presence(waiting, sent), _) => {
                self.presence_busy = false;
                match &r {
                    // (over the relay's rate: again at the pace, unless a newer one is asked, which these sets wait for
                    // then)
                    Err(RelayError::Refused(e))
                        if e == LIMIT_EXCEEDED
                            && self.gone.is_none()
                            && (self.presence_wanted.is_some() || self.presence_tries < PRESENCE_TRIES) =>
                    {
                        if self.presence_wanted.is_none() {
                            self.presence_wanted = Some(sent.clone());
                        }
                        let mut w = waiting.clone();
                        w.append(&mut self.presence_waiting);
                        self.presence_waiting = w;
                    }
                    _ => {
                        for &tag in waiting {
                            self.event(RelayEvent::Answer { tag, result: r.clone() });
                        }
                    }
                }
                self.send_presence();
            }
            _ => {}
        }
        if let Some(tag) = p.tag {
            self.event(RelayEvent::Answer { tag, result: r.clone() });
        }
        if let Some(reply) = p.reply {
            let _ = reply.send(r);
        }
    }

    // ---- presence

    /// `setPresence`: the newest asked goes at the pace.
    fn set_presence(&mut self, tag: u32, p: WirePresence) {
        if let Some(why) = &self.gone {
            let result = Err(RelayError::Closed(why.clone()));
            return self.event(RelayEvent::Answer { tag, result });
        }
        self.presence_wanted = Some(p);
        self.presence_tries = 0;
        self.presence_waiting.push(tag);
        self.send_presence();
    }

    fn send_presence(&mut self) {
        if self.gone.is_some() || self.presence_busy || self.presence_wanted.is_none() {
            return;
        }
        // (not due yet: `timers` tries again)
        if self.now() < self.presence_sent_at + PRESENCE_EVERY_MS {
            return;
        }
        let Some(p) = self.presence_wanted.take() else { return };
        let waiting = std::mem::take(&mut self.presence_waiting);
        self.presence_busy = true;
        self.presence_sent_at = self.now();
        self.presence_tries += 1;
        self.request_as(Request::Presence { presence: p.clone() }, None, None, Some(Asked::Presence(waiting, p)));
    }

    // ---- timers

    fn timers(&mut self) {
        let now = self.now();
        self.resume_timers(now);
        if self.gone.is_some() {
            return;
        }
        // (what went on a connection that dropped is never answered: a resume's connection times its own)
        if self.down.is_some() {
            self.pings.clear();
            self.probes.clear();
        }
        if now - self.last_ping >= self.t.ping_ms && self.down.is_none() {
            self.last_ping = now;
            if now - self.last_heard > self.t.silent_ms {
                return self.drop_link("the relay went silent");
            }
            self.write_json(&ClientMessage::Ping);
        }
        if now - self.last_probe >= self.t.udp_probe_ms {
            self.probe();
        }
        if self.presence_wanted.is_some() {
            self.send_presence();
        }
        let late: Vec<i64> =
            self.reqs.iter().filter(|(_, p)| now - p.at >= self.t.request_ms).map(|(&id, _)| id).collect();
        for id in late {
            // (one made while the connection is down is not sent at the resume either)
            self.held.retain(|h| h.0 != id);
            if let Some(p) = self.reqs.shift_remove(&id) {
                self.answered(p, Err(RelayError::Timeout));
            }
        }
        if !self.pending.is_empty() {
            self.drain();
        }
        if let Some(h) = self.route.as_mut().and_then(|r| r.hosted.as_mut())
            && now - h.pumped >= REFEREE_PUMP_MS
        {
            h.pumped = now;
            self.pump(None);
        }
    }

    /// A probe, its flag set while acknowledgements come (the relay then sends us channel 1 by UDP).
    fn probe(&mut self) {
        let now = self.now();
        self.last_probe = now;
        if self.udp_blocked {
            return;
        }
        let heard = now - self.last_ack < self.t.udp_lost_ms;
        self.send_udp(UDP_PROBE, &[&[u8::from(heard)]]);
        if self.down.is_none() {
            while self.probes.front().is_some_and(|&at| now - at > PROBE_FORGET_MS) {
                self.probes.pop_front();
            }
            self.probes.push_back(now);
        }
        self.share_udp(now);
    }

    /// The UDP link's state for the game thread: on while acknowledgements come; not known before the first one
    /// within udp_lost_ms of the start; else off (blocked, no socket, or silent).
    fn share_udp(&self, now: f64) {
        let v = if self.udp_on() {
            UDP_ON
        } else if !self.udp_blocked
            && self.udp.is_some()
            && self.last_ack == f64::NEG_INFINITY
            && now - self.udp_since < self.t.udp_lost_ms
        {
            UDP_NOT_KNOWN
        } else {
            UDP_OFF
        };
        self.shared_udp.store(v, Ordering::Relaxed);
    }

    /// A round trip timed (a ping's pong, a probe's acknowledgement): the smallest so far is the game's.
    fn timed(&mut self, ms: f64) {
        if ms >= 0.0 && ms < self.rtt {
            self.rtt = ms;
            self.shared_rtt.store(ms.to_bits(), Ordering::Relaxed);
        }
    }

    /// A datagram to the relay of `kind` and `parts`, sealed, through the faults (a datagram the faults drop takes
    /// no counter; one the counter cannot seal any more is not sent).
    fn send_udp(&mut self, kind: u8, parts: &[&[u8]]) {
        if self.down.is_some() {
            return;
        }
        let now = self.now();
        let Some(at) = self.faults.udp_wait(now) else { return };
        self.w.tx.datagram_into(&mut self.datagram, &self.w.token, kind, parts);
        if self.datagram.is_empty() {
            return;
        }
        if at <= now {
            if let Some(u) = &self.udp {
                let _ = u.send_to(&self.datagram, self.w.udp_to);
            }
        } else {
            self.faults.udp_out.push((at, self.datagram.clone()));
        }
    }

    fn udp_on(&self) -> bool {
        !self.udp_blocked && self.udp.is_some() && self.now() - self.last_ack < self.t.udp_lost_ms
    }

    /// In a match, UDP still on but no acknowledgement for UDP_PROBE_MS + UDP_LATE_MS (a probe's interval and its
    /// lateness): channel 1 goes both ways.
    /// Deviation: the port's own rule, the relay's (its `u.at + UDP_PROBE_MS + UDP_LATE_MS`) on the client's side.
    fn udp_late(&self, now: f64) -> bool {
        self.in_match(now) && now - self.last_ack > self.t.udp_probe_ms + UDP_LATE_MS
    }

    /// This client is in a match: it sent channel 1 within MATCH_ON_MS, or the game gave it a route.
    fn in_match(&self, now: f64) -> bool {
        self.route.is_some() || now - self.played < MATCH_ON_MS
    }

    /// This client sends channel 1: a match begins when it had not (nothing taken in it yet), and the cut's clock
    /// starts at the first.
    fn playing(&mut self, now: f64) {
        if !self.in_match(now) {
            self.match_heard = None;
        }
        self.played = now;
        self.faults.go.get_or_insert(now);
    }

    fn read_udp(&mut self, buf: &mut [u8]) {
        loop {
            let Some(u) = &self.udp else { return };
            let (n, from) = match u.recv_from(buf) {
                Ok(x) => x,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                // (Windows tells a datagram's ICMP port unreachable at the next receive: not this one's)
                Err(e) if matches!(e.kind(), ErrorKind::ConnectionReset | ErrorKind::Interrupted) => continue,
                Err(_) => return,
            };
            if self.gone.is_some() || self.udp_blocked || self.down.is_some() {
                continue;
            }
            let now = self.now();
            match self.faults.udp_wait(now) {
                None => {}
                Some(at) if at <= now => self.datagram(&mut buf[..n], from),
                Some(at) => self.faults.udp_in.push((at, buf[..n].to_vec(), from)),
            }
        }
    }

    /// A datagram that came (opened in place).
    fn datagram(&mut self, b: &mut [u8], from: SocketAddr) {
        if self.gone.is_some() || self.down.is_some() {
            return;
        }
        // (from the relay's TCP address and UDP port only: compared as normal_address, as a dual-stack socket may name
        // one address two ways)
        if self.w.udp_from != Some(from) {
            if from.port() != self.w.udp_to.port()
                || normal_address(&from.ip().to_string()).as_deref() != Some(self.w.peer_ip.as_str())
            {
                return;
            }
            self.w.udp_from = Some(from);
        }
        if datagram_token(b) != Some(self.w.token) {
            return;
        }
        let Some(d) = self.w.datagrams.open(b).and_then(read_udp_body) else { return };
        if d.kind == UDP_PROBE_ACK {
            let now = self.now();
            self.last_ack = now;
            if let Some(at) = self.probes.pop_front() {
                self.timed(now - at);
            }
            self.share_udp(now);
        } else if d.kind == UDP_MATCH {
            self.packet(CHANNEL_MATCH, d.handle, d.bytes);
        }
    }

    /// The bytes and datagrams the faults held that are due.
    fn faults_due(&mut self) {
        let now = self.now();
        if self.faults.idle() {
            return;
        }
        let cut = self.faults.cut(now);
        if cut && !self.in_cut {
            self.stats.cuts += 1;
        }
        self.in_cut = cut;
        if !self.faults.cut(now) {
            while let Some((at, _)) = self.faults.tcp_out.front()
                && *at <= now
            {
                let (_, b) = self.faults.tcp_out.pop_front().expect("a front");
                self.out.extend_from_slice(&b);
            }
            while self.live()
                && let Some((at, _)) = self.faults.tcp_in.front()
                && *at <= now
            {
                let (_, b) = self.faults.tcp_in.pop_front().expect("a front");
                self.take_bytes(&b);
            }
        }
        for d in self.faults.due_out(now) {
            if let Some(u) = &self.udp {
                let _ = u.send_to(&d, self.w.udp_to);
            }
        }
        for (mut d, from) in self.faults.due_in(now) {
            self.datagram(&mut d, from);
        }
    }

    // ---- packets

    /// The link to `peer` is over: the handles naming it either way are freed.
    fn free_peer(&mut self, peer: &str) {
        self.out_handles.shift_remove(peer);
        if let Some(h) = self.in_by_id.shift_remove(peer) {
            self.in_handles.shift_remove(&h);
        }
    }

    fn accept(&self, from: &str) -> bool {
        match &self.accepted {
            Some(ids) => ids.contains(from),
            None => self.lobbies.values().any(|m| m.iter().any(|x| x == from)),
        }
    }

    fn packet(&mut self, channel: u8, handle: u32, bytes: &[u8]) {
        let Some(from) = self.in_handles.get(&handle).cloned() else { return };
        if !self.links.contains(&from) {
            // (a referee and a host core are the relay's own, not players that link)
            if !is_referee_id(&from) && !is_host_id(&from) && !self.accept(&from) {
                self.stats.refused += 1;
                let now = self.now();
                if self.refused_at.get(&from).is_none_or(|&last| now - last >= REFUSE_EVERY_MS) {
                    self.refused_at.insert(from.clone(), now);
                    self.write_json(&ClientMessage::ClosePeer { id: from.clone() });
                    self.free_peer(&from);
                }
                return;
            }
            self.links.insert(from.clone());
            self.refused_at.shift_remove(&from);
        }
        let now = self.now();
        if channel == CHANNEL_MATCH {
            self.match_heard = Some(now);
        }
        if channel == CHANNEL_MATCH
            && let Some(r) = &mut self.route
        {
            // a human's packet to this thread's referee
            if let Some(h) = &mut r.hosted
                && let Some(&slot) = h.slots.get(&from)
            {
                if packet_fits(bytes, h.referee.match_slots()) {
                    self.referee(|r, out| r.receive(slot, bytes, now, out));
                    // (what it ruled goes out now, not at the clock's next pump)
                    self.pump(None);
                }
                return;
            }
            // the referee's packet to this player's session
            if r.referee == from {
                // (of a match the game's session checks them against)
                if packet_fits(bytes, MAX_SLOTS) {
                    let _ = r.to_game.send(bytes.to_vec());
                }
                return;
            }
        }
        self.event(RelayEvent::Packet { from, channel, bytes: bytes.to_vec() });
    }

    /// A packet of the game's session: to this thread's referee as its own slot's, else to the referee's id.
    fn game_packet(&mut self, bytes: Vec<u8>) {
        let Some(r) = &mut self.route else { return };
        if let Some(h) = &mut r.hosted {
            let Some(&slot) = h.slots.get(&self.w.id) else { return };
            let fits = packet_fits(&bytes, h.referee.match_slots());
            let now = self.now();
            self.playing(now);
            if fits {
                self.referee(|r, out| r.receive(slot, &bytes, now, out));
                self.pump(None);
            }
            return;
        }
        let to = r.referee.clone();
        self.send(&to, CHANNEL_MATCH, &bytes);
    }

    /// This thread's referee pumped (`now` its clock; None: the thread's).
    fn pump(&mut self, now: Option<f64>) {
        let now = now.unwrap_or_else(|| self.now());
        self.referee(|r, out| r.pump(now, out));
    }

    /// A call of this thread's referee, what it sent routed. A panic is caught and logged once a match
    ///: the thread and the connection go on, the referee's next call tried as ever.
    fn referee(&mut self, call: impl FnOnce(&mut Referee, &mut Collect)) {
        let Some(h) = self.route.as_mut().and_then(|r| r.hosted.as_mut()) else { return };
        let mut out = Collect::default();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(&mut h.referee, &mut out)));
        if let Err(e) = res {
            if !h.failed {
                let why = e
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| e.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "a panic".to_owned());
                eprintln!("relay: the match's referee failed: {why}");
            }
            h.failed = true;
            return;
        }
        self.routed(out);
    }

    /// What this thread's referee sent: its own slot's to the game, the others' on channel 1.
    fn routed(&mut self, out: Collect) {
        let Some(r) = &self.route else { return };
        let Some(h) = &r.hosted else { return };
        let mut sends = Vec::new();
        for (slot, bytes) in out.sends {
            let Some(id) = h.ids.get(&slot) else { continue };
            if *id == self.w.id {
                let _ = r.to_game.send(bytes);
            } else {
                sends.push((id.clone(), bytes));
            }
        }
        let silent = out.silent;
        for (to, bytes) in sends {
            self.send(&to, CHANNEL_MATCH, &bytes);
        }
        for (slot, news) in silent {
            self.event(RelayEvent::Slot { slot: u32::try_from(slot).unwrap_or(u32::MAX), news });
        }
    }

    fn send(&mut self, to: &str, channel: u8, bytes: &[u8]) -> bool {
        if self.gone.is_some() || to.is_empty() || to == self.w.id || bytes.len() > P2P_MAX_PACKET {
            return false;
        }
        // (an empty channel-1 packet is a match's beat while the match goes another way, an Epic path's: the client
        // is in a match, its match heard when the relay was last heard at each beat, so a stall of the relay's link
        // is found at RESUME_TRY_MS the whole match long, and nothing is sent)
        if channel == CHANNEL_MATCH && bytes.is_empty() {
            let now = self.now();
            self.playing(now);
            let heard = self.match_heard.map_or(self.last_heard, |h| h.max(self.last_heard));
            self.match_heard = Some(heard);
            return true;
        }
        if self.out.len() > MAX_QUEUED {
            return false;
        }
        if channel != CHANNEL_MATCH {
            // (past its queue; a peer the relay could not name for us)
            if self.pending_bytes + bytes.len() > MESSAGE_QUEUE_BYTES
                || (!self.out_handles.contains_key(to) && self.out_handles.len() >= MAX_PEER_HANDLES)
            {
                return false;
            }
            self.links.insert(to.to_owned());
            self.stats.messages += 1;
            self.pace_write(Paced::Data { to: to.to_owned(), bytes: bytes.to_vec() });
            return true;
        }
        let now = self.now();
        self.playing(now);
        // (a peer whose closePeer still waits for channel 0's pace: its new handle's peer message waits behind it; a
        // connection that is down: lost, as on a lossy way)
        if self.closing.contains_key(to) || self.down.is_some() {
            return false;
        }
        let fresh = !self.out_handles.contains_key(to);
        let Some(h) = self.handle(to) else { return false };
        self.links.insert(to.to_owned());
        self.stats.matched += 1;
        // (a new handle's first packet goes on TCP behind its peer message: a datagram could pass it and be dropped as
        // of an unknown handle; while the probes go unanswered, on TCP too)
        let udp = self.udp_on() && !fresh;
        let tcp = !udp || self.udp_late(now);
        if udp {
            self.stats.udp += 1;
            self.send_udp(UDP_MATCH, &[&h.to_be_bytes(), bytes]);
        }
        if tcp {
            self.stats.tcp_match += 1;
            if udp {
                self.stats.both += 1;
            }
            self.write_frame(FT_MATCH, &[&h.to_be_bytes(), bytes]);
        }
        true
    }

    /// Our handle for `to`, named to the relay by a peer message first when it is new; None past MAX_PEER_HANDLES
    /// live ones (the relay holds no more).
    fn handle(&mut self, to: &str) -> Option<u32> {
        if let Some(&h) = self.out_handles.get(to) {
            return Some(h);
        }
        if self.out_handles.len() >= MAX_PEER_HANDLES {
            return None;
        }
        let h = self.next_handle;
        self.next_handle += 1;
        self.out_handles.insert(to.to_owned(), h);
        self.write_json(&ClientMessage::Peer { handle: h, id: to.to_owned() });
        Some(h)
    }

    fn close_peer(&mut self, peer: &str) {
        let had =
            self.links.shift_remove(peer) || self.out_handles.contains_key(peer) || self.in_by_id.contains_key(peer);
        if let Some(h) = self.in_by_id.shift_remove(peer) {
            self.in_handles.shift_remove(&h);
        }
        // (behind the peer's channel 0 frames that wait for the pace; our handle for it is freed as it goes)
        if had {
            *self.closing.entry(peer.to_owned()).or_insert(0) += 1;
            self.pace_write(Paced::Close(peer.to_owned()));
        }
    }

    /// The pace's tokens now.
    fn paced(&mut self) -> f64 {
        let now = self.now();
        let rate = self.t.message_bytes_per_sec;
        self.pace_tokens = rate.min(self.pace_tokens + ((now - self.pace_at) / 1000.0) * rate);
        self.pace_at = now;
        self.pace_tokens
    }

    /// A frame of channel 0's order: behind the others, written as the pace has room and the connection is up.
    fn pace_write(&mut self, p: Paced) {
        self.pending_bytes += p.cost();
        self.pending.push_back(p);
        self.drain();
    }

    /// The waiting frames that the pace has room for go.
    fn drain(&mut self) {
        while self.gone.is_none() && self.down.is_none() {
            let Some(cost) = self.pending.front().map(Paced::cost) else { return };
            #[allow(clippy::cast_precision_loss, reason = "a frame's bytes")]
            let c = cost as f64;
            if self.paced() < c {
                return;
            }
            let p = self.pending.pop_front().expect("a front");
            self.pace_tokens -= c;
            self.pending_bytes -= cost;
            match p {
                Paced::Data { to, bytes } => {
                    // (no handle to be had: dropped, as a send refused)
                    let Some(h) = self.handle(&to) else { continue };
                    if bytes.len() + 5 <= MAX_FRAME {
                        self.write_frame(FT_DATA, &[&h.to_be_bytes(), &bytes]);
                        self.ring.sent(to, bytes);
                    }
                }
                Paced::Close(peer) => {
                    self.write_json(&ClientMessage::ClosePeer { id: peer.clone() });
                    self.out_handles.shift_remove(&peer);
                    self.ring.closed(&peer);
                    if let Some(n) = self.closing.get_mut(&peer) {
                        *n -= 1;
                        if *n == 0 {
                            self.closing.shift_remove(&peer);
                        }
                    }
                }
            }
        }
    }

    // ---- the end

    /// The connection ends: the pending calls fail with `why`, and the game is told unless it asked for the end.
    fn lose(&mut self, why: &str) {
        if self.gone.is_some() {
            return;
        }
        self.gone = Some(why.to_owned());
        let closed = || Err(RelayError::Closed(why.to_owned()));
        let mut tags: Vec<u32> = std::mem::take(&mut self.presence_waiting);
        self.presence_wanted = None;
        for (_, p) in self.reqs.drain(..) {
            if let Some(r) = p.reply {
                let _ = r.send(closed());
            }
            if let Some(tag) = p.tag {
                tags.push(tag);
            }
            if let Asked::Presence(w, _) = p.what {
                tags.extend(w);
            }
        }
        for tag in tags {
            let _ = self.events.send(RelayEvent::Answer { tag, result: closed() });
        }
        self.links.clear();
        self.pending.clear();
        self.pending_bytes = 0;
        self.closing.clear();
        self.held.clear();
        if let Some(a) = self.attempt.take() {
            a.end();
        }
        self.udp = None;
        let _ = self.w.tcp.shutdown(Shutdown::Both);
        if !self.ending {
            self.event(RelayEvent::SignedOut(why.to_owned()));
        }
    }

    /// `shutdown`: what waits to go is written, then a bye (the relay keeps no seat for a resume), and the relay's
    /// close waited for a moment. A connection that is down ends as it is.
    fn shutdown(&mut self) {
        if self.gone.is_some() {
            return self.report();
        }
        self.ending = true;
        if self.down.is_some() {
            self.report();
            return self.lose("shut down");
        }
        let until = Instant::now() + Duration::from_millis(SHUTDOWN_MS);
        while !self.pending.is_empty() && Instant::now() < until && self.gone.is_none() && self.down.is_none() {
            self.drain();
            std::thread::sleep(LOOP_SLEEP);
        }
        self.write_json(&ClientMessage::Bye);
        // (the bytes a delay holds go at once)
        while let Some((_, b)) = self.faults.tcp_out.pop_front() {
            self.out.extend_from_slice(&b);
        }
        while !self.out.is_empty() && Instant::now() < until && self.gone.is_none() && self.down.is_none() {
            self.flush();
            std::thread::sleep(LOOP_SLEEP);
        }
        let _ = self.w.tcp.shutdown(Shutdown::Write);
        let mut buf = [0u8; 4096];
        while Instant::now() < until {
            match self.w.tcp.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                    std::thread::sleep(LOOP_SLEEP);
                }
                Err(_) => break,
            }
        }
        self.report();
        self.lose("shut down");
    }
}

/// One signed-in relay connection, from the game thread: its net thread runs from
/// [`RelayClient::connect`] to [`RelayClient::shutdown`] or the connection's end.
#[derive(Debug)]
pub struct RelayClient {
    me: String,
    relay_key: String,
    version: u32,
    /// The game protocol of the cores the relay hosts, as its welcome said (None: it did not say).
    proto: Option<u32>,
    ms: f64,
    request_ms: f64,
    cmds: Sender<Cmd>,
    events: Receiver<RelayEvent>,
    thread: Option<JoinHandle<()>>,
    /// The net thread's end is told (`poll`'s SignedOut).
    dead: std::cell::Cell<bool>,
    stats: Arc<Mutex<RelayStats>>,
    /// The net thread's smallest round trip (f64 bits, NO_RTT none).
    rtt: Arc<AtomicU64>,
    /// The net thread's UDP link state (UDP_NOT_KNOWN, UDP_OFF, UDP_ON).
    udp: Arc<AtomicU8>,
    /// A relay chosen by its address: its presence, its referee and its online list (not a direct host's).
    pub(crate) by_address: bool,
    /// The Backend side: the calls' tags and what they answer, the links as the events told, the events made at once.
    pub(crate) next_tag: u32,
    pub(crate) asked: std::collections::HashMap<u32, backend::Kind>,
    pub(crate) linked: IndexSet<String>,
    pub(crate) made: std::collections::VecDeque<crate::backend::BackendEvent>,
    pub(crate) refused_words: mvs_protocol::names::Profanity,
}

/// A connection's stats, read from another owner than the client's (`RelayClient::stats_reader`).
#[derive(Clone, Debug)]
pub struct StatsReader(Arc<Mutex<RelayStats>>);

impl StatsReader {
    pub fn get(&self) -> RelayStats {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A match's packets between the game's session and its referee (`MatchLink`): through the net thread.
#[derive(Debug)]
pub struct RelayMatchLink {
    cmds: Sender<Cmd>,
    from: Receiver<Vec<u8>>,
}

impl MatchLink for RelayMatchLink {
    fn send(&mut self, bytes: &[u8], _now: f64) {
        let _ = self.cmds.send(Cmd::MatchPacket(bytes.to_vec()));
    }

    fn recv(&mut self, _now: f64) -> Option<Vec<u8>> {
        self.from.try_recv().ok()
    }
}

impl RelayClient {
    /// `connectRelayWith`: signed in to the relay at `address` under `key`, its net thread started.
    pub fn connect(address: &str, key: &RelayKey, o: &ClientOptions) -> Result<RelayClient, RelayError> {
        RelayClient::connect_with(address, key, o, &StartWith::default())
    }

    /// `connect` with a host key (a direct host's joiner: no presence, no referee of the relay's), a claim and a
    /// cancel.
    pub fn connect_with(
        address: &str,
        key: &RelayKey,
        o: &ClientOptions,
        x: &StartWith,
    ) -> Result<RelayClient, RelayError> {
        let w = start(address, key, o, x, None)?;
        w.tcp.set_nonblocking(true).map_err(|e| RelayError::Start(socket_why(&e)))?;
        w.tcp.set_read_timeout(None).map_err(|e| RelayError::Start(socket_why(&e)))?;
        let udp = if o.udp_blocked {
            None
        } else {
            // (a relay on loopback: a socket on loopback, so that nothing binds every address in a local run)
            let any: SocketAddr = if w.udp_to.ip().is_loopback() {
                if w.udp_to.is_ipv6() { "[::1]:0" } else { "127.0.0.1:0" }
            } else if w.udp_to.is_ipv6() {
                "[::]:0"
            } else {
                "0.0.0.0:0"
            }
            .parse()
            .expect("an address");
            let u = UdpSocket::bind(any).map_err(|e| RelayError::Start(socket_why(&e)))?;
            u.set_nonblocking(true).map_err(|e| RelayError::Start(socket_why(&e)))?;
            Some(u)
        };
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        let me = w.id.clone();
        let (relay_key, version, proto, ms) = (w.relay_key.clone(), w.version, w.proto, w.ms);
        let now = o.epoch.elapsed().as_secs_f64() * 1000.0;
        let stats = Arc::new(Mutex::new(RelayStats::default()));
        let rtt = Arc::new(AtomicU64::new(NO_RTT));
        let udp_state = Arc::new(AtomicU8::new(if o.udp_blocked || udp.is_none() { UDP_OFF } else { UDP_NOT_KNOWN }));
        // (pinned or not, a resume must find the key this start proved)
        let proved = B64.decode(&w.relay_key).ok().map(|r| key_id(&r));
        let net = Net {
            w,
            t: o.times,
            epoch: o.epoch,
            udp,
            udp_blocked: o.udp_blocked,
            out: Vec::new(),
            datagram: Vec::with_capacity(MAX_SEALED_DATAGRAM),
            gone: None,
            ending: false,
            events: ev_tx,
            cmds: cmd_rx,
            reqs: IndexMap::new(),
            next_req: 1,
            out_handles: IndexMap::new(),
            next_handle: 1,
            in_handles: IndexMap::new(),
            in_by_id: IndexMap::new(),
            links: IndexSet::new(),
            refused_at: IndexMap::new(),
            lobbies: IndexMap::new(),
            accepted: None,
            presence_wanted: None,
            presence_waiting: Vec::new(),
            presence_sent_at: f64::NEG_INFINITY,
            presence_busy: false,
            presence_tries: 0,
            last_heard: now,
            last_ack: f64::NEG_INFINITY,
            last_ping: now,
            last_probe: now,
            pings: VecDeque::new(),
            probes: VecDeque::new(),
            rtt: f64::INFINITY,
            shared_rtt: rtt.clone(),
            udp_since: now,
            shared_udp: udp_state.clone(),
            pending: VecDeque::new(),
            pending_bytes: 0,
            closing: IndexMap::new(),
            pace_tokens: o.times.message_bytes_per_sec,
            pace_at: now,
            route: None,
            played: f64::NEG_INFINITY,
            match_heard: None,
            stall_ping: None,
            in_cut: false,
            stats: RelayStats::default(),
            shared_stats: stats.clone(),
            address: address.to_owned(),
            key: key.clone(),
            options: o.clone(),
            host_key: x.host_key.clone(),
            relay_id: x.relay_id.clone().or(proved),
            down: None,
            attempt: None,
            next_try: f64::NEG_INFINITY,
            ring: Ring::default(),
            taken: 0,
            skip: 0,
            held: Vec::new(),
            faults: Faults::new(o.faults, &me),
        };
        let thread = std::thread::Builder::new()
            .name("mvs-net".to_owned())
            .spawn(move || net.run())
            .map_err(|e| RelayError::Start(e.to_string()))?;
        Ok(RelayClient {
            me,
            relay_key,
            version,
            proto,
            ms,
            request_ms: o.times.request_ms,
            cmds: cmd_tx,
            events: ev_rx,
            thread: Some(thread),
            dead: std::cell::Cell::new(false),
            stats,
            rtt,
            udp: udp_state,
            by_address: x.host_key.is_none(),
            next_tag: 1,
            asked: std::collections::HashMap::new(),
            linked: IndexSet::new(),
            made: std::collections::VecDeque::new(),
            refused_words: mvs_protocol::names::Profanity::new(&mvs_protocol::names::BAD_WORDS),
        })
    }

    /// This player's id on the relay.
    pub fn me(&self) -> &str {
        &self.me
    }

    /// The id of the key the relay proved (`relay_wire::key_id`).
    pub fn relay_id(&self) -> String {
        B64.decode(&self.relay_key).map(|r| key_id(&r)).unwrap_or_default()
    }

    /// The relay's key (raw, base64), its wire version and the hello's time to its challenge (ms).
    pub fn relay(&self) -> (&str, u32, f64) {
        (&self.relay_key, self.version, self.ms)
    }

    /// The game protocol of the cores the relay hosts, as its welcome said (None: it did not say: an older relay).
    pub fn proto(&self) -> Option<u32> {
        self.proto
    }

    /// What the connection has sent and refused so far (the net thread's count, a millisecond late at the most).
    pub fn stats(&self) -> RelayStats {
        *self.stats.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The smallest round trip to the relay timed so far, in ms (a ping to its pong on TCP, a UDP probe to its
    /// acknowledgement); None before the first. A queue join carries it.
    pub fn rtt(&self) -> Option<f64> {
        let bits = self.rtt.load(Ordering::Relaxed);
        (bits != NO_RTT).then(|| f64::from_bits(bits))
    }

    /// The UDP link to the relay: on (its probes acknowledged within udp_lost_ms), off (blocked, no socket, or
    /// silent past that), None while not known yet (the first udp_lost_ms with no acknowledgement).
    pub fn udp(&self) -> Option<bool> {
        match self.udp.load(Ordering::Relaxed) {
            UDP_ON => Some(true),
            UDP_OFF => Some(false),
            _ => None,
        }
    }

    /// (tests) The TCP connection ends as a network's drop would end it: no bye, a resume follows.
    pub fn cut_socket(&self) {
        let _ = self.cmds.send(Cmd::CutSocket);
    }

    /// (tests) A second connection starts now beside the first, which goes on meanwhile.
    pub fn resume_now(&self) {
        let _ = self.cmds.send(Cmd::Resume);
    }

    /// (tests) The stats as `stats` gives them, readable once the client is the net service's backend.
    pub fn stats_reader(&self) -> StatsReader {
        StatsReader(self.stats.clone())
    }

    /// A lobby call, its answer waited for (`request`).
    pub fn request(&self, req: Request) -> Result<Value, RelayError> {
        let (tx, rx) = mpsc::channel();
        self.cmds.send(Cmd::Request { req, reply: Some(tx) }).map_err(|_| self.closed())?;
        // (the thread answers a call it did not hear back on within request_ms: this wait is its bound)
        let wait = Duration::from_secs_f64(self.request_ms / 1000.0 + 1.0);
        match rx.recv_timeout(wait) {
            Ok(r) => r,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(RelayError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(self.closed()),
        }
    }

    fn closed(&self) -> RelayError {
        RelayError::Closed(crate::fail::RELAY_CLOSED.to_owned())
    }

    /// A call whose answer comes as `RelayEvent::Answer` of the tag it returns.
    pub(crate) fn ask(&mut self, req: Request) -> u32 {
        let tag = self.next_tag;
        self.next_tag += 1;
        if self.cmds.send(Cmd::Ask { tag, req }).is_err() {
            let why = self.closed().to_string();
            self.made.push_back(crate::backend::BackendEvent::Answered(crate::backend::Ask(tag), Err(why)));
        }
        tag
    }

    /// `createLobby`: its id.
    pub fn create_lobby(&self, bucket: &str, max: u32) -> Result<String, RelayError> {
        let v = self.request(Request::Create { bucket: bucket.to_owned(), max, listed: None, host: false })?;
        v.get("lobby").and_then(Value::as_str).map(str::to_owned).ok_or(RelayError::Answer("create"))
    }

    /// `joinLobby`: its owner and members.
    pub fn join_lobby(&self, id: &str) -> Result<Joined, RelayError> {
        let v = self.request(Request::Join { lobby: id.to_owned() })?;
        serde_json::from_value(v).map_err(|_| RelayError::Answer("join"))
    }

    /// `leaveLobby`.
    pub fn leave_lobby(&self, id: &str) -> Result<(), RelayError> {
        self.request(Request::Leave { lobby: id.to_owned() }).map(|_| ())
    }

    /// `send`: one packet (at most [`P2P_MAX_PACKET`] bytes) to another player on a channel.
    pub fn send(&self, to: &str, channel: u8, bytes: &[u8]) {
        let _ = self.cmds.send(Cmd::Send { to: to.to_owned(), channel, bytes: bytes.to_vec() });
    }

    /// `closePeer`.
    pub fn close_peer(&self, peer: &str) {
        let _ = self.cmds.send(Cmd::ClosePeer(peer.to_owned()));
    }

    /// The next thing the relay said, if any.
    pub fn poll(&self) -> Option<RelayEvent> {
        self.poll_all()
    }

    pub(crate) fn poll_all(&self) -> Option<RelayEvent> {
        match self.events.try_recv() {
            Ok(e) => {
                if matches!(e, RelayEvent::SignedOut(_)) {
                    self.dead.set(true);
                }
                Some(e)
            }
            Err(TryRecvError::Empty) => None,
            // (the net thread is gone without its sign-out told: a panic)
            Err(TryRecvError::Disconnected) if !self.dead.replace(true) => {
                Some(RelayEvent::SignedOut("the connection's thread stopped".to_owned()))
            }
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// A match the net thread referees itself: its slot count (its bots' among them), its humans (slot, id), its tick
    /// 0 on the client's clock. Its referee's id is this player's.
    pub fn host_match(&self, slots: usize, humans: Vec<(usize, String)>, tick0: f64) -> RelayMatchLink {
        let (tx, rx) = mpsc::channel();
        let hosted = Some(HostedMatch { slots, humans, tick0 });
        let _ = self.cmds.send(Cmd::Match { referee: self.me.clone(), to_game: tx, hosted });
        RelayMatchLink { cmds: self.cmds.clone(), from: rx }
    }

    /// A guest's match: its packets to and from `referee`.
    pub fn join_match(&self, referee: &str) -> RelayMatchLink {
        self.route(referee.to_owned(), None)
    }

    fn route(&self, referee: String, hosted: Option<HostedMatch>) -> RelayMatchLink {
        let (tx, rx) = mpsc::channel();
        let _ = self.cmds.send(Cmd::Match { referee, to_game: tx, hosted });
        RelayMatchLink { cmds: self.cmds.clone(), from: rx }
    }

    /// The match is over: its packets are no more routed, a hosted referee goes.
    pub fn end_match(&self) {
        let _ = self.cmds.send(Cmd::MatchEnd);
    }

    /// `shutdown`: the connection ended without a sign-out, its thread joined.
    pub fn shutdown(self) {
        // (the drop waits for the bye)
        let _ = self.cmds.send(Cmd::Shutdown);
    }

    /// `connect_with` on a thread of its own: the result is polled (the service's thread never waits on the network);
    /// the pending's cancel ends the start.
    pub fn begin_connect(
        address: String,
        key: RelayKey,
        o: ClientOptions,
        x: StartWith,
    ) -> crate::pending::Pending<Result<RelayClient, RelayError>> {
        crate::pending::Pending::start(move |cancel| {
            let x = StartWith { cancel: Some(cancel), ..x };
            RelayClient::connect_with(&address, &key, &o, &x)
        })
    }
}

/// What the launcher's look at an address found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayProbe {
    Failed(String),
    /// The relay's wire version, its id, the hello's time to its challenge (ms), whether a UDP probe came back and
    /// the game protocol of the cores it hosts (None: it did not say: an older relay).
    Found {
        version: u32,
        relay: String,
        ms: u64,
        udp: bool,
        proto: Option<u32>,
    },
}

/// The launcher's look at an address. It signs in under a key of its own, made for the
/// probe (a relay ends an id's first connection at its second), and sends UDP probes for `probe_udp_ms`.
pub fn probe_relay(address: &str, o: &ClientOptions) -> RelayProbe {
    let key = RelayKey::fresh();
    let mut w = match start(address, &key, o, &StartWith::default(), None) {
        Ok(w) => w,
        Err(e) => return RelayProbe::Failed(e.to_string()),
    };
    let any: &str = match (w.udp_to.ip().is_loopback(), w.udp_to.is_ipv6()) {
        (true, false) => "127.0.0.1:0",
        (true, true) => "[::1]:0",
        (false, false) => "0.0.0.0:0",
        (false, true) => "[::]:0",
    };
    let mut acked = false;
    if let Ok(u) = UdpSocket::bind(any)
        && u.set_nonblocking(true).is_ok()
    {
        let end = Instant::now() + Duration::from_secs_f64(o.times.probe_udp_ms / 1000.0);
        let mut next = Instant::now();
        let mut buf = vec![0u8; 2048];
        while !acked && Instant::now() < end {
            if Instant::now() >= next {
                let mut d = Vec::new();
                w.tx.datagram_into(&mut d, &w.token, UDP_PROBE, &[&[0]]);
                let _ = u.send_to(&d, w.udp_to);
                next += Duration::from_millis(250);
            }
            while let Ok((n, from)) = u.recv_from(&mut buf) {
                let ours = from.port() == w.udp_to.port()
                    && normal_address(&from.ip().to_string()).as_deref() == Some(w.peer_ip.as_str());
                if ours
                    && datagram_token(&buf[..n]) == Some(w.token)
                    && let Some(d) = w.datagrams.open(&mut buf[..n]).and_then(read_udp_body)
                    && d.kind == UDP_PROBE_ACK
                {
                    acked = true;
                }
            }
            std::thread::sleep(LOOP_SLEEP);
        }
    }
    let relay = B64.decode(&w.relay_key).map(|r| key::key_id(&r)).unwrap_or_default();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a time in ms")]
    let ms = w.ms.round().max(0.0) as u64;
    let _ = w.tcp.shutdown(Shutdown::Both);
    RelayProbe::Found { version: w.version, relay, ms, udp: acked, proto: w.proto }
}

impl Drop for RelayClient {
    /// The net thread's end waited for, SHUTDOWN_MS and a little at the most: its bye goes out before a quitting
    /// program ends (the relay keeps no seat for it). A thread already ended is not waited for.
    fn drop(&mut self) {
        let _ = self.cmds.send(Cmd::Shutdown);
        if let Some(t) = self.thread.take() {
            let until = Instant::now() + Duration::from_millis(SHUTDOWN_MS + 100);
            while !t.is_finished() && Instant::now() < until {
                std::thread::sleep(LOOP_SLEEP);
            }
        }
    }
}

/// `probe_relay` on a thread of its own: the result is polled.
pub fn begin_probe(address: String, o: ClientOptions) -> crate::pending::Pending<RelayProbe> {
    crate::pending::Pending::start(move |_| probe_relay(&address, &o))
}

#[cfg(test)]
mod tests {
    use super::{RelayError, is_referee_id, other_key, welcome_proto};

    /// A welcome with no `proto` (an older relay) signs in as before, its protocol not known; one that is not a whole
    /// u32 is not known either.
    #[test]
    fn the_welcomes_proto_is_read_leniently() {
        let w = |v: serde_json::Value| welcome_proto(v.as_object().expect("an object"));
        let base = serde_json::json!({ "t": "welcome", "id": "r1", "token": "dA==", "udp": 7777 });
        assert_eq!(w(base.clone()), None);
        let mut with = base.clone();
        for (p, want) in [
            (serde_json::json!(28), Some(28)),
            (serde_json::json!(27.0), Some(27)),
            (serde_json::json!(27.5), None),
            (serde_json::json!(-1), None),
            (serde_json::json!("28"), None),
            (serde_json::json!(null), None),
            (serde_json::json!(4_294_967_296_u64), None),
        ] {
            with["proto"] = p.clone();
            assert_eq!(w(with.clone()), want, "{p}");
        }
    }

    #[test]
    fn other_key_reads_its_id_back() {
        let id = format!("r{}", "0123456789abcdef".repeat(2));
        let why = RelayError::OtherKey { proved: id.clone() }.to_string();
        assert_eq!(other_key(&why), Some(id.as_str()));
        assert_eq!(other_key(&format!("the relay dropped this player: {why}")), Some(id.as_str()));
        assert_eq!(other_key("the relay did not prove its key"), None);
    }

    #[test]
    fn a_referee_id_is_an_owner_and_a_match() {
        let owner = format!("r{}", "0123456789abcdef".repeat(2));
        assert!(is_referee_id(&format!("{owner}.m0")));
        assert!(is_referee_id(&format!("{owner}.m17")));
        assert!(!is_referee_id(&owner));
        assert!(!is_referee_id(&format!("{owner}.m")));
        assert!(!is_referee_id(&format!("{owner}.mx")));
        assert!(!is_referee_id(&format!("r{}.m1", "0123456789ABCDEF".repeat(2))));
        assert!(is_referee_id("queue.m4"));
        assert!(is_referee_id("l0a1b.host.m12"));
        assert!(!is_referee_id("l0a1b.m12"));
    }
}
