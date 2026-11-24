//! Public LAN lobbies on the local network: a lobby
//! hosted on a direct host is announced by UDP broadcast while it is listed, and the service of an app with LAN on
//! hears the announcements while a page browses, and keeps the lobbies for the custom game browser.
//!   the port       [`DISCOVERY_PORT`]: fixed, not the LAN setting's, so that the hosted lobby's own listener keeps the
//!                  LAN port, and no other program's port is held
//!   the packet     [`ANNOUNCE_MAGIC`], [`ANNOUNCE_V`], the lobby's members and most players, its direct code's length
//!                  and the code, then its listing's text to the end; at most [`ANNOUNCE_MAX_BYTES`] (one datagram on
//!                  any network, no fragments)
//!   the announcer  a socket of its own (any port), the packet every [`ANNOUNCE_EVERY_MS`] while the lobby is listed
//!                  and once at once on a change, to the discovery port at 255.255.255.255 and at the broadcast
//!                  address of each interface its code names (127.255.255.255 alone with `loopback`, the tests')
//!   the listener   the discovery port, reuse-address (every app on one PC binds it, and each hears a broadcast); it
//!                  never sends, so it answers nobody. A packet is dropped past its sender's rate, from an address
//!                  outside a local network, that does not parse, whose code does not decode or does not name its
//!                  sender's address, or of this install's own key; a lobby is kept until [`HEARD_FOR_MS`] after its
//!                  last announcement, [`HEARD_MAX`] at the most and [`SENDER_LOBBIES`] from one address, its code cut
//!                  down to the address it was heard from (a join of it tries that one alone).
//! Both own their timers: `pump(now)` and `next_due()` with the service's clock (ms); sockets are non-blocking.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};

use indexmap::{IndexMap, IndexSet};
use mvs_protocol::lobby_listing::{LISTING_MAX_BYTES, listing_of};
use socket2::{Domain, Protocol, Socket, Type};

use super::direct_code::{Interfaces, Local, decode_direct_code, direct_text, encode_direct_code, local_address};

/// The UDP port of the announcements (the settings' LAN_DISCOVERY_PORT, which says why that number).
pub const DISCOVERY_PORT: u16 = 39417;
/// The packet's first bytes ("MVSL") and its shape's version: a packet of another one is dropped.
pub const ANNOUNCE_MAGIC: [u8; 4] = [0x4d, 0x56, 0x53, 0x4c];
pub const ANNOUNCE_V: u8 = 1;
/// The most bytes of a packet: under the 1472 of a 1500-byte Ethernet frame, so it never fragments. The port's own.
pub const ANNOUNCE_MAX_BYTES: usize = 1200;
/// A listed lobby is announced this often (ms). Unverified: the port's own number.
pub const ANNOUNCE_EVERY_MS: f64 = 2000.0;
/// A heard lobby is kept this long after its last announcement: three missed.
pub const HEARD_FOR_MS: f64 = 3.0 * ANNOUNCE_EVERY_MS;
/// The most lobbies kept, and the most from one address; a new one past either is dropped until one expires.
/// Unverified: the port's own numbers.
pub const HEARD_MAX: usize = 64;
pub const SENDER_LOBBIES: usize = 4;
/// The most packets taken from one address per [`ANNOUNCE_EVERY_MS`] (a host's lobbies, each to several broadcast
/// addresses), and the most senders counted at once. Unverified: the port's own numbers.
pub const SENDER_PACKETS: u32 = 24;
pub const SENDERS_MAX: usize = 256;

const HEAD: usize = ANNOUNCE_MAGIC.len() + 4;

/// What a hosted lobby announces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    /// The lobby's direct code.
    pub code: String,
    /// Its listing's text (`listing_text`).
    pub info: String,
    pub members: u32,
    pub max: u32,
}

/// The packet of an announcement; None when it does not fit [`ANNOUNCE_MAX_BYTES`] or a field is out of range (a
/// programming error: a listing is [`LISTING_MAX_BYTES`] at the most, already cut to fit, and a direct code about 50
/// characters, so the host's own always fit).
pub fn encode_announcement(a: &Announcement) -> Option<Vec<u8>> {
    let code = direct_text(&a.code);
    let code = code.as_bytes();
    if code.is_empty() || code.len() > 255 {
        return None;
    }
    if a.max < 1 || a.max > 255 || a.members > a.max {
        return None;
    }
    let text = a.info.as_bytes();
    let n = HEAD + code.len() + text.len();
    if text.len() > LISTING_MAX_BYTES || n > ANNOUNCE_MAX_BYTES {
        return None;
    }
    let mut b = Vec::with_capacity(n);
    b.extend_from_slice(&ANNOUNCE_MAGIC);
    b.push(ANNOUNCE_V);
    b.push(u8::try_from(a.members).ok()?);
    b.push(u8::try_from(a.max).ok()?);
    b.push(u8::try_from(code.len()).ok()?);
    b.extend_from_slice(code);
    b.extend_from_slice(text);
    Some(b)
}

/// UTF-8 as a strict TextDecoder reads it: a leading byte order mark dropped, anything invalid refused.
fn strict_utf8(b: &[u8]) -> Option<String> {
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    std::str::from_utf8(b).ok().map(str::to_owned)
}

/// A packet read back; None when it is not one of this shape and version.
pub fn decode_announcement(b: &[u8]) -> Option<Announcement> {
    if b.len() < HEAD + 1 || b.len() > ANNOUNCE_MAX_BYTES || b[..4] != ANNOUNCE_MAGIC || b[4] != ANNOUNCE_V {
        return None;
    }
    let (members, max, n) = (u32::from(b[5]), u32::from(b[6]), usize::from(b[7]));
    let at = HEAD;
    if max == 0 || members > max || n == 0 || at + n > b.len() {
        return None;
    }
    Some(Announcement { members, max, code: strict_utf8(&b[at..at + n])?, info: strict_utf8(&b[at + n..])? })
}

/// Where an announcement goes: 255.255.255.255 and the broadcast address of each IPv4 interface whose address is one of
/// `addresses` (the code's: a copy out of another interface would come from an address the code does not name, and be
/// dropped); 127.255.255.255 alone with `loopback`.
pub fn broadcast_targets(addresses: &[String], nets: &Interfaces, loopback: bool) -> Vec<String> {
    if loopback {
        return vec!["127.255.255.255".to_owned()];
    }
    let mut out: IndexSet<String> = IndexSet::new();
    out.insert("255.255.255.255".to_owned());
    let quad = |s: &str| -> Option<[u32; 4]> {
        let v: Vec<u32> = s.split('.').map(js_octet).collect::<Option<_>>()?;
        v.try_into().ok()
    };
    for (_, list) in nets {
        for x in list {
            if !x.v4 || x.internal || !addresses.contains(&x.address) {
                continue;
            }
            let (Some(a), Some(m)) = (quad(&x.address), quad(&x.netmask)) else { continue };
            out.insert((0..4).map(|i| (a[i] | (!m[i] & 255)).to_string()).collect::<Vec<_>>().join("."));
        }
    }
    out.into_iter().collect()
}

/// `Number(s)` of one dotted part when it is an integer of 0 to 255: the empty text is 0, and 0x, 0o and 0b texts read
/// (an exponent or a sign other than + is not read: no netmask has one).
fn js_octet(s: &str) -> Option<u32> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0);
    }
    let (radix, digits) = match t.get(..2) {
        Some("0x" | "0X") => (16, &t[2..]),
        Some("0o" | "0O") => (8, &t[2..]),
        Some("0b" | "0B") => (2, &t[2..]),
        _ => (10, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    u32::from_str_radix(digits, radix).ok().filter(|v| *v <= 255)
}

/// The out trait for a log line (a lan problem the player may want in the log).
pub type LogFn = Box<dyn FnMut(&str) + Send>;

pub struct AnnouncerOptions {
    /// Where the listeners are: [`DISCOVERY_PORT`] (tests: their own).
    pub port: Option<u16>,
    /// The hosted lobby's code's addresses.
    pub addresses: Vec<String>,
    /// Tests: 127.255.255.255 alone, from a socket on 127.0.0.1.
    pub loopback: bool,
    pub log: Option<LogFn>,
    /// This PC's interfaces (tests); the system's by default.
    pub interfaces: Option<fn() -> Interfaces>,
}

/// One hosted lobby's announcements: `set` the lobby as it is now (None: not listed, so not announced); a change is
/// sent at once, then every [`ANNOUNCE_EVERY_MS`]. Its socket opens with the first announcement and closes at `close`.
pub struct LanAnnouncer {
    o: AnnouncerOptions,
    now: Option<Vec<u8>>,
    sock: Option<UdpSocket>,
    /// When the next send is due (ms).
    due: Option<f64>,
    closed: bool,
    failed: bool,
    /// The targets whose last send failed (logged once, until one to it works again).
    failing: IndexSet<String>,
}

impl LanAnnouncer {
    pub fn new(o: AnnouncerOptions) -> LanAnnouncer {
        LanAnnouncer { o, now: None, sock: None, due: None, closed: false, failed: false, failing: IndexSet::new() }
    }

    fn log(&mut self, line: &str) {
        if let Some(l) = &mut self.o.log {
            l(line);
        }
    }

    pub fn set(&mut self, a: Option<&Announcement>, now: f64) {
        if self.closed {
            return;
        }
        let b = a.and_then(encode_announcement);
        if let (Some(a), None) = (a, &b) {
            let line = format!("lan: the announcement of {} does not fit a packet", a.code);
            self.log(&line);
        }
        if (b.is_some() && b == self.now) || (b.is_none() && self.now.is_none()) {
            return;
        }
        self.now = b;
        self.due = None;
        if self.now.is_none() {
            return;
        }
        self.send();
        self.due = Some(now + ANNOUNCE_EVERY_MS);
    }

    /// Sends when the next announcement is due.
    pub fn pump(&mut self, now: f64) {
        if let Some(d) = self.due
            && now >= d
        {
            self.send();
            self.due = Some(d.max(now - ANNOUNCE_EVERY_MS) + ANNOUNCE_EVERY_MS);
        }
    }

    pub fn next_due(&self) -> Option<f64> {
        self.due
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.now = None;
        self.due = None;
        self.sock = None;
    }

    fn open(&mut self) -> Result<UdpSocket, String> {
        // A LAN that is not loopback binds every address (any port); the tests use 127.0.0.1.
        let ip = if self.o.loopback { Ipv4Addr::LOCALHOST } else { Ipv4Addr::UNSPECIFIED };
        let s = UdpSocket::bind(SocketAddrV4::new(ip, 0)).map_err(|e| format!("lan: announcing: {e}"))?;
        s.set_broadcast(true).map_err(|e| format!("lan: no broadcast: {e}"))?;
        s.set_nonblocking(true).map_err(|e| format!("lan: announcing: {e}"))?;
        Ok(s)
    }

    fn send(&mut self) {
        if self.closed || self.failed {
            return;
        }
        let Some(b) = self.now.clone() else { return };
        if self.sock.is_none() {
            match self.open() {
                Ok(s) => self.sock = Some(s),
                Err(e) => {
                    // (a socket that cannot bind: no more tries for this lobby)
                    self.failed = true;
                    return self.log(&e);
                }
            }
        }
        let nets = self.o.interfaces.map_or_else(system_interfaces, |f| f());
        let port = self.o.port.unwrap_or(DISCOVERY_PORT);
        for to in broadcast_targets(&self.o.addresses, &nets, self.o.loopback) {
            let Ok(ip) = to.parse::<Ipv4Addr>() else { continue };
            let sent = self.sock.as_ref().map(|s| s.send_to(&b, SocketAddr::from((ip, port))));
            match sent {
                Some(Ok(_)) => {
                    self.failing.shift_remove(&to);
                }
                // (a failed send logged once per target, until one to it works again)
                Some(Err(e)) if self.failing.insert(to.clone()) => self.log(&format!("lan: announcing to {to}: {e}")),
                Some(Err(_)) => {}
                None => {}
            }
        }
    }
}

/// This PC's interfaces as the system lists them (os.networkInterfaces()).
pub fn system_interfaces() -> Interfaces {
    let mut out: Interfaces = Vec::new();
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        // (an adapter that is down is not listed, as Node leaves it out)
        if !i.is_oper_up() {
            continue;
        }
        let (address, netmask, v4) = match &i.addr {
            if_addrs::IfAddr::V4(a) => (a.ip.to_string(), a.netmask.to_string(), true),
            if_addrs::IfAddr::V6(a) => (a.ip.to_string(), a.netmask.to_string(), false),
        };
        let nic = super::direct_code::Nic { address, netmask, v4, internal: i.is_loopback() };
        match out.iter_mut().find(|(n, _)| *n == i.name) {
            Some((_, l)) => l.push(nic),
            None => out.push((i.name.clone(), vec![nic])),
        }
    }
    out
}

/// A lobby heard on the network: the browser's row of it.
#[derive(Clone, Debug, PartialEq)]
pub struct HeardLobby {
    pub code: String,
    pub info: String,
    pub members: u32,
    pub max: u32,
    /// Its sender's address, and when it was first and last heard (the clock's ms).
    pub from: String,
    pub first: f64,
    pub last: f64,
}

/// Why a packet was not kept (the log's and the checks' reading of it); a lobby of this install's own, or one heard
/// from another of its host's addresses first, is not kept and not a drop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// A sender past its rate.
    Rate,
    NotLocal,
    NotAnnouncement,
    /// Its code does not read.
    Code(super::direct_code::DirectWhy),
    NotSender,
    Listing,
    TooMany,
    TooManyFromAddress,
}

impl Dropped {
    fn text(&self) -> String {
        match self {
            Dropped::Rate => "rate".to_owned(),
            Dropped::NotLocal => "not of a local network".to_owned(),
            Dropped::NotAnnouncement => "not an announcement of this version".to_owned(),
            Dropped::Code(why) => format!("its code does not read: {}", why.log_text()),
            Dropped::NotSender => "its code does not name its sender".to_owned(),
            Dropped::Listing => "its listing does not read".to_owned(),
            Dropped::TooMany => "too many lobbies heard".to_owned(),
            Dropped::TooManyFromAddress => "too many lobbies from that address".to_owned(),
        }
    }
}

pub struct ListenerOptions {
    /// [`DISCOVERY_PORT`] (tests: their own).
    pub port: Option<u16>,
    /// This install's key's fingerprint: its own lobbies are not kept.
    pub own: String,
    /// Tests: on 127.0.0.1 alone, and codes and senders may be 127.x.
    pub loopback: bool,
    pub log: Option<LogFn>,
}

struct Window {
    at: f64,
    n: u32,
    logged: bool,
}

/// The lobbies heard on the discovery port while it is open, by their host's key and lobby.
pub struct LanListener {
    o: ListenerOptions,
    /// This PC's networks at the listener's start: a sender's address and its code's are taken on one of them.
    local: Local,
    heard_by: IndexMap<String, HeardLobby>,
    /// Each sender's packets in its current window.
    senders: IndexMap<String, Window>,
    sock: Option<UdpSocket>,
    /// Why it is not listening ('' while it is, or before `start`).
    pub why: String,
}

impl LanListener {
    pub fn new(o: ListenerOptions) -> LanListener {
        let local = Local::of(&system_interfaces(), o.loopback);
        LanListener { o, local, heard_by: IndexMap::new(), senders: IndexMap::new(), sock: None, why: String::new() }
    }

    pub fn listening(&self) -> bool {
        self.sock.is_some()
    }

    /// Binds the discovery port (reuse-address); Err when it cannot (the reason is `why` too).
    pub fn start(&mut self) -> Result<(), String> {
        let port = self.o.port.unwrap_or(DISCOVERY_PORT);
        // A LAN that is not loopback binds every address; the tests use 127.0.0.1. On Linux a socket on 127.0.0.1 is
        // not given a broadcast to 127.255.255.255, so loopback binds every address there too, and `pump` takes what
        // comes from 127/8 alone.
        let ip = if self.o.loopback && cfg!(windows) { Ipv4Addr::LOCALHOST } else { Ipv4Addr::UNSPECIFIED };
        let open = || -> std::io::Result<UdpSocket> {
            let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
            s.set_reuse_address(true)?;
            s.bind(&SocketAddr::from((ip, port)).into())?;
            s.set_nonblocking(true)?;
            Ok(s.into())
        };
        match open() {
            Ok(s) => {
                self.sock = Some(s);
                self.why.clear();
                Ok(())
            }
            Err(e) => {
                self.why = format!("the port {port} LAN games are found on cannot be listened on: {e}");
                Err(self.why.clone())
            }
        }
    }

    pub fn close(&mut self) {
        self.sock = None;
        self.heard_by.clear();
        self.senders.clear();
    }

    fn log(&mut self, line: &str) {
        if let Some(l) = &mut self.o.log {
            l(line);
        }
    }

    /// Takes the datagrams waiting on the socket.
    pub fn pump(&mut self, now: f64) {
        // (a datagram cut short by a small buffer would be an error on Windows: one buffer for any size, a long one
        // is then "not an announcement" under its sender's rate)
        let mut buf = vec![0u8; 65536];
        loop {
            let Some(s) = &self.sock else { return };
            let (n, from) = match s.recv_from(&mut buf) {
                Ok(x) => x,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                // (Windows tells a datagram's ICMP port unreachable at the next receive)
                Err(e) if matches!(e.kind(), ErrorKind::ConnectionReset | ErrorKind::Interrupted) => continue,
                Err(e) => {
                    let line = format!("lan: listening: {e}");
                    self.log(&line);
                    return;
                }
            };
            // (Linux, loopback: the socket is on every address, and what does not come from this PC is not heard, as
            // on Windows, where it is on 127.0.0.1)
            #[cfg(not(windows))]
            if self.o.loopback && !from.ip().is_loopback() {
                continue;
            }
            let addr = from.ip().to_string();
            let why = self.take(&buf[..n], &addr, now);
            // (one line per sender per window at the most; none past its rate)
            if let Some(w) = why
                && w != Dropped::Rate
                && let Some(win) = self.senders.get_mut(&addr)
                && !win.logged
            {
                win.logged = true;
                let line = format!("lan: dropped a packet from {addr}: {}", w.text());
                self.log(&line);
            }
        }
    }

    /// One packet from `from` (its sender's address): kept (None), or why not. Every sender is counted against its
    /// rate, one outside a local network too. A lobby of this install's own, or one heard from another of its host's
    /// addresses first, is not kept and is None too.
    pub fn take(&mut self, bytes: &[u8], from: &str, now: f64) -> Option<Dropped> {
        let loopback = self.o.loopback;
        let addr = from.get(..7).filter(|p| p.eq_ignore_ascii_case("::ffff:")).map_or(from, |_| &from[7..]).to_owned();
        let fresh = self.senders.get(&addr).is_none_or(|s| now - s.at >= ANNOUNCE_EVERY_MS);
        if fresh {
            if !self.senders.contains_key(&addr) && self.senders.len() >= SENDERS_MAX {
                self.senders.retain(|_, v| now - v.at < ANNOUNCE_EVERY_MS);
                if self.senders.len() >= SENDERS_MAX {
                    return Some(Dropped::Rate);
                }
            }
            self.senders.insert(addr.clone(), Window { at: now, n: 0, logged: false });
        }
        let s = self.senders.get_mut(&addr).expect("counted");
        s.n += 1;
        if s.n > SENDER_PACKETS {
            return Some(Dropped::Rate);
        }
        let local = &self.local;
        if !local_address(&addr, local) {
            return Some(Dropped::NotLocal);
        }
        let Some(a) = decode_announcement(bytes) else { return Some(Dropped::NotAnnouncement) };
        let d = match decode_direct_code(&a.code, local) {
            Ok(d) => d,
            Err(w) => return Some(Dropped::Code(w)),
        };
        if !d.addresses.contains(&addr) {
            return Some(Dropped::NotSender);
        }
        if d.fingerprint == self.o.own {
            return None;
        }
        if listing_of(&serde_json::Value::String(a.info.clone())).is_none() {
            return Some(Dropped::Listing);
        }
        self.expire(now);
        let key = format!("{}:{}", d.fingerprint, d.lobby);
        let was = self.heard_by.get(&key).cloned();
        // (a host on two networks: its lobby is kept as heard from the first address, while that one goes on)
        if was.as_ref().is_some_and(|w| w.from != addr) {
            return None;
        }
        if was.is_none() {
            if self.heard_by.len() >= HEARD_MAX {
                return Some(Dropped::TooMany);
            }
            if self.heard_by.values().filter(|h| h.from == addr).count() >= SENDER_LOBBIES {
                return Some(Dropped::TooManyFromAddress);
            }
        }
        // (the code cut down to the sender's address: a code naming another address too is not tried there)
        let code = if d.addresses.len() == 1 {
            direct_text(&a.code)
        } else {
            let cut = super::direct_code::DirectCode { addresses: vec![addr.clone()], ..d.clone() };
            encode_direct_code(&cut, loopback).unwrap_or_else(|_| direct_text(&a.code))
        };
        self.heard_by.insert(
            key,
            HeardLobby {
                code,
                info: a.info,
                members: a.members,
                max: a.max,
                from: addr,
                first: was.map_or(now, |w| w.first),
                last: now,
            },
        );
        None
    }

    /// The lobbies heard within [`HEARD_FOR_MS`], first heard first.
    pub fn heard(&mut self, now: f64) -> Vec<HeardLobby> {
        self.expire(now);
        let mut v: Vec<HeardLobby> = self.heard_by.values().cloned().collect();
        v.sort_by(|p, q| p.first.total_cmp(&q.first));
        v
    }

    fn expire(&mut self, now: f64) {
        self.heard_by.retain(|_, v| now - v.last <= HEARD_FOR_MS);
    }
}
