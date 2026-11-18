//! A page's session on a host core the relay runs (wire 5, online plan section 4): the relay's queue core
//! (`relay_wire::QUEUE_HOST`), or the core of a custom lobby this page leads, made with `Backend::host_lobby` on a
//! relay or on a LAN direct host of its own (whose relay core hosts it the same way). The page is the core's member
//! like any other: its texts go over a [`Guesting`] to the host id, the core's come back from it. No host core and no
//! referee run in this process.
//!
//! The queue: the page's queue.join goes to the core with the backend's smallest round trip (`rtt`); a party's members
//! each join with their party, and the core holds them as one ticket. What the session reads of the core's texts is
//! whether the page is queued: a party out of the queue on the other side (its party's `Out`) cancels this side's
//! ticket, when the page's join carried its party, and tells the page so, the core's own off that follows not passed
//! on again.
//!
//! A custom lobby: the lobby made with `host_lobby`, its code (on the release's relay the short code the relay gave it,
//! on another relay its relay code, on LAN the direct host's), then the leader linked to the lobby's host id as a
//! member. The listing the lobby's core tells its owner (`BackendEvent::Listing`, None while private) is the lobby's:
//! on a relay it goes to the backend's list (`set_listed`, with the code), as the in-process host lists its lobby; on
//! LAN the session announces it, counting the core's humans.

use serde_json::Value;

use mvs_protocol::relay_wire::QUEUE_HOST;

use crate::backend::{Answer, Ask, BackendKind, Listed, LobbyChange};
use crate::codes::is_short;
use crate::lan::announce::{Announcement, AnnouncerOptions, LanAnnouncer};
use crate::lan::direct::{DirectHost, DirectHostOptions, HostStartWhy, begin_host};
use crate::lan::direct_code::DIRECT_ADDRESSES;
use crate::pending::Pending;
use crate::service::api::{ClosedWhy, LanOptions, NetEvent};
use crate::service::custom::make_why;
use crate::service::hosting::{GuestNote, Guesting};
use crate::service::page::{PageId, Timers};
use crate::service::party::{PartyNote, SessionNote};
use crate::service::path::{self, PathSide, hello_text, round_ms, with_keys};
use crate::service::session::{SessionBase, Via};
use crate::service::{CUSTOM_BUCKET, CUSTOM_MAX, LIST_GAP_MS, LIST_RETRY_MS, NetService, OwnKind, lock};

/// What the queue's session knows of the page's place in the core's queue.
#[derive(Default)]
struct Queued {
    /// The page's queue.join went to the core, and no off, found or cancel since (Epic's `searching`).
    searching: bool,
    /// That join carried the page's party.
    party: bool,
    /// The core sent queue.found since the page's last join.
    found: bool,
    /// This side took the page out for its party (and told it): the core's offs are not passed on until the page's
    /// next join or cancel.
    quiet_off: bool,
}

/// A custom lobby's own parts: its create, its id, and on LAN its direct host and announcements.
#[derive(Default)]
struct Custom {
    /// The create out (its ask).
    creating: Option<Ask>,
    /// The lobby's id ('' until made).
    id: String,
    /// The lobby's members, as the backend last told them.
    members: u32,
    /// On LAN: the direct host's link (its number) and whether it is on loopback, its start under way, the lobby's
    /// announcements, and the core's latest listing (None: private).
    lan: Option<(u32, bool)>,
    starting: Option<(Pending<Result<DirectHost, HostStartWhy>>, bool)>,
    announcer: Option<LanAnnouncer>,
    lan_info: Option<String>,
    /// The humans of the core's lobby, from its last lobby.state (None: none seen yet; the backend's count then).
    core_members: Option<u32>,
    /// On a relay: the listing the backend is given.
    listing: Listing,
}

/// A relay lobby's listing (custom.rs HostSession's): the core's latest, the one the backend has, whether an update
/// is out or waiting its gap, the lobby's `at` (its create's time, the browser's order).
#[derive(Default)]
struct Listing {
    want: Option<String>,
    has: Option<String>,
    busy: bool,
    at: f64,
    /// The update out: its ask and its text.
    out: Option<(Ask, Option<String>)>,
}

#[derive(Clone, Debug, PartialEq)]
enum HostedDue {
    /// The listing's gap is over.
    List,
}

/// The session of a page on a relay-hosted core.
pub struct RelayHosted {
    pub base: SessionBase,
    /// The link to the core, once the lobby (a custom one) is made.
    pub g: Option<Guesting>,
    /// The queue's core (true) or a custom lobby's.
    pub queue: bool,
    queued: Queued,
    custom: Custom,
    notes: Vec<PartyNote>,
    timers: Timers<HostedDue>,
    /// The match's path while the core measures or a player's app referees (None: the relay's alone).
    pub path: Option<Box<PathSide>>,
}

impl RelayHosted {
    fn new(base: SessionBase, queue: bool) -> RelayHosted {
        RelayHosted {
            base,
            g: None,
            queue,
            queued: Queued::default(),
            custom: Custom::default(),
            notes: Vec::new(),
            timers: Timers::default(),
            path: None,
        }
    }

    /// A packet on Epic from `from` for the path (`PathSide::packet`): true when it took it.
    pub fn path_packet(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) -> bool {
        let base = &self.base;
        let taken = self.path.as_mut().is_some_and(|p| p.packet(svc, base, from, bytes, now));
        self.path_said(svc, now);
        taken
    }

    /// A match packet to the referee this app runs for its path (`PathSide::referee_take`).
    pub fn path_referee_take(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) -> bool {
        let base = &self.base;
        let taken = self.path.as_mut().is_some_and(|p| p.referee_take(svc, base, from, bytes, now));
        self.path_said(svc, now);
        taken
    }

    /// The path's texts for the core, on the link to it (`path::said`).
    fn path_said(&mut self, svc: &mut NetService, now: f64) {
        if !path::said(&mut self.path, svc, &self.base, self.g.as_ref()) {
            self.end(svc, ClosedWhy::SendFailed, now);
        }
    }

    /// The path's end: its links let go.
    fn end_path(&mut self, svc: &mut NetService, now: f64) {
        if let Some(mut p) = self.path.take() {
            p.end(svc, &self.base, now);
        }
    }

    /// The relay's queue: the page's queue.join goes to QUEUE_HOST with the backend's round trip (`Backend::rtt`).
    pub fn open_queue(svc: &mut NetService, page: PageId, now: f64) -> RelayHosted {
        let kind = BackendKind::Relay;
        let mut base = SessionBase::new(page, Some(kind), true, false);
        base.me = svc.me_on(Via::Backend(kind));
        let mut s = RelayHosted::new(base, true);
        svc.post(page, s.base.opened());
        s.g = Some(Guesting::new(svc, &s.base, "", QUEUE_HOST, QUEUE_HOST, &[], now));
        s
    }

    /// A custom lobby the relay (or a LAN host) on `via` hosts, led by this page: made with `host_lobby`, then linked
    /// to its host id as a member.
    pub fn open_custom(svc: &mut NetService, page: PageId, via: Via, now: f64) -> RelayHosted {
        let backend = match via {
            Via::Backend(k) => Some(k),
            Via::Own(_) => None,
        };
        let mut base = SessionBase::new(page, backend, false, false);
        base.via = Some(via);
        base.me = svc.me_on(via);
        let mut s = RelayHosted::new(base, false);
        s.create(svc, now);
        s
    }

    /// A custom lobby on LAN: a direct host of its own (whose relay core hosts the lobby), its code the direct code.
    pub fn open_lan(page: PageId, o: &LanOptions) -> RelayHosted {
        let base = SessionBase::new(page, Some(BackendKind::Lan), false, false);
        let mut s = RelayHosted::new(base, false);
        start_direct(&mut s, o);
        s
    }

    /// A start under way (the service polls it).
    pub fn pending(&self) -> bool {
        self.custom.starting.is_some()
    }

    /// The direct host's start, once it is over: its link, the listening line, the lobby's create.
    pub fn poll_pending(&mut self, svc: &mut NetService, now: f64) {
        let Some((p, loopback)) = self.custom.starting.as_mut() else { return };
        let loopback = *loopback;
        let Some(d) = p.poll() else { return };
        self.custom.starting = None;
        let mut d = match d {
            Ok(d) => d,
            Err(why) => {
                svc.listening(None, None, Some(why.clone()));
                return self.base.finish(svc, ClosedWhy::MakeHostFailed { why }, now);
            }
        };
        // (a session ended while its host started: the host is closed, never listed)
        if self.base.closed {
            return d.close();
        }
        let backend = d.backend.take().expect("a direct host's start gives its link");
        let at = format!("{}:{}", d.addresses.first().cloned().unwrap_or_default(), d.port);
        let n = svc.add_own(Box::new(backend), OwnKind::DirectHost(Box::new(d)));
        self.base.via = Some(Via::Own(n));
        self.base.release = Some(n);
        self.base.me = svc.me_on(Via::Own(n));
        self.custom.lan = Some((n, loopback));
        svc.listening(Some((n, at)), None, None);
        self.create(svc, now);
    }

    fn create(&mut self, svc: &mut NetService, now: f64) {
        let Some(b) = self.base.via.and_then(|v| svc.backend(v)) else { return };
        let ask = lock(&b).host_lobby(CUSTOM_BUCKET, CUSTOM_MAX, None);
        match ask {
            Some(a) => self.custom.creating = Some(a),
            None => {
                svc.log("custom: the backend hosts no lobby");
                self.base.finish(svc, ClosedWhy::BadAnswer, now);
            }
        }
    }

    /// The lobby is made: its code, the page told, the leader's link to the lobby's core. `short` the relay's short
    /// code of it, its code on the release's relay.
    fn created(&mut self, svc: &mut NetService, id: &str, host: &str, short: Option<&str>, now: f64) {
        let code = if let Some((n, loopback)) = self.custom.lan {
            let Some(OwnKind::DirectHost(d)) = svc.own.get(&n).map(|l| &l.kind) else { return };
            let code = match d.code(id) {
                Ok(c) => c,
                Err(why) => {
                    self.base.leave_lobby(svc, id, false);
                    return self.base.finish(svc, ClosedWhy::MakeCodeFailed { why }, now);
                }
            };
            let Some(OwnKind::DirectHost(d)) = svc.own.get(&n).map(|l| &l.kind) else { return };
            let addresses = d.addresses.clone();
            let port = d.port;
            let listening = addresses.iter().map(|a| format!("{a}:{port}")).collect::<Vec<_>>().join(", ");
            svc.log(&format!("direct: listening on {listening}"));
            // (announced on the discovery port, where an app with LAN on listens while it browses)
            self.custom.announcer = Some(LanAnnouncer::new(AnnouncerOptions {
                port: Some(svc.discovery_port()),
                addresses: addresses.into_iter().take(DIRECT_ADDRESSES).collect(),
                loopback,
                log: None,
                interfaces: None,
            }));
            code
        } else if let Some(s) = short.filter(|s| is_short(s) && svc.on_release_relay()) {
            s.to_string()
        } else {
            let kind = self.base.backend.unwrap_or(BackendKind::Relay);
            match svc.code_on(kind, id) {
                Ok(c) => c,
                Err(e) => {
                    self.base.leave_lobby(svc, id, false);
                    return self.base.finish(svc, e.closed(false), now);
                }
            }
        };
        if self.base.closed {
            return self.base.leave_lobby(svc, id, false);
        }
        self.base.code = code;
        self.custom.id = id.to_string();
        self.custom.members = 1;
        self.custom.listing.at = svc.wall();
        svc.log(&format!("hosted lobby {id} on {host}"));
        let e = self.base.opened();
        svc.post(self.base.page, e);
        let me = self.base.me.clone();
        self.g = Some(Guesting::new(svc, &self.base, id, host, &me, std::slice::from_ref(&me), now));
    }

    /// On LAN: the lobby's announcement as it is now (`info` the core's listing, None while it is private). `info`
    /// None: the last one.
    fn announce(&mut self, info: Option<Option<String>>, now: f64) {
        if let Some(i) = info {
            self.custom.lan_info = i;
        }
        if self.base.closed {
            return;
        }
        let members = self.custom.core_members.unwrap_or(self.custom.members).max(1);
        let Some(a) = self.custom.announcer.as_mut() else { return };
        let x = self.custom.lan_info.as_ref().map(|i| Announcement {
            code: self.base.code.clone(),
            info: i.clone(),
            members,
            max: CUSTOM_MAX,
        });
        a.set(x.as_ref(), now);
    }

    /// The listing of a lobby this session leads changed (the backend's `Listing`, None: private): a LAN host
    /// announces it; on a relay it goes to the backend's list.
    pub fn listing(&mut self, svc: &mut NetService, lobby: &str, info: Option<String>, now: f64) {
        if self.queue || self.custom.id.is_empty() || self.custom.id != lobby {
            return;
        }
        if self.custom.lan.is_some() {
            return self.announce(Some(info), now);
        }
        self.list(svc, info);
    }

    /// The core's listing goes to the backend: one update at a time, LIST_GAP_MS apart, the latest text winning
    /// (HostSession::list).
    fn list(&mut self, svc: &mut NetService, text: Option<String>) {
        self.custom.listing.want = text;
        let l = &self.custom.listing;
        if l.busy || self.base.closed || l.want == l.has {
            return;
        }
        let Some(b) = self.base.via.and_then(|v| svc.backend(v)) else { return };
        let want = l.want.clone();
        let listed = want.clone().map(|i| Listed { at: l.at, info: Some(i), code: Some(self.base.code.clone()) });
        let ask = lock(&b).set_listed(&self.custom.id, listed);
        self.custom.listing.busy = true;
        self.custom.listing.out = Some((ask, want));
    }

    pub fn in_lobby(&self, id: &str) -> bool {
        !id.is_empty() && self.g.as_ref().is_some_and(|g| !g.done && g.lobby == id)
    }

    /// The core (its host id) and the lobby's members.
    pub fn has_peer(&self, peer: &str) -> bool {
        self.g.as_ref().is_some_and(|g| !g.done && (g.host == peer || g.members.contains(peer)))
    }

    /// The core, the lobby's other members and the match's referee (a host's id, whose packets a LAN backend takes
    /// by its accept set alone).
    pub fn peers(&self) -> Vec<String> {
        let Some(g) = self.g.as_ref().filter(|g| !g.done) else { return Vec::new() };
        let me = &self.base.me;
        let mut out: Vec<String> = Vec::new();
        out.push(g.host.clone());
        out.extend(g.members.iter().filter(|m| *m != me && **m != g.host).cloned());
        if let Some(m) = &self.base.match_route
            && !out.contains(&m.referee)
        {
            out.push(m.referee.clone());
        }
        out
    }

    /// A text of the page to the core.
    pub fn from_page(&mut self, svc: &mut NetService, text: &str, now: f64) {
        if self.base.closed {
            return;
        }
        if self.g.is_none() {
            return;
        }
        let text = if self.queue { self.queue_text(svc, text) } else { text.to_string() };
        let text = if text.contains("\"hello\"") { hello_text(svc, &self.base, &text) } else { text };
        if !self.g.as_ref().is_some_and(|g| g.send(svc, &self.base, &text)) {
            self.end(svc, ClosedWhy::SendFailed, now);
        }
    }

    /// The page's queue texts as the core takes them: a queue.join with this player's round trip to the relay (the
    /// page's own `rtt`, if it wrote one, stays). A join or a cancel of the page's ends this side's quiet.
    fn queue_text(&mut self, svc: &NetService, text: &str) -> String {
        let m: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let t = m.get("t").and_then(Value::as_str).unwrap_or_default();
        if t == "queue.cancel" {
            self.queued.quiet_off = false;
            self.queued.searching = false;
        }
        if t != "queue.join" {
            return text.to_string();
        }
        self.queued.quiet_off = false;
        self.queued.found = false;
        self.queued.searching = true;
        // (a party of one queues solo: QueueSession's party_ids, empty below two)
        self.queued.party =
            m.get("party").and_then(|p| p.get("members")).and_then(Value::as_array).is_some_and(|ids| ids.len() > 1);
        let rtt = self.base.eos(svc).and_then(|b| lock(&b).rtt());
        match rtt {
            Some(ms) if m.get("rtt").is_none() && ms.is_finite() && ms >= 0.0 => with_rtt(text, ms),
            _ => text.to_string(),
        }
    }

    /// A text of the core: what the queue reads of it, then to the page.
    fn core_said(&mut self, svc: &mut NetService, text: &str, now: f64) {
        if self.base.closed {
            return;
        }
        if path::is_path_text(text) && (self.path_told(svc, text, now) || self.base.closed) {
            return;
        }
        if self.queue {
            let m: Value = serde_json::from_str(text).unwrap_or(Value::Null);
            match m.get("t").and_then(Value::as_str).unwrap_or_default() {
                "queue.state" => {
                    if m.get("on") == Some(&Value::Bool(true)) {
                        self.queued.quiet_off = false;
                    } else {
                        self.queued.searching = false;
                        if self.queued.quiet_off {
                            return;
                        }
                    }
                }
                "queue.found" => {
                    self.queued.found = true;
                    self.queued.searching = false;
                }
                _ => {}
            }
        }
        if !self.queue && text.contains("lobby.state") {
            self.core_lobby(text, now);
        }
        svc.post(self.base.page, NetEvent::Msg(text.to_string()));
    }

    /// A core text of the match's path (`path::told`): true when it is the path's alone.
    fn path_told(&mut self, svc: &mut NetService, text: &str, now: f64) -> bool {
        let core = self.g.as_ref().map(|g| g.host.clone()).unwrap_or_default();
        let took = path::told(&mut self.path, svc, &self.base, &core, text, now);
        self.path_said(svc, now);
        took
    }

    /// A lobby.state of the core: its humans, which the LAN announcement counts.
    fn core_lobby(&mut self, text: &str, now: f64) {
        let m: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        if m.get("t").and_then(Value::as_str) != Some("lobby.state") {
            return;
        }
        let Some(teams) = m.get("data").and_then(|d| d.get("teams")).and_then(Value::as_array) else { return };
        let humans =
            teams.iter().filter_map(Value::as_array).flatten().filter(|p| p.get("isBot") != Some(&Value::Bool(true)));
        let humans = u32::try_from(humans.count()).unwrap_or(CUSTOM_MAX);
        if humans > 0 && self.custom.core_members != Some(humans) {
            self.custom.core_members = Some(humans);
            self.announce(None, now);
        }
    }

    fn notes_of(&mut self, svc: &mut NetService, notes: Vec<GuestNote>, now: f64) {
        for n in notes {
            match n {
                GuestNote::Text(t) => self.core_said(svc, &t, now),
                GuestNote::Lost(why) => {
                    // (the match first, as over's setMatch(null): the link is done, so the core is no peer now)
                    self.base.set_match(svc, None, false);
                    self.end_announcer();
                    self.end_path(svc, now);
                    self.base.finish(svc, why, now);
                }
            }
        }
    }

    /// A channel-0 packet from `from` (the core's pieces and bye).
    pub fn message(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) {
        if self.base.closed {
            return;
        }
        let Some(g) = self.g.as_mut() else { return };
        let n = g.message(svc, &self.base, from, bytes, now);
        self.notes_of(svc, n, now);
    }

    pub fn lobby_changed(&mut self, svc: &mut NetService, c: &LobbyChange, now: f64) {
        if self.base.closed {
            return;
        }
        if let LobbyChange::Members { members, .. } = c {
            self.custom.members = u32::try_from(members.len()).unwrap_or(CUSTOM_MAX);
        }
        if let Some(g) = self.g.as_mut() {
            g.lobby_changed(svc, &self.base, c, now);
        }
        self.announce(None, now);
    }

    pub fn peer_closed(&mut self, svc: &mut NetService, peer: &str, now: f64) {
        let Some(g) = self.g.as_mut() else { return };
        let n = g.peer_closed(svc, &self.base, peer, now);
        self.notes_of(svc, n, now);
    }

    fn end_announcer(&mut self) {
        if let Some(mut a) = self.custom.announcer.take() {
            a.close();
        }
    }

    /// The session is over for this page: the core told (FRAME_BYE), the lobby left; a leader's leave ends the
    /// lobby and its core on the relay.
    pub fn end(&mut self, svc: &mut NetService, why: ClosedWhy, now: f64) {
        if self.base.closed {
            return;
        }
        self.end_announcer();
        self.end_path(svc, now);
        // (a start under way is cancelled: its host, if it comes, is closed as it is dropped)
        self.custom.starting = None;
        self.base.finish(svc, why.clone(), now);
        if let Some(g) = self.g.as_mut() {
            g.end(svc, &self.base, &why, now);
        } else if !self.custom.id.is_empty() {
            let id = self.custom.id.clone();
            self.base.leave_lobby(svc, &id, false);
        }
    }

    /// The backend of `via` is gone: its create's ask is let go.
    pub fn forget(&mut self, via: Via) {
        if self.base.via == Some(via) {
            self.custom.creating = None;
            self.custom.listing.out = None;
        }
    }

    pub fn answered(&mut self, svc: &mut NetService, via: Via, ask: Ask, r: &Result<Answer, String>, now: f64) -> bool {
        if self.base.via != Some(via) {
            return false;
        }
        if self.custom.listing.out.as_ref().is_some_and(|(a, _)| *a == ask) {
            let (_, want) = self.custom.listing.out.take().expect("out");
            let gap = match r {
                Ok(_) => {
                    self.custom.listing.has = want;
                    LIST_GAP_MS
                }
                Err(e) => {
                    svc.log(&format!("custom: listing {} {e}", self.custom.id));
                    LIST_RETRY_MS
                }
            };
            self.timers.after(now, gap, HostedDue::List);
            return true;
        }
        if self.custom.creating != Some(ask) {
            return false;
        }
        self.custom.creating = None;
        match r {
            Ok(Answer::Hosted { lobby, host, short }) => self.created(svc, lobby, host, short.as_deref(), now),
            Ok(other) => {
                svc.log(&format!("custom: the hosted lobby's make answered {other:?}"));
                self.base.finish(svc, ClosedWhy::BadAnswer, now);
            }
            Err(e) => self.base.finish(svc, make_why(e), now),
        }
        true
    }

    /// The notes for the page's party since the last call.
    pub fn take_notes(&mut self) -> Vec<PartyNote> {
        std::mem::take(&mut self.notes)
    }

    /// A note of the page's party: its queue over on the other side (a member left the party, or its queue) takes
    /// this side out of the core's queue too, the page told with the party's why. The core tells each member of a
    /// party's ticket itself; the moves of the lobby queue (follows) are none of a hosted queue's.
    pub fn note(&mut self, svc: &mut NetService, n: SessionNote, now: f64) {
        let SessionNote::Out(why) = n else { return };
        let q = &self.queued;
        if !self.queue || self.base.closed || !q.searching || !q.party || q.found {
            return;
        }
        svc.log(&format!("queue: out with the party {why:?}"));
        self.queued.searching = false;
        self.queued.quiet_off = true;
        if let Some(g) = &self.g
            && !g.send(svc, &self.base, r#"{"t":"queue.cancel"}"#)
        {
            return self.end(svc, ClosedWhy::SendFailed, now);
        }
        let why = serde_json::to_string(&why).unwrap_or_default();
        svc.post(self.base.page, NetEvent::Msg(format!(r#"{{"t":"queue.state","on":false,"why":{why}}}"#)));
    }

    pub fn pump(&mut self, svc: &mut NetService, now: f64) {
        if self.path.as_deref().and_then(PathSide::next_due).is_some_and(|t| t <= now) {
            let base = &self.base;
            if let Some(p) = self.path.as_mut() {
                p.pump(svc, base, now);
            }
            self.path_said(svc, now);
        }
        while let Some((_, _, HostedDue::List)) = self.timers.take_due(now) {
            self.custom.listing.busy = false;
            let want = self.custom.listing.want.clone();
            self.list(svc, want);
        }
        loop {
            let lan = self.custom.announcer.as_ref().and_then(LanAnnouncer::next_due).filter(|t| *t <= now);
            let link = self.g.as_ref().and_then(Guesting::next_due).filter(|t| *t <= now);
            match (lan, link) {
                (None, None) => break,
                (Some(t), l) if l.is_none_or(|l| t <= l) => {
                    if let Some(a) = self.custom.announcer.as_mut() {
                        a.pump(t);
                    }
                }
                (_, Some(t)) => {
                    let Some(g) = self.g.as_mut() else { break };
                    let n = g.pump(svc, &self.base, t);
                    self.notes_of(svc, n, t);
                }
                _ => break,
            }
        }
    }

    pub fn next_due(&self) -> Option<f64> {
        self.next_key().map(|k| k.0)
    }

    /// The session's first timer: its time and set order (an announcement's last of its time).
    pub fn next_key(&self) -> Option<(f64, u64)> {
        let lan = self.custom.announcer.as_ref().and_then(LanAnnouncer::next_due).map(|t| (t, u64::MAX));
        let path = self.path.as_deref().and_then(PathSide::next_key);
        [self.g.as_ref().and_then(Guesting::next_key), lan, self.timers.next_key(), path]
            .into_iter()
            .flatten()
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
    }
}

/// The direct host's start for a LAN lobby.
fn start_direct(s: &mut RelayHosted, o: &LanOptions) {
    let d = begin_host(DirectHostOptions {
        key_file: o.direct.key_file.clone(),
        max: CUSTOM_MAX,
        join_within_ms: None,
        port: o.port,
        loopback: o.direct.loopback,
        log: None,
        interfaces: None,
    });
    s.custom.starting = Some((d, o.direct.loopback));
}

/// A queue.join's text with `"rtt"` (whole ms, rounded) as its last key; the text as it is when it is no object.
fn with_rtt(text: &str, ms: f64) -> String {
    match round_ms(ms) {
        Some(n) => with_keys(text, &[("rtt", n.to_string())]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::with_rtt;
    use crate::backend::BackendKind;
    use crate::backend::memory::{MemoryNet, MemoryNetOptions};
    use crate::service::page::PageId;
    use crate::service::path::hello_text;
    use crate::service::session::SessionBase;
    use crate::service::{NetService, ServiceOptions};

    #[test]
    fn a_join_carries_its_round_trip_last() {
        assert_eq!(
            with_rtt(r#"{"t":"queue.join","modes":["1v1"]}"#, 41.6),
            r#"{"t":"queue.join","modes":["1v1"],"rtt":42}"#
        );
        assert_eq!(with_rtt("{}", 3.2), r#"{"rtt":3}"#);
        assert_eq!(with_rtt("[1]", 3.0), "[1]");
        assert_eq!(with_rtt("{\"t\":1}", 1e12), "{\"t\":1}");
    }

    #[test]
    fn a_hello_carries_the_path_keys() {
        let net = MemoryNet::new(MemoryNetOptions::default(), 0.0);
        let mut svc = NetService::new(ServiceOptions::default(), 0.0);
        let base = SessionBase::new(PageId(0), Some(BackendKind::Relay), true, false);
        let hello = r#"{"t":"hello","proto":26,"name":"n","app":"a"}"#;
        assert_eq!(
            hello_text(&svc, &base, hello),
            r#"{"t":"hello","proto":26,"name":"n","app":"a","direct":true}"#,
            "no Epic, no relay round trip"
        );
        svc.start_backend(BackendKind::Epic, Box::new(net.player(Some("e\"1"), BackendKind::Epic)), 0.0);
        let t = hello_text(&svc, &base, hello);
        assert_eq!(t, r#"{"t":"hello","proto":26,"name":"n","app":"a","epic":"e\"1","direct":true}"#);
        let own = r#"{"t":"hello","epic":"x","direct":false}"#;
        assert_eq!(hello_text(&svc, &base, own), own, "the page's keys stay");
        assert_eq!(hello_text(&svc, &base, r#"{"t":"ping"}"#), r#"{"t":"ping"}"#);
    }
}
