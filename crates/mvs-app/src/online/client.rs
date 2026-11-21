//! One online session of the game: a custom game this player hosts, one it joined
//! by its code, or the queue's. Its messages are JSON `{ t, .. }` (mvs_protocol::messages) to and from the session's
//! host core; a match's packets (match_wire) go to the match's referee and come from it. A session is open once the
//! host core's first message names this game's PROTOCOL.
//!
//! Deviation: a promise and handlers are `News` the owner takes from each event and each `pump`; the clock is
//! the owner's `now` (ms).

use std::collections::{BTreeMap, VecDeque};

#[cfg(test)]
use mvs_net::service::api::SessionOn;
use mvs_net::service::api::{AskWhat, ClosedWhy, NetEvent, NetRequest};
use mvs_protocol::match_wire::PingWindow;
use mvs_protocol::messages::{ClientMessage, ServerMessage};
use mvs_protocol::names::clean_name;
use mvs_protocol::version::PROTOCOL;

use super::port::NetPort;

/// How long a new session waits for the host core's first message: a guest's service knocks on the host's for up to
/// 10 s before the link stands.
pub const HANDSHAKE_MS: f64 = 15000.0;
/// The time between two pings in the menus and in a match.
pub const PING_MENUS_MS: f64 = 2000.0;
pub const PING_MATCH_MS: f64 = 1000.0;
/// The pings without an answer kept at the most.
const PINGS_KEPT: usize = 64;

/// A question of the service the session waits on: the player's yes or no goes back with `id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetAsk {
    pub id: u32,
    pub what: AskWhat,
}

/// Why a session did not open.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenFailed {
    /// ProtocolMismatch: the host core speaks another protocol.
    Mismatch,
    /// Deviation: (online.ts: ProtocolMismatch as well) the host core said nothing within HANDSHAKE_MS; the menus
    /// tell the player it did not answer, not that it speaks another protocol.
    NoAnswer,
    /// The service closed the session first.
    Closed(ClosedWhy),
}

/// What a session tells its owner, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum News {
    /// The host core's first message named PROTOCOL: the session is open.
    Ready,
    /// The session did not open (it is over).
    Failed(OpenFailed),
    /// A message of the host core (once Ready).
    Msg(Box<ServerMessage>),
    /// A match's packets from the referee (`from` its slot).
    Packets { from: i32, packets: Vec<Vec<u8>> },
    /// The question the session waits on, or None: the one told is dropped unanswered.
    Ask(Option<NetAsk>),
    /// The open session is over (closed by either side or lost).
    Closed(ClosedWhy),
}

/// The packets and bytes of the match so far (?diag).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Traffic {
    pub packets_in: u64,
    pub packets_out: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

pub struct NetClient {
    /// Its number on the port.
    pub session: u32,
    /// Not over (closed by either side or lost).
    pub open: bool,
    /// The host core's first message came with PROTOCOL.
    pub ready: bool,
    /// The lobby's code, shown to the players and pasted to join.
    pub code: String,
    /// This player's id in the session (its `Opened`).
    pub me: String,
    /// The session is on a chosen backend (false: a direct code's join).
    pub chosen: bool,
    /// The session's links to other players that stand, by id: relayed or not (Epic's alone).
    pub links: BTreeMap<String, bool>,
    /// The host core's name (its first message).
    pub server_name: String,
    /// ms to the host core and back, measured every PING_MENUS_MS (-1 before the first).
    pub ping: f64,
    pub traffic: Traffic,
    recent: PingWindow,
    ping_n: u32,
    pings: VecDeque<(u32, f64)>,
    ping_every: f64,
    next_ping: f64,
    /// The handshake's deadline (None: held by an open question, or the session is ready).
    handshake: Option<f64>,
    asked: Option<NetAsk>,
    /// Questions are shown (false: each is answered no at once).
    asks: bool,
    /// A test delay added to every match packet this side sends (?lag).
    lag: f64,
    lagged: VecDeque<(f64, Vec<u8>)>,
}

impl NetClient {
    /// Opens a session on `port` (the service closes one still open first). `asks`: its questions are shown (they hold
    /// the HANDSHAKE_MS wait while open), else each is answered no. `lag`: ms added to each packet sent.
    pub fn open(
        port: &mut NetPort,
        target: mvs_net::service::api::SessionTarget,
        now: f64,
        asks: bool,
        lag: f64,
    ) -> NetClient {
        let session = port.open(target);
        NetClient {
            session,
            open: true,
            ready: false,
            code: String::new(),
            me: String::new(),
            chosen: true,
            links: BTreeMap::new(),
            server_name: String::new(),
            ping: -1.0,
            traffic: Traffic::default(),
            recent: PingWindow::default(),
            ping_n: 0,
            pings: VecDeque::new(),
            ping_every: PING_MENUS_MS,
            next_ping: f64::INFINITY,
            handshake: Some(now + HANDSHAKE_MS),
            asked: None,
            asks,
            lag,
            lagged: VecDeque::new(),
        }
    }

    /// An event of the port, `of` its session.
    pub fn take(&mut self, port: &mut NetPort, e: &NetEvent, of: u32, now: f64) -> Vec<News> {
        let mut out = Vec::new();
        if !self.open || of != self.session {
            return out;
        }
        match e {
            NetEvent::Opened { code, me, chosen, .. } => {
                self.code.clone_from(code);
                self.me.clone_from(me);
                self.chosen = *chosen;
            }
            NetEvent::Closed(why) => self.ended(why.clone(), &mut out),
            NetEvent::Ask { id, what } => self.ask(port, NetAsk { id: *id, what: what.clone() }, &mut out),
            NetEvent::Link { peer, relayed } => match relayed {
                None => {
                    self.links.remove(peer);
                }
                Some(r) => {
                    self.links.insert(peer.clone(), *r);
                }
            },
            NetEvent::Packets { from, packets } => {
                self.traffic.packets_in += packets.len() as u64;
                self.traffic.bytes_in += packets.iter().map(|p| p.len() as u64).sum::<u64>();
                if self.ready {
                    out.push(News::Packets { from: *from, packets: packets.clone() });
                }
            }
            NetEvent::Msg(text) => self.message(port, text, now, &mut out),
            _ => {}
        }
        out
    }

    fn message(&mut self, port: &mut NetPort, text: &str, now: f64, out: &mut Vec<News>) {
        let Ok(mut v) = serde_json::from_str::<serde_json::Value>(text) else { return };
        if !self.ready {
            // (the open waits for `server { proto, name }` alone)
            if v.get("t").and_then(|t| t.as_str()) != Some("server") {
                return;
            }
            self.handshake = None;
            if v.get("proto").and_then(serde_json::Value::as_f64) != Some(f64::from(PROTOCOL)) {
                self.close(port);
                out.clear();
                out.push(News::Failed(OpenFailed::Mismatch));
                return;
            }
            self.server_name = clean_name(v.get("name").and_then(|n| n.as_str()).unwrap_or_default());
            self.ready = true;
            self.ping_every(PING_MENUS_MS, now);
            self.send_ping(port, now);
            out.push(News::Ready);
            return;
        }
        lenient(&mut v);
        // Deviation: a message not of its shape (mvs_protocol::messages) is dropped whole; each field was read
        // leniently (the fields `lenient` mends are read as they were).
        let Ok(mut m) = serde_json::from_value::<ServerMessage>(v) else { return };
        m.clean_names();
        if let ServerMessage::Pong { n } = &m
            && let Some(n) = n.as_f64()
            && let Some(&(_, sent)) = self.pings.iter().find(|p| f64::from(p.0) == n)
        {
            self.ping = now - sent;
            self.recent.add(self.ping);
            self.pings.retain(|p| f64::from(p.0) > n);
        }
        out.push(News::Msg(Box::new(m)));
    }

    // One question at a time: a newer one answers the open one no. Not shown: no at once.
    fn ask(&mut self, port: &mut NetPort, a: NetAsk, out: &mut Vec<News>) {
        if let Some(old) = self.asked.take() {
            port.post(NetRequest::Answer { id: old.id, yes: false });
            out.push(News::Ask(None));
        }
        if !self.asks {
            port.post(NetRequest::Answer { id: a.id, yes: false });
            return;
        }
        if !self.ready {
            self.handshake = None;
        }
        self.asked = Some(a.clone());
        out.push(News::Ask(Some(a)));
    }

    /// The player's answer to the open question `id` (one already answered or dropped: nothing).
    pub fn answer(&mut self, port: &mut NetPort, id: u32, yes: bool, now: f64) {
        if !self.open || self.asked.as_ref().is_none_or(|a| a.id != id) {
            return;
        }
        self.asked = None;
        port.post(NetRequest::Answer { id, yes });
        if !self.ready {
            self.handshake = Some(now + HANDSHAKE_MS);
        }
    }

    /// The question the session waits on.
    pub fn asked(&self) -> Option<&NetAsk> {
        self.asked.as_ref()
    }

    fn ended(&mut self, why: ClosedWhy, out: &mut Vec<News>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.handshake = None;
        self.lagged.clear();
        // (a backend's own reason the menus do not word, mvs_net::fail NetFail::Other, is the log's alone)
        let other = match &why {
            ClosedWhy::MakeFailed { why } | ClosedWhy::PartyMakeFailed { why } => Some(why),
            ClosedWhy::MakeHostFailed { why: mvs_net::lan::direct::HostStartWhy::OwnRefused { why } } => Some(why),
            _ => None,
        };
        if let Some(mvs_net::fail::NetFail::Other { log }) = other {
            eprintln!("mvs: online: the backend's reason: {log}");
        }
        if self.asked.take().is_some() {
            out.push(News::Ask(None));
        }
        out.push(if self.ready { News::Closed(why) } else { News::Failed(OpenFailed::Closed(why)) });
    }

    /// The timers due by `now`: the handshake's wait, the ping, the lagged packets.
    pub fn pump(&mut self, port: &mut NetPort, now: f64) -> Vec<News> {
        let out = Vec::new();
        if !self.open {
            return out;
        }
        if self.handshake.is_some_and(|t| now >= t) {
            self.handshake = None;
            self.close(port);
            return vec![News::Failed(OpenFailed::NoAnswer)];
        }
        if self.ready && now >= self.next_ping {
            self.send_ping(port, now);
            self.next_ping = now + self.ping_every;
        }
        while self.lagged.front().is_some_and(|p| now >= p.0) {
            let (_, b) = self.lagged.pop_front().expect("a lagged packet");
            port.post(NetRequest::Send(b));
        }
        out
    }

    /// When `pump` has something to do next.
    pub fn next_due(&self) -> Option<f64> {
        if !self.open {
            return None;
        }
        [self.handshake, self.ready.then_some(self.next_ping), self.lagged.front().map(|p| p.0)]
            .into_iter()
            .flatten()
            .reduce(f64::min)
    }

    fn send_ping(&mut self, port: &mut NetPort, now: f64) {
        if self.pings.len() > PINGS_KEPT {
            self.pings.pop_front();
        }
        self.ping_n += 1;
        self.pings.push_back((self.ping_n, now));
        self.send(port, &ClientMessage::Ping { n: self.ping_n });
    }

    /// The smallest of the last measured pings (below 0 before the first): the match's start gate's.
    pub fn ping_min(&self) -> f64 {
        self.recent.min()
    }

    /// The time between two pings (PING_MENUS_MS, PING_MATCH_MS).
    pub fn ping_every(&mut self, ms: f64, now: f64) {
        if !self.open {
            return;
        }
        self.ping_every = ms;
        self.next_ping = now + ms;
    }

    /// (?diag) ms the oldest ping without an answer has been out (0: none is).
    pub fn unanswered(&self, now: f64) -> f64 {
        self.pings.front().map_or(0.0, |p| now - p.1)
    }

    pub fn send(&self, port: &mut NetPort, m: &ClientMessage) {
        if self.open {
            port.post(NetRequest::Msg(m.text()));
        }
    }

    /// A match's packets: from match.go on (its `peers`, this game's slot) to match_end.
    pub fn start_match(&self, port: &mut NetPort, peers: Vec<String>, slot: u32, referee: String) {
        if self.open {
            port.post(NetRequest::Match { peers, slot, referee });
        }
    }

    pub fn send_packet(&mut self, port: &mut NetPort, bytes: Vec<u8>, now: f64) {
        if !self.open {
            return;
        }
        self.traffic.packets_out += 1;
        self.traffic.bytes_out += bytes.len() as u64;
        if self.lag > 0.0 {
            self.lagged.push_back((now + self.lag, bytes));
        } else {
            port.post(NetRequest::Send(bytes));
        }
    }

    pub fn match_end(&self, port: &mut NetPort) {
        if self.open {
            port.post(NetRequest::MatchEnd);
        }
    }

    /// Invites a player (their id) to this session; `name` this player's.
    pub fn invite(&self, port: &mut NetPort, to: String, name: String) {
        if self.open {
            port.post(NetRequest::Invite { to, name });
        }
    }

    /// Leaves the session (the host's leaving ends it for every member); nothing more is told of it.
    pub fn close(&mut self, port: &mut NetPort) {
        if !self.open {
            return;
        }
        port.post(NetRequest::Close);
        self.open = false;
        self.handshake = None;
        self.asked = None;
        self.lagged.clear();
    }
}

/// A message's fields read leniently, made the shape the typed read takes: pm.end's `why` ('left', else
/// 'error') and its `name` (a string, else '').
fn lenient(v: &mut serde_json::Value) {
    let Some(o) = v.as_object_mut() else { return };
    if o.get("t").and_then(serde_json::Value::as_str) == Some("pm.end") {
        let why = if o.get("why").and_then(serde_json::Value::as_str) == Some("left") { "left" } else { "error" };
        o.insert("why".into(), why.into());
        let name = o.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
        o.insert("name".into(), name.into());
    }
}

#[cfg(test)]
mod tests {
    use super::super::port::fake;
    use super::*;
    use mvs_net::service::api::SessionTarget;
    use mvs_protocol::messages::PmEndWhy;

    fn server(proto: u32) -> NetEvent {
        NetEvent::Msg(ServerMessage::Server { proto, name: "core".into() }.text())
    }

    #[test]
    fn opens_on_the_protocol_and_pings() {
        let (mut p, w) = fake::port();
        let mut c = NetClient::open(&mut p, SessionTarget::Host, 0.0, true, 0.0);
        let e = NetEvent::Opened { code: "r-1".into(), me: "me".into(), chosen: true, on: SessionOn::Relay };
        assert!(c.take(&mut p, &e, 1, 0.0).is_empty());
        assert_eq!(c.take(&mut p, &server(PROTOCOL), 1, 10.0), [News::Ready]);
        assert!(c.code == "r-1" && c.server_name == "core");
        let posted = &w.borrow().posted;
        assert!(matches!(&posted[1], NetRequest::Msg(t) if t == r#"{"t":"ping","n":1}"#));
        let pong = NetEvent::Msg(ServerMessage::Pong { n: serde_json::json!(1) }.text());
        assert!(matches!(c.take(&mut p, &pong, 1, 60.0)[0], News::Msg(_)));
        assert!(c.ping == 50.0 && c.ping_min() == 50.0 && c.unanswered(60.0) == 0.0);
        assert_eq!(c.next_due(), Some(10.0 + PING_MENUS_MS));
        // an event of another session is not this one's
        assert!(c.take(&mut p, &NetEvent::Closed(ClosedWhy::Left), 2, 70.0).is_empty());
        assert_eq!(
            c.take(&mut p, &NetEvent::Closed(ClosedWhy::HostLeft), 1, 70.0),
            [News::Closed(ClosedWhy::HostLeft)]
        );
        assert!(!c.open);
    }

    #[test]
    fn another_protocol_or_none_is_refused() {
        let (mut p, w) = fake::port();
        let mut c = NetClient::open(&mut p, SessionTarget::Host, 0.0, true, 0.0);
        assert_eq!(c.take(&mut p, &server(PROTOCOL + 1), 1, 1.0), [News::Failed(OpenFailed::Mismatch)]);
        assert!(!c.open && matches!(w.borrow().posted.last(), Some(NetRequest::Close)));
        let mut c = NetClient::open(&mut p, SessionTarget::Host, 0.0, true, 0.0);
        assert!(c.pump(&mut p, HANDSHAKE_MS - 1.0).is_empty());
        assert_eq!(c.pump(&mut p, HANDSHAKE_MS), [News::Failed(OpenFailed::NoAnswer)]);
        let mut c = NetClient::open(&mut p, SessionTarget::Host, 0.0, true, 0.0);
        let closed = NetEvent::Closed(ClosedWhy::Limited);
        assert_eq!(c.take(&mut p, &closed, 3, 1.0), [News::Failed(OpenFailed::Closed(ClosedWhy::Limited))]);
    }

    #[test]
    fn a_question_holds_the_handshake() {
        let (mut p, w) = fake::port();
        let mut c = NetClient::open(&mut p, SessionTarget::Join("r".into()), 0.0, true, 0.0);
        let ask = |id| NetEvent::Ask { id, what: AskWhat::NewRelay { address: "a:1".into() } };
        assert!(matches!(&c.take(&mut p, &ask(1), 1, 0.0)[..], [News::Ask(Some(_))]));
        assert!(c.pump(&mut p, HANDSHAKE_MS * 2.0).is_empty());
        // a newer question answers the old one no
        assert!(matches!(&c.take(&mut p, &ask(2), 1, 0.0)[..], [News::Ask(None), News::Ask(Some(_))]));
        assert!(matches!(w.borrow().posted.last(), Some(NetRequest::Answer { id: 1, yes: false })));
        c.answer(&mut p, 2, true, 100.0);
        assert!(matches!(w.borrow().posted.last(), Some(NetRequest::Answer { id: 2, yes: true })));
        assert_eq!(c.next_due(), Some(100.0 + HANDSHAKE_MS));
        // not shown: no at once
        let mut c = NetClient::open(&mut p, SessionTarget::Join("r".into()), 0.0, false, 0.0);
        assert!(c.take(&mut p, &ask(3), 2, 0.0).is_empty());
        assert!(matches!(w.borrow().posted.last(), Some(NetRequest::Answer { id: 3, yes: false })));
    }

    #[test]
    fn packets_are_lagged_and_counted() {
        let (mut p, w) = fake::port();
        let mut c = NetClient::open(&mut p, SessionTarget::Host, 0.0, true, 30.0);
        c.take(&mut p, &server(PROTOCOL), 1, 0.0);
        c.send_packet(&mut p, vec![1, 2, 3], 5.0);
        assert!(!matches!(w.borrow().posted.last(), Some(NetRequest::Send(_))));
        c.pump(&mut p, 35.0);
        assert!(matches!(w.borrow().posted.last(), Some(NetRequest::Send(b)) if b == &[1, 2, 3]));
        assert!(c.traffic.packets_out == 1 && c.traffic.bytes_out == 3);
    }

    #[test]
    fn reads_an_unknown_pm_end_why_as_an_error() {
        let (mut p, _w) = fake::port();
        let mut c = NetClient::open(&mut p, SessionTarget::Host, 0.0, true, 0.0);
        c.take(&mut p, &server(PROTOCOL), 1, 0.0);
        let end = NetEvent::Msg(r#"{"t":"pm.end","why":"timeout","name":3}"#.into());
        let news = c.take(&mut p, &end, 1, 1.0);
        assert!(matches!(
            news.as_slice(),
            [News::Msg(m)] if matches!(&**m, ServerMessage::PmEnd { why: PmEndWhy::Error, name } if name.is_empty())
        ));
        let left = NetEvent::Msg(r#"{"t":"pm.end","why":"left","name":"Ann"}"#.into());
        assert!(matches!(
            c.take(&mut p, &left, 1, 2.0).as_slice(),
            [News::Msg(m)] if matches!(&**m, ServerMessage::PmEnd { why: PmEndWhy::Left, name } if name == "Ann")
        ));
    }
}
