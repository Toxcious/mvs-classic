//! The match path of a session in a core the relay hosts (its queue's, a custom lobby's leader or guest: online plan v4
//! section 5, `mvs_protocol::path`): told the other humans' Epic ids by the core (path.peers), it links to each over
//! Epic, pings them ([`mvs_protocol::match_wire`] path pings) and reports its round trips (path.report). When the
//! core chooses this player's app as the match's referee (ref.go) it runs the match's `Referee` here
//! ([`crate::service::refereed`]), the own page's packets in process and the others' over Epic, and tells the core
//! who went silent (ref.news); as another human of a match a player's app referees it tells the core when no ruled
//! packet has come for `RESUME_MS` (ref.lost). A guest of a lobby on Epic or a LAN direct host has no path.
//!
//! The measure: the first ping goes one PATH_PING_MS after path.peers, so that the step which took it has ended and
//! the service's relay check and accept sets count the path (`open_as`, `epic_peers`) before a link opens; a queue's
//! path (never direct) forces Epic's relays at once besides. Then PATH_PINGS pings PATH_PING_MS apart to each peer,
//! the report one PATH_PING_MS after the last: each peer's smallest round trip (None: no pong came) and whether its
//! link is relayed.
//!
//! While the page's match goes through a player's app (its route's referee one of the path's Epic ids, this player's
//! own among them) the relay client is kept "in a match" by a beat of empty channel-1 sends every MATCH_BEAT_MS
//! (`link::relay`'s thread takes them as its match's and sends nothing), so its stall ping and the dev cut work as
//! in a match the relay referees.
//!
//! Deviation: the port's own (the build's matches go through its own servers).

use indexmap::IndexMap;
use serde_json::Value;

use mvs_protocol::match_wire::{MSG_PATH_PING, MSG_PATH_PONG, RESUME_MS, packet_fits, path_ping, read_path_ping};
use mvs_protocol::messages::{ClientMessage, PathLink, PathPeer, RefNewsWire, ServerMessage};
use mvs_protocol::path::{PATH_PING_MS, PATH_PINGS, PATH_WAIT_MS};
use mvs_protocol::referee::{Referee, SlotNews};

use crate::backend::{BackendKind, Channel, RelayMode};
use crate::service::hosting::Guesting;
use crate::service::page::{TimerId, Timers};
use crate::service::refereed::{Outs, RefOut, Refereed};
use crate::service::session::{SessionBase, Via};
use crate::service::{NetService, REFEREE_PUMP_MS, Shared, lock};

/// The beat that keeps the relay client in a match while the match goes on Epic (ms): under the client's
/// MATCH_ON_MS (1000).
/// Unverified: the port's own number.
pub const MATCH_BEAT_MS: f64 = 250.0;

#[derive(Clone, Debug, PartialEq)]
enum PathDue {
    /// The pings of this number to every peer.
    Ping(u8),
    /// The measure's report.
    Report,
    /// The wait for pm.path after the report: none by then, the links go.
    Unchosen,
    /// The referee here pumped.
    Pump,
    /// The relay client's beat, and the ref.lost wait looked at.
    Beat,
}

/// Another human of the path.
struct Peer {
    slot: u32,
    epic: String,
    /// The smallest round trip a pong gave (ms).
    rtt: Option<f64>,
    /// Epic's word on the link (None: none, or it went down).
    relayed: Option<bool>,
}

/// As another human of a match a player's app referees: that referee's id, the last ruled packet's time, ref.lost
/// sent.
struct Watch {
    referee: String,
    last: f64,
    told: bool,
}

/// The path's state of one relay-hosted session.
pub struct PathSide {
    /// The Epic backend the path's links and the referee's packets go on.
    epic: Shared,
    /// This player's Epic product user id.
    me_epic: String,
    /// The core's id on the relay (the beat's address: a peer the relay client has already).
    core: String,
    peers: Vec<Peer>,
    /// The links may be direct (a custom lobby whose humans all said so).
    direct: bool,
    /// From path.peers to the report.
    measuring: bool,
    /// The links are kept for a match a player's app referees (pm.path named a player).
    keep: bool,
    /// A forced path whose Epic relays cannot be forced: no pings, no pongs, no links offered, no referee here.
    no_force: bool,
    /// The match this app referees (the core's number).
    refereed: Option<(u32, Refereed)>,
    watch: Option<Watch>,
    timers: Timers<PathDue>,
    beat: Option<TimerId>,
    said: Vec<String>,
}

impl PathSide {
    pub fn new(epic: Shared, me_epic: String) -> PathSide {
        PathSide {
            epic,
            me_epic,
            core: String::new(),
            peers: Vec::new(),
            direct: false,
            measuring: false,
            keep: false,
            no_force: false,
            refereed: None,
            watch: None,
            timers: Timers::default(),
            beat: None,
            said: Vec::new(),
        }
    }

    /// The core's id on the relay, which the beat goes to.
    pub fn set_core(&mut self, core: &str) {
        self.core = core.to_string();
    }

    /// The core's path.peers: the humans to link to and ping (`direct`: the links may be direct).
    pub fn peers_told(&mut self, svc: &mut NetService, base: &SessionBase, peers: &[PathPeer], direct: bool, now: f64) {
        // (a set's next game measures again: what the last one had is let go first)
        self.timers.cancel_where(|d| matches!(d, PathDue::Ping(_) | PathDue::Report | PathDue::Unchosen));
        let old: Vec<String> = self.peers.drain(..).map(|p| p.epic).collect();
        self.peers = peers
            .iter()
            .filter(|p| p.epic != self.me_epic && !p.epic.is_empty())
            .map(|p| Peer { slot: p.slot, epic: p.epic.clone(), rtt: None, relayed: None })
            .collect();
        let gone: Vec<String> = old.into_iter().filter(|o| !self.peers.iter().any(|p| p.epic == *o)).collect();
        self.close_links(svc, &gone);
        self.direct = direct;
        svc.log(&format!("path: measuring {} peers{}", self.peers.len(), if direct { ", direct" } else { "" }));
        self.measuring = true;
        self.keep = false;
        // (the service's relay control told before the first ping: a stranger's link is relayed from its first
        // packet; a forced path that cannot have Force pings no one, its report all None, and the relay referees)
        let ids: Vec<String> = self.peers.iter().map(|p| p.epic.clone()).collect();
        svc.path_wanted(base.page, direct, &ids, now);
        let no_force = !direct && svc.epic_relay_mode() != Some(RelayMode::Force);
        if no_force {
            svc.log("path: Epic's relays cannot be forced: no pings");
        }
        self.no_force = no_force;
        for n in 0..PATH_PINGS {
            if no_force {
                break;
            }
            let k = u8::try_from(n).expect("PATH_PINGS fits a byte");
            self.timers.after(now, f64::from(n + 1) * PATH_PING_MS, PathDue::Ping(k));
        }
        self.timers.after(now, f64::from(PATH_PINGS + 1) * PATH_PING_MS, PathDue::Report);
        self.beat_on(now);
    }

    /// The beat runs from the first path.peers to the path's end.
    fn beat_on(&mut self, now: f64) {
        if self.beat.is_none_or(|t| !self.timers.is_set(t)) {
            self.beat = Some(self.timers.every(now, MATCH_BEAT_MS, PathDue::Beat));
        }
    }

    /// The core's pm.path: `referee` the chosen slot (-1 the relay). The measure is over either way; with the relay
    /// the links go.
    pub fn chose(&mut self, svc: &mut NetService, referee: i32, now: f64) {
        let _ = now;
        self.timers.cancel_where(|d| matches!(d, PathDue::Ping(_) | PathDue::Report | PathDue::Unchosen));
        self.measuring = false;
        self.keep = referee >= 0;
        if !self.keep && self.refereed.is_none() {
            self.let_go(svc);
        }
    }

    /// The pre-match is over before its match (pm.end, queue.state off) or begins again (a new pm.begin): a path
    /// that referees nothing here lets its links go (the next path.peers measures again).
    pub fn dropped(&mut self, svc: &mut NetService) {
        self.timers.cancel_where(|d| matches!(d, PathDue::Ping(_) | PathDue::Report | PathDue::Unchosen));
        self.measuring = false;
        if self.refereed.is_none() {
            self.keep = false;
            self.let_go(svc);
        }
    }

    /// A packet on Epic from `from` (an Epic id): true when the path took it (a ping, a pong, a ruled packet of the
    /// path's referee, a human's packet to the referee here).
    pub fn packet(&mut self, svc: &mut NetService, base: &SessionBase, from: &str, bytes: &[u8], now: f64) -> bool {
        if let Some((kind, n, stamp)) = read_path_ping(bytes) {
            let Some(i) = self.peers.iter().position(|p| p.epic == from) else { return true };
            if self.no_force {
                return true;
            }
            if kind == MSG_PATH_PING {
                lock(&self.epic).send(from, Channel::Match, &path_ping(MSG_PATH_PONG, n, stamp));
            } else if self.measuring {
                let ms = f64::from(clock_ms(now).wrapping_sub(stamp));
                let p = &mut self.peers[i];
                p.rtt = Some(p.rtt.map_or(ms, |r| r.min(ms)));
                if let Some(r) = lock(&self.epic).relayed(from) {
                    p.relayed = Some(r);
                }
            }
            return true;
        }
        if self.referee_take(svc, base, from, bytes, now) {
            return true;
        }
        // (the referee app's ruled packets come here alone, never through the session's match_packet)
        let is_referee = base.match_route.as_ref().is_some_and(|m| m.referee == from) && from != self.me_epic;
        if !is_referee {
            return self.peers.iter().any(|p| p.epic == from);
        }
        if packet_fits(bytes) {
            match self.watch.as_mut() {
                Some(w) if w.referee == from => w.last = now,
                _ => self.watch = Some(Watch { referee: from.to_string(), last: now, told: false }),
            }
            base.from_referee(svc, bytes);
        }
        true
    }

    /// Epic's word on a link to `peer`: relayed or direct, None when it went down.
    pub fn link(&mut self, peer: &str, relayed: Option<bool>) {
        if let Some(p) = self.peers.iter_mut().find(|p| p.epic == peer) {
            p.relayed = relayed;
        }
    }

    /// The Epic ids the path links to (the service's accept set on Epic takes them).
    pub fn epic_peers(&self) -> Vec<String> {
        if self.open_as().is_none() || self.no_force {
            return Vec::new();
        }
        let mut out: Vec<String> = self.peers.iter().map(|p| p.epic.clone()).collect();
        if let Some((_, r)) = &self.refereed {
            out.extend(
                r.slots.keys().filter(|id| **id != self.me_epic && !out.contains(id)).cloned().collect::<Vec<_>>(),
            );
        }
        out
    }

    /// While the path wants its links: Some(direct), the Direct rule's input (`NetService::relay_check`); else None.
    pub fn open_as(&self) -> Option<bool> {
        (self.measuring || self.keep || self.refereed.is_some()).then_some(self.direct)
    }

    /// A core message for the path's referee side (ref.go, ref.left, ref.end). The referee here keeps ruling after
    /// its own page leaves the match, to the core's ref.end, and ends with the session (`end`). A ref.go this app
    /// cannot rule (a forced path without Epic's Force, two humans of one Epic id) is let be: the core is told
    /// nothing, and the others' ref.lost or the leaver rule ends the match.
    pub fn ref_told(&mut self, svc: &mut NetService, base: &SessionBase, m: &ServerMessage, now: f64) {
        match m {
            ServerMessage::RefGo { r#match, in_ms, humans } => {
                self.end_referee();
                let forced = !self.direct && svc.epic_relay_mode() != Some(RelayMode::Force);
                let twice = humans.iter().enumerate().any(|(i, h)| humans[..i].iter().any(|o| o.epic == h.epic));
                if forced || twice {
                    let why = if forced { "Epic's relays cannot be forced" } else { "two humans of one Epic id" };
                    svc.log(&format!("path: match {match} not refereed here: {why}", match = r#match));
                    return;
                }
                // (the go is `in` from the core's send: half the round trip to it has passed already)
                // Unverified: tick 0 as now + in - rtt / 2 (the port's own estimate of the core's send time).
                let rtt = base.eos(svc).and_then(|b| lock(&b).rtt()).filter(|r| r.is_finite() && *r >= 0.0);
                let tick0 = now + f64::from(*in_ms) - rtt.unwrap_or(0.0) / 2.0;
                let slots: Vec<usize> = humans.iter().map(|h| h.slot as usize).collect();
                let r = Refereed {
                    referee: Referee::new(&slots, tick0),
                    ids: humans.iter().map(|h| (h.slot, h.epic.clone())).collect(),
                    slots: humans.iter().map(|h| (h.epic.clone(), h.slot)).collect::<IndexMap<_, _>>(),
                };
                self.refereed = Some((*r#match, r));
                self.keep = true;
                self.timers.every(now, REFEREE_PUMP_MS, PathDue::Pump);
                self.beat_on(now);
                svc.log(&format!("path: this app referees match {match}", match = r#match));
            }
            ServerMessage::RefLeft { r#match, slot } => {
                let Some((m, r)) = self.refereed.as_mut() else { return };
                if *m != *r#match {
                    return;
                }
                let Some(id) = r.ids.shift_remove(slot) else { return };
                r.slots.shift_remove(&id);
                r.referee.left(*slot as usize, now);
                svc.log(&format!("path: slot {slot} left match {m}"));
            }
            ServerMessage::RefEnd { r#match } if self.refereed.as_ref().is_some_and(|(m, _)| m == r#match) => {
                svc.log(&format!("path: match {match} over", match = r#match));
                self.end_referee();
                self.keep = false;
                if !self.measuring {
                    self.let_go(svc);
                }
            }
            _ => {}
        }
    }

    fn end_referee(&mut self) {
        if self.refereed.take().is_some() {
            self.timers.cancel_where(|d| *d == PathDue::Pump);
        }
    }

    /// A match packet from `from_epic` (or this player's own page) to the referee here; false when it runs none or
    /// the sender plays in none of its matches.
    pub fn referee_take(
        &mut self,
        svc: &mut NetService,
        base: &SessionBase,
        from_epic: &str,
        bytes: &[u8],
        now: f64,
    ) -> bool {
        // (the own page's packets come by this player's id on either backend)
        let from = if from_epic == base.me { self.me_epic.as_str() } else { from_epic };
        let Some((_, r)) = self.refereed.as_mut() else { return false };
        let Some(slot) = r.slots.get(from).copied() else { return false };
        if !packet_fits(bytes) {
            return true;
        }
        let mut outs = Outs::default();
        r.referee.receive(slot as usize, bytes, now, &mut outs);
        self.referee_outs(svc, base, outs);
        // (what it ruled goes out now, not at the clock's next pump)
        self.run_referee(svc, base, now);
        true
    }

    fn run_referee(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) {
        let Some((_, r)) = self.refereed.as_mut() else { return };
        let mut outs = Outs::default();
        r.referee.pump(now, &mut outs);
        self.referee_outs(svc, base, outs);
    }

    /// What the referee here put out: its ruled packets to the own page in process and to the others over Epic, its
    /// news to the core.
    fn referee_outs(&mut self, svc: &mut NetService, base: &SessionBase, outs: Outs) {
        for o in outs.0 {
            let Some((m, r)) = self.refereed.as_ref() else { return };
            match o {
                RefOut::Send(slot, bytes) => {
                    let Some(id) = u32::try_from(slot).ok().and_then(|s| r.ids.get(&s)) else { continue };
                    if *id == self.me_epic {
                        base.from_referee(svc, &bytes);
                    } else {
                        lock(&self.epic).send(id, Channel::Match, &bytes);
                    }
                }
                RefOut::News(slot, news) => {
                    let news = match news {
                        SlotNews::Away => RefNewsWire::Away,
                        SlotNews::Back => RefNewsWire::Back,
                        SlotNews::Gone => RefNewsWire::Gone,
                    };
                    let slot = u32::try_from(slot).unwrap_or(u32::MAX);
                    self.said.push(ClientMessage::RefNews { r#match: *m, slot, news }.text());
                }
            }
        }
    }

    /// The texts for the core since the last call (path.report, ref.news, ref.lost), in order: the session sends
    /// them on its link to the core.
    pub fn take_said(&mut self) -> Vec<String> {
        std::mem::take(&mut self.said)
    }

    /// The path's timers due by `now` (the pings, the report, the referee's pump, the ref.lost wait).
    pub fn pump(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) {
        while let Some((at, _, what)) = self.timers.take_due(now) {
            match what {
                PathDue::Ping(n) => {
                    // (stamped with the send's time, not the timer's: a late pump adds nothing to the round trip)
                    let ping = path_ping(MSG_PATH_PING, n, clock_ms(now));
                    let mut e = lock(&self.epic);
                    for p in &self.peers {
                        e.send(&p.epic, Channel::Match, &ping);
                    }
                }
                PathDue::Report => self.report(svc, base, at),
                PathDue::Unchosen => {
                    if !self.measuring && self.refereed.is_none() {
                        svc.log("path: no pm.path: the links go");
                        self.keep = false;
                        self.let_go(svc);
                    }
                }
                PathDue::Pump => self.run_referee(svc, base, at),
                PathDue::Beat => self.beat(svc, base, at),
            }
        }
    }

    /// The measure's report: the round trip to the relay and each peer's.
    fn report(&mut self, svc: &NetService, base: &SessionBase, now: f64) {
        self.measuring = false;
        let relay = base.eos(svc).and_then(|b| lock(&b).rtt()).and_then(whole_ms);
        let e = lock(&self.epic);
        let peers = self
            .peers
            .iter()
            .map(|p| PathLink {
                slot: p.slot,
                rtt: p.rtt.and_then(whole_ms),
                relayed: p.relayed.or_else(|| e.relayed(&p.epic)),
            })
            .collect();
        drop(e);
        self.said.push(ClientMessage::PathReport { relay, peers }.text());
        // (the links stay until pm.path says who referees, or PATH_WAIT_MS without it)
        self.keep = true;
        self.timers.after(now, PATH_WAIT_MS, PathDue::Unchosen);
    }

    /// The beat: while the page's match goes through a player's app, the relay client kept in a match and (another
    /// human's app refereeing) the ref.lost wait looked at; a path match over lets the links go.
    fn beat(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) {
        let route = base.match_route.as_ref().map(|m| m.referee.clone());
        let referee = route.clone().filter(|r| self.is_path_referee(r));
        let Some(referee) = referee else {
            // (the page's match over, as another human; or its go named the relay though pm.path named a player: the
            // links go. The referee here keeps ruling to ref.end, even once its own page has left the match)
            let fell_back = route.is_some() && self.keep;
            if (self.watch.take().is_some() || fell_back) && self.refereed.is_none() && !self.measuring {
                self.keep = false;
                self.let_go(svc);
            }
            return;
        };
        if let Some(relay) = base.eos(svc)
            && !self.core.is_empty()
        {
            lock(&relay).send(&self.core, Channel::Match, &[]);
        }
        if referee == self.me_epic {
            return;
        }
        let w = match self.watch.as_mut() {
            Some(w) if w.referee == referee => w,
            _ => self.watch.insert(Watch { referee, last: now, told: false }),
        };
        // Unverified: looked at on the beat, so up to MATCH_BEAT_MS late.
        if !w.told && now - w.last >= RESUME_MS {
            w.told = true;
            svc.log("path: no ruled packet from the match's referee app: lost");
            self.said.push(ClientMessage::RefLost { r#match: 0 }.text());
        }
    }

    /// A route's referee is a player's app of this path (this player's own among them).
    fn is_path_referee(&self, id: &str) -> bool {
        id == self.me_epic
            || self.peers.iter().any(|p| p.epic == id)
            || self.refereed.as_ref().is_some_and(|(_, r)| r.slots.contains_key(id))
    }

    pub fn next_due(&self) -> Option<f64> {
        self.next_key().map(|k| k.0)
    }

    /// The path's first timer: its time and set order.
    pub fn next_key(&self) -> Option<(f64, u64)> {
        self.timers.next_key()
    }

    /// The links to these Epic ids closed, unless another session or a party uses them.
    fn close_links(&self, svc: &NetService, ids: &[String]) {
        let via = Via::Backend(BackendKind::Epic);
        for id in ids {
            if !svc.peer_in_use(via, id) {
                lock(&self.epic).close_peer(id);
            }
        }
    }

    /// The path's links let go and its beat stopped (the service sees the path closed when the session goes back in
    /// its page).
    fn let_go(&mut self, svc: &mut NetService) {
        let ids: Vec<String> = self.peers.drain(..).map(|p| p.epic).collect();
        self.close_links(svc, &ids);
        self.watch = None;
        self.timers.cancel_where(|d| *d == PathDue::Beat);
        self.beat = None;
    }

    /// The match or the session is over: the referee here ended, the path's links let go (unless in use).
    pub fn end(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) {
        let _ = (base, now);
        self.end_referee();
        self.timers.clear();
        self.measuring = false;
        self.keep = false;
        self.let_go(svc);
    }
}

/// A core text that may be the match's path's (path.peers, ref.*, pm.path; pm.begin, pm.end and queue.state, which
/// end a path's measure), by its text alone.
pub fn is_path_text(text: &str) -> bool {
    ["\"path.", "\"ref.", "\"pm.path\"", "\"pm.begin\"", "\"pm.end\"", "\"queue.state\""]
        .iter()
        .any(|t| text.contains(t))
}

/// A core text of the match's path told to a session's path, made at the core's path.peers (`core` the core's id on
/// the relay): path.peers and the referee's ref.* are the path's (true: not the page's); pm.path is told the path
/// and goes on to the page. The session sends the path's texts after (`said`).
pub fn told(
    path: &mut Option<Box<PathSide>>,
    svc: &mut NetService,
    base: &SessionBase,
    core: &str,
    text: &str,
    now: f64,
) -> bool {
    let Ok(m) = serde_json::from_str::<ServerMessage>(text) else { return false };
    match &m {
        ServerMessage::PathPeers { peers, direct } => {
            if path.is_none() {
                let Some(epic) = svc.eos_of(BackendKind::Epic) else {
                    svc.log("path: the core measures, but Epic is not signed in");
                    return true;
                };
                let me = lock(&epic).me().to_string();
                let mut p = PathSide::new(epic, me);
                p.set_core(core);
                *path = Some(Box::new(p));
            }
            if let Some(p) = path.as_mut() {
                p.peers_told(svc, base, peers, *direct, now);
            }
            true
        }
        ServerMessage::RefGo { .. } | ServerMessage::RefLeft { .. } | ServerMessage::RefEnd { .. } => {
            if let Some(p) = path.as_mut() {
                p.ref_told(svc, base, &m, now);
            }
            true
        }
        ServerMessage::PmPath { referee, .. } => {
            if let Some(p) = path.as_mut() {
                p.chose(svc, *referee, now);
            }
            false
        }
        ServerMessage::PmBegin(_) | ServerMessage::PmEnd { .. } | ServerMessage::QueueState { on: false, .. } => {
            if let Some(p) = path.as_mut() {
                p.dropped(svc);
            }
            false
        }
        _ => false,
    }
}

/// The path's texts for the core, on the session's link to it (`g`): false when a send failed (the session ends).
pub fn said(path: &mut Option<Box<PathSide>>, svc: &NetService, base: &SessionBase, g: Option<&Guesting>) -> bool {
    let Some(said) = path.as_deref_mut().map(PathSide::take_said) else { return true };
    for t in said {
        if base.closed {
            return true;
        }
        if !g.is_some_and(|g| g.send(svc, base, &t)) {
            return false;
        }
    }
    true
}

/// The page's hello as the core takes it: with this player's Epic id while Epic is signed in, its round trip to the
/// relay and its Direct setting (a match's path, `mvs_protocol::path`); a key the page wrote stays.
pub fn hello_text(svc: &NetService, base: &SessionBase, text: &str) -> String {
    let m: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    if m.get("t").and_then(Value::as_str) != Some("hello") {
        return text.to_string();
    }
    let mut keys: Vec<(&str, String)> = Vec::new();
    let epic = svc.eos_of(BackendKind::Epic).map(|e| lock(&e).me().to_string()).filter(|id| !id.is_empty());
    if let Some(id) = epic
        && m.get("epic").is_none()
    {
        keys.push(("epic", serde_json::to_string(&id).expect("a string")));
    }
    let rtt = base.eos(svc).and_then(|b| lock(&b).rtt()).and_then(round_ms);
    if let Some(ms) = rtt
        && m.get("rtt").is_none()
    {
        keys.push(("rtt", ms.to_string()));
    }
    if m.get("direct").is_none() {
        keys.push(("direct", svc.config().allow_direct.to_string()));
    }
    with_keys(text, &keys)
}

/// A JSON object's text with these keys (their JSON values) last; the text as it is when it is no object.
pub fn with_keys(text: &str, keys: &[(&str, String)]) -> String {
    let t = text.trim_end();
    let Some(body) = t.strip_suffix('}') else { return text.to_string() };
    let mut out = body.to_string();
    for (k, v) in keys {
        let sep = if out.trim_end().ends_with('{') { "" } else { "," };
        out.push_str(sep);
        out.push('"');
        out.push_str(k);
        out.push_str("\":");
        out.push_str(v);
    }
    out.push('}');
    out
}

/// Whole ms, rounded (Math.round as the core reads a whole number); None when it is no round trip.
pub fn round_ms(ms: f64) -> Option<u32> {
    if !ms.is_finite() || ms < 0.0 {
        return None;
    }
    let ms = (ms + 0.5).floor();
    if ms > f64::from(u32::MAX) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a whole, non-negative count of ms")]
    let n = ms as u32;
    Some(n)
}

/// The service's clock as a ping's stamp (whole ms, modulo 2^32).
fn clock_ms(now: f64) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a stamp modulo 2^32 of whole ms")]
    let n = (now.max(0.0).floor() as u64 & u64::from(u32::MAX)) as u32;
    n
}

/// A round trip as whole ms (rounded), None when it is none.
fn whole_ms(ms: f64) -> Option<u32> {
    if !ms.is_finite() || ms < 0.0 {
        return None;
    }
    let r = (ms + 0.5).floor();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a whole, non-negative count of ms")]
    let n = (r <= f64::from(u32::MAX)).then_some(r as u32);
    n
}

#[cfg(test)]
mod tests {
    use indexmap::IndexSet;
    use serde_json::Value;

    use mvs_protocol::match_wire::{RESUME_MS, TICK_MS};
    use mvs_protocol::messages::{PathPeer, ServerMessage};
    use mvs_protocol::referee::GONE_TICKS;

    use super::PathSide;
    use crate::backend::memory::{MemoryNet, MemoryNetOptions};
    use crate::backend::{BackendEvent, BackendKind, RelayMode};
    use crate::service::page::PageId;
    use crate::service::session::{MatchRoute, SessionBase, Via};
    use crate::service::{NetService, ServiceOptions, lock};

    /// One player: its service with an Epic backend of the memory net, its session's base, its path.
    struct Side {
        svc: NetService,
        base: SessionBase,
        path: PathSide,
        said: Vec<Value>,
    }

    fn side(net: &MemoryNet, id: &str, other: &str) -> Side {
        let mut svc = NetService::new(ServiceOptions::default(), 0.0);
        svc.start_backend(BackendKind::Epic, Box::new(net.player(Some(id), BackendKind::Epic)), 0.0);
        let epic = svc.eos_of(BackendKind::Epic).expect("Epic on");
        // (the service's accept sets, set here as the service sets them)
        lock(&epic).set_accept(IndexSet::from([other.to_string()]));
        let base = SessionBase::new(PageId(0), Some(BackendKind::Relay), true, false);
        Side { svc, base, path: PathSide::new(epic, id.to_string()), said: Vec::new() }
    }

    /// Both sides from `from` to `to` (ms), a ms at a time: the net's deliveries, each side's Epic events to its path,
    /// its timers.
    fn run(net: &MemoryNet, sides: &mut [&mut Side], from: f64, to: f64) {
        let mut t = from;
        while t <= to {
            net.pump(t);
            for s in sides.iter_mut() {
                let epic = s.svc.eos_of(BackendKind::Epic).expect("Epic on");
                loop {
                    let e = lock(&epic).poll(t);
                    match e {
                        Some(BackendEvent::Packet { from, bytes, .. }) => {
                            s.path.packet(&mut s.svc, &s.base, &from, &bytes, t);
                        }
                        Some(BackendEvent::Link { peer, relayed }) => s.path.link(&peer, relayed),
                        Some(_) => {}
                        None => break,
                    }
                }
                s.path.pump(&mut s.svc, &s.base, t);
                for x in s.path.take_said() {
                    s.said.push(serde_json::from_str(&x).expect("JSON"));
                }
            }
            t += 1.0;
        }
    }

    fn peers(p: &[(u32, &str)]) -> Vec<PathPeer> {
        p.iter().map(|(slot, epic)| PathPeer { slot: *slot, epic: (*epic).to_string() }).collect()
    }

    #[test]
    fn two_players_measure_each_other_and_report() {
        let net = MemoryNet::new(MemoryNetOptions { delay_ms: 5.0, ..Default::default() }, 0.0);
        let mut a = side(&net, "ea", "eb");
        let mut b = side(&net, "eb", "ea");
        let told = peers(&[(0, "ea"), (1, "eb")]);
        a.path.peers_told(&mut a.svc, &a.base, &told, false, 0.0);
        b.path.peers_told(&mut b.svc, &b.base, &told, false, 0.0);
        assert_eq!(a.path.open_as(), Some(false), "a queue's path wants its links, never direct");
        assert_eq!(a.path.epic_peers(), vec!["eb".to_string()], "the own id is no peer");
        assert_eq!(a.svc.epic_relay_mode(), Some(RelayMode::Force), "forced before the first ping");
        run(&net, &mut [&mut a, &mut b], 0.0, 2300.0);
        for s in [&a, &b] {
            assert_eq!(s.said.len(), 1, "one report: {:?}", s.said);
            let r = &s.said[0];
            assert_eq!(r["t"], "path.report");
            assert!(r.get("relay").is_none(), "no relay round trip known: {r}");
            let p = &r["peers"][0];
            assert_eq!(p["rtt"], 10, "5 ms each way: {r}");
            assert_eq!(p["relayed"], true, "a memory link under Force is relayed: {r}");
        }
        assert_eq!(a.said[0]["peers"][0]["slot"], 1);
        assert_eq!(b.said[0]["peers"][0]["slot"], 0);
        // (the relay chosen: the links go, the path wants none)
        a.path.chose(&mut a.svc, -1, 2400.0);
        assert_eq!(a.path.open_as(), None);
        assert!(a.path.epic_peers().is_empty());
    }

    #[test]
    fn a_peer_that_never_answers_reports_no_round_trip() {
        let net = MemoryNet::new(MemoryNetOptions { delay_ms: 5.0, ..Default::default() }, 0.0);
        let mut a = side(&net, "ea", "eb");
        let mut b = side(&net, "eb", "ea");
        a.path.peers_told(&mut a.svc, &a.base, &peers(&[(0, "ea"), (1, "eb")]), true, 0.0);
        assert_eq!(a.path.open_as(), Some(true), "a direct custom path");
        // (b was told nothing: its path answers no ping)
        run(&net, &mut [&mut a, &mut b], 0.0, 2300.0);
        assert_eq!(a.said.len(), 1);
        assert!(a.said[0]["peers"][0].get("rtt").is_none(), "no pong, no round trip: {}", a.said[0]);
    }

    #[test]
    fn the_referee_here_tells_who_went_silent_and_ends() {
        let net = MemoryNet::new(MemoryNetOptions { delay_ms: 5.0, ..Default::default() }, 0.0);
        let mut a = side(&net, "ea", "eb");
        let mut b = side(&net, "eb", "ea");
        let told = peers(&[(0, "ea"), (1, "eb")]);
        a.path.peers_told(&mut a.svc, &a.base, &told, false, 0.0);
        a.path.chose(&mut a.svc, 0, 10.0);
        let go = ServerMessage::RefGo { r#match: 4, in_ms: 250, humans: told.clone() };
        a.path.ref_told(&mut a.svc, &a.base, &go, 10.0);
        assert_eq!(a.path.open_as(), Some(false), "the referee keeps its links");
        assert!(!a.path.referee_take(&mut a.svc, &a.base, "ex", &[1, 2, 3], 11.0), "no human of the match");
        assert!(a.path.referee_take(&mut a.svc, &a.base, "eb", &[1], 11.0), "a human's, dropped as it does not fit");
        let gone = 260.0 + f64::from(GONE_TICKS + 60) * TICK_MS;
        run(&net, &mut [&mut a, &mut b], 11.0, gone);
        let news: Vec<(u64, String)> = a
            .said
            .iter()
            .filter(|m| m["t"] == "ref.news" && m["match"] == 4)
            .map(|m| (m["slot"].as_u64().unwrap_or(9), m["news"].as_str().unwrap_or("").to_string()))
            .collect();
        assert!(news.contains(&(1, "away".into())), "{news:?}");
        assert!(news.contains(&(1, "gone".into())), "{news:?}");
        a.path.ref_told(&mut a.svc, &a.base, &ServerMessage::RefEnd { r#match: 3 }, gone);
        assert!(a.path.open_as().is_some(), "another match's end changes nothing");
        a.path.ref_told(&mut a.svc, &a.base, &ServerMessage::RefEnd { r#match: 4 }, gone);
        assert_eq!(a.path.open_as(), None, "the match over: the links go");
    }

    #[test]
    fn another_human_tells_the_core_once_when_the_referee_app_is_lost() {
        let net = MemoryNet::new(MemoryNetOptions { delay_ms: 5.0, ..Default::default() }, 0.0);
        let mut a = side(&net, "ea", "eb");
        let mut b = side(&net, "eb", "ea");
        let told = peers(&[(0, "ea"), (1, "eb")]);
        b.path.peers_told(&mut b.svc, &b.base, &told, false, 0.0);
        b.path.chose(&mut b.svc, 0, 10.0);
        b.base.match_route = Some(MatchRoute {
            peers: vec!["ra".into(), "rb".into()],
            slot: 1,
            referee: "ea".into(),
            on: Some(Via::Backend(BackendKind::Epic)),
        });
        // (a's app rules nothing)
        run(&net, &mut [&mut a, &mut b], 0.0, 300.0 + RESUME_MS + 600.0);
        let lost: Vec<&Value> = b.said.iter().filter(|m| m["t"] == "ref.lost").collect();
        assert_eq!(lost.len(), 1, "once: {:?}", b.said);
        assert_eq!(lost[0]["match"], 0);
        // (the page's match over: the links go)
        b.base.match_route = None;
        run(&net, &mut [&mut a, &mut b], 21500.0, 21800.0);
        assert_eq!(b.path.open_as(), None);
    }
}
