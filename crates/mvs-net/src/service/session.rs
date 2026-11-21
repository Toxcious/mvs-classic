//! A page's session: its base, the same for every kind, and its kinds (a custom lobby's host
//! and guest, the queue's, a relay-hosted core's member). The service keeps a page's session in the page; a call
//! that needs the service takes the session out of its page for the call and puts it back after, unless it is over.

use crate::backend::{Answer, Ask, BackendKind, Channel, LobbyChange, P2P_MAX_PACKET};
use crate::service::api::{ClosedWhy, NetEvent, SessionOn};
use crate::service::custom::{GuestSession, HostSession};
use crate::service::page::PageId;
use crate::service::party::{PartyNote, SessionNote};
use crate::service::path::PathSide;
use crate::service::queue::QueueSession;
use crate::service::relay_host::RelayHosted;
use crate::service::{NetService, RELEASE_MS, Shared, lock};

use mvs_protocol::match_wire::packet_fits;
use mvs_protocol::names::NAME_LIMIT;
use mvs_protocol::p2p_frame::{FRAME_BYE, signal, split_message};

/// What a session's packets go through: a backend of the service, or a link of the session's own (a LAN host's, a
/// join's of a direct code or of another relay), by its number on the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Via {
    Backend(BackendKind),
    Own(u32),
}

/// A match's routing (NetRequest Match): match.go's peers, this page's slot, its referee's id, and the way its
/// packets go: None the session's `via`, Some (Epic) a path's whose referee is a player's app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchRoute {
    pub peers: Vec<String>,
    pub slot: u32,
    pub referee: String,
    pub on: Option<Via>,
}

/// What a session's end leaves to the service: the page's `Closed`, Epic's
/// relay control looked at again (`epic_closed`), and a link of the session's own let go RELEASE_MS later.
#[derive(Clone, Debug, PartialEq)]
pub struct Over {
    pub event: NetEvent,
    pub release: Option<u32>,
}

/// What every session has.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionBase {
    /// The page it is the session of.
    pub page: PageId,
    pub match_route: Option<MatchRoute>,
    pub closed: bool,
    /// The lobby code invites carry ('' none).
    pub code: String,
    /// The backend of the service it is on (None: a link of its own); a relay code's join takes the config relay's
    /// when its relay turns out to be that one.
    pub backend: Option<BackendKind>,
    /// What its packets go through: the backend's, or its own link's once it stands (None before: nothing is sent).
    pub via: Option<Via>,
    /// This player's id on `via`.
    pub me: String,
    /// On Epic: a friend game whose links may be direct (the config allowed it at the session's open; a queue's
    /// never).
    pub direct: bool,
    /// A queue session: its links are never direct.
    pub queue: bool,
    /// A link of the session's own, let go RELEASE_MS after the session's end.
    pub release: Option<u32>,
    /// A start in flight (a link of its own, a question to the player) is cancelled: set by `over`.
    pub cancelled: bool,
    /// The backend `via` named when the service last put the session back: kept so that its last leave and close go to
    /// it after the backend went off.
    pub held: Option<Held>,
    /// What the session is connected through where its backend does not say it all (None: its backend's kind): a
    /// LAN host's or a direct code's address, a relay code's link of its own, a local lobby.
    pub on: Option<SessionOn>,
}

/// A session's backend as it was (SessionBase::held): the same backend when it is the same one.
#[derive(Clone)]
pub struct Held(pub Via, pub Shared);

impl std::fmt::Debug for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Held({:?})", self.0)
    }
}

impl PartialEq for Held {
    fn eq(&self, o: &Held) -> bool {
        self.0 == o.0 && std::sync::Arc::ptr_eq(&self.1, &o.1)
    }
}

impl SessionBase {
    /// A session of `page` on `backend` (None: a link of its own, `via` set once it stands). `allow_direct` the
    /// config's. The caller tells the service of an Epic session (epic_opened) after.
    pub fn new(page: PageId, backend: Option<BackendKind>, queue: bool, allow_direct: bool) -> SessionBase {
        SessionBase {
            page,
            match_route: None,
            closed: false,
            code: String::new(),
            backend,
            via: backend.map(Via::Backend),
            me: String::new(),
            direct: !queue && allow_direct,
            queue,
            release: None,
            cancelled: false,
            held: None,
            on: None,
        }
    }

    /// The backend of `via`: the service's, else the one held (gone off since).
    pub fn eos(&self, svc: &NetService) -> Option<Shared> {
        let via = self.via?;
        svc.backend(via).or_else(|| self.held.as_ref().filter(|h| h.0 == via).map(|h| h.1.clone()))
    }

    /// The session is on a backend of the service, ended when that backend goes off; a session on a link of its own
    /// is not.
    pub fn chosen(&self) -> bool {
        self.backend.is_some()
    }

    /// Marks the session over, once: its match stopped, its start in flight cancelled; what the service does after
    /// (the page's `Closed`, epic_closed, the release) in the answer. None when it was over already.
    pub fn over(&mut self, why: ClosedWhy) -> Option<Over> {
        if self.closed {
            return None;
        }
        self.closed = true;
        self.match_route = None;
        self.cancelled = true;
        Some(Over { event: NetEvent::Closed(why), release: self.release.take() })
    }

    /// The session is open: its code and this player's id in it, to the page.
    pub fn opened(&self) -> NetEvent {
        let on = self.on.clone().unwrap_or(match self.backend {
            Some(BackendKind::Relay) => SessionOn::Relay,
            Some(BackendKind::Epic) => SessionOn::Epic,
            // (a LAN host and a direct code's join set their address)
            _ => SessionOn::Lan { at: String::new() },
        });
        NetEvent::Opened { code: self.code.clone(), me: self.me.clone(), chosen: self.chosen(), on }
    }

    /// Marks the session over and does the service's part: the page told `Closed`, Epic's
    /// relay control looked at again, its own link let go RELEASE_MS later. Its match's referee link is let go by
    /// the caller first (`Session::set_match(None)`).
    pub fn finish(&mut self, svc: &mut NetService, why: ClosedWhy, now: f64) {
        let Some(o) = self.over(why) else { return };
        svc.drop_asks(self.page);
        svc.epic_closed(self.page, self.queue, now);
        svc.post(self.page, o.event);
        if let Some(n) = o.release {
            svc.release_later(n, RELEASE_MS, now);
        }
    }

    /// Leaves a lobby on `via` unless another session uses it, or (`keeps`) this one still does from another place.
    pub fn leave_lobby(&self, svc: &mut NetService, id: &str, keeps: bool) {
        let Some(via) = self.via else { return };
        if id.is_empty() || svc.lobby_in_use(via, id) || (!self.closed && keeps) {
            return;
        }
        // (its answer goes to no one: a failed leave is the backend's to log)
        if let Some(b) = self.eos(svc) {
            lock(&b).leave_lobby(id);
        }
    }

    /// Closes the link to a peer unless another session or a party uses it, or (`keeps`) this one still does.
    pub fn close_peer(&self, svc: &mut NetService, peer: &str, keeps: bool) {
        let Some(via) = self.via else { return };
        if svc.peer_in_use(via, peer) || (!self.closed && keeps) {
            return;
        }
        if let Some(b) = self.eos(svc) {
            lock(&b).close_peer(peer);
        }
    }

    /// The page's match from its `Match` to its `MatchEnd`: the link to the old referee let
    /// go when it is none of the session's peers (`keep`), this player, or another match's referee. A referee on a
    /// way not the session's (a match path's on Epic) keeps its link: the path lets it go at its close.
    pub fn set_match(&mut self, svc: &mut NetService, m: Option<MatchRoute>, keep: bool) {
        let old = self.match_route.take();
        let new = m.as_ref().map(|x| x.referee.clone());
        self.match_route = m;
        let Some(old) = old else { return };
        let Some(via) = self.via else { return };
        if old.on.is_some_and(|on| on != via) {
            return;
        }
        let old = old.referee;
        if Some(&old) == new.as_ref() || old == self.me || keep || svc.referee_in_use(via, &old, self.page) {
            return;
        }
        self.close_peer(svc, &old, false);
    }

    /// A text as channel 0's packets; false when the backend refused one.
    pub fn send_text(&self, svc: &NetService, to: &str, text: &str) -> bool {
        let Some(via) = self.via else { return false };
        split_message(text.as_bytes(), P2P_MAX_PACKET).iter().all(|p| svc.send(via, to, Channel::Messages, p))
    }

    /// FRAME_BYE with its why (the code's JSON) to a peer.
    pub fn bye(&self, svc: &NetService, to: &str, why: &ClosedWhy) {
        let Some(via) = self.via else { return };
        let text = serde_json::to_string(why).unwrap_or_default();
        svc.send(via, to, Channel::Messages, &signal(FRAME_BYE, text.as_bytes()));
    }

    /// The way the match's packets go and this player's id on it: the route's own (a match path's on Epic), else the
    /// session's `via` and `me`. None with no match or no way.
    pub fn match_way(&self, svc: &NetService) -> Option<(Via, String)> {
        let m = self.match_route.as_ref()?;
        match m.on {
            Some(on) if Some(on) != self.via => Some((on, svc.me_on(on))),
            _ => Some((self.via?, self.me.clone())),
        }
    }

    /// A packet of the match's referee to the page, while its match is on.
    pub fn from_referee(&self, svc: &mut NetService, bytes: &[u8]) {
        let Some(m) = &self.match_route else { return };
        if self.closed {
            return;
        }
        let from = m.peers.iter().position(|p| *p == m.referee).map_or(-1, |i| i32::try_from(i).unwrap_or(-1));
        svc.post(self.page, NetEvent::Packets { from, packets: vec![bytes.to_vec()] });
    }

    /// An invite with the session's code.
    pub fn invite(&self, svc: &mut NetService, to: &str, name: &str, now: f64) {
        let on = if !self.code.is_empty() && !self.chosen() {
            [BackendKind::Epic, BackendKind::Relay].into_iter().find(|k| svc.eos_of(*k).is_some()).map(Via::Backend)
        } else {
            None
        };
        let Some(via) = on.or(self.via) else { return };
        if self.code.is_empty() || to.is_empty() || to == svc.me_on(via) {
            return;
        }
        if !svc.may_invite(to, now) {
            return svc.post(self.page, NetEvent::InviteRefused { to: to.to_string() });
        }
        let name: String = {
            let mut n = 0;
            name.chars()
                .take_while(|c| {
                    n += c.len_utf16();
                    n <= NAME_LIMIT
                })
                .collect()
        };
        // (the key order: kind, code, name)
        let s = |x: &str| serde_json::to_string(x).expect("a string");
        let payload = format!(r#"{{"kind":"custom","code":{},"name":{}}}"#, s(&self.code), s(&name));
        svc.send_invite(via, self.page, to, &payload);
    }
}

/// A page's session, by kind.
pub enum Session {
    Host(Box<HostSession>),
    Guest(GuestSession),
    Queue(QueueSession),
    /// A member of a core the relay (or a LAN direct host) runs: its queue's, or a custom lobby's this page leads.
    Hosted(Box<RelayHosted>),
}

impl Session {
    pub fn base(&self) -> &SessionBase {
        match self {
            Session::Host(s) => &s.base,
            Session::Guest(s) => &s.base,
            Session::Queue(s) => &s.base,
            Session::Hosted(s) => &s.base,
        }
    }

    pub fn base_mut(&mut self) -> &mut SessionBase {
        match self {
            Session::Host(s) => &mut s.base,
            Session::Guest(s) => &mut s.base,
            Session::Queue(s) => &mut s.base,
            Session::Hosted(s) => &mut s.base,
        }
    }

    /// A start of the session's own under way (a direct host's, a link's): the service polls it.
    pub fn pending(&self) -> bool {
        match self {
            Session::Guest(s) => s.pending(),
            Session::Hosted(s) => s.pending(),
            Session::Host(_) | Session::Queue(_) => false,
        }
    }

    pub fn poll_pending(&mut self, svc: &mut NetService, now: f64) {
        match self {
            Session::Guest(s) => s.poll_pending(svc, now),
            Session::Hosted(s) => s.poll_pending(svc, now),
            Session::Host(_) | Session::Queue(_) => {}
        }
    }

    /// The session is in that lobby (its changes come to it).
    pub fn in_lobby(&self, id: &str) -> bool {
        match self {
            Session::Host(s) => s.in_lobby(id),
            Session::Guest(s) => s.in_lobby(id),
            Session::Queue(s) => s.in_lobby(id),
            Session::Hosted(s) => s.in_lobby(id),
        }
    }

    /// That player is a member of one of the session's lobbies (it may link, and its messages come to the session).
    pub fn has_peer(&self, peer: &str) -> bool {
        match self {
            Session::Host(s) => s.has_peer(peer),
            Session::Guest(s) => s.has_peer(peer),
            Session::Queue(s) => s.has_peer(peer),
            Session::Hosted(s) => s.has_peer(peer),
        }
    }

    /// The session itself still uses that lobby from another of its places (a queue session's).
    pub fn keeps_lobby(&self, id: &str) -> bool {
        match self {
            Session::Queue(s) => s.keeps_lobby(id),
            _ => false,
        }
    }

    pub fn keeps_peer(&self, peer: &str) -> bool {
        match self {
            Session::Queue(s) => s.keeps_peer(peer),
            _ => false,
        }
    }

    /// A page's message to the session's host core.
    pub fn from_page(&mut self, svc: &mut NetService, text: &str, now: f64) {
        match self {
            Session::Host(s) => s.from_page(svc, text, now),
            Session::Guest(s) => s.from_page(svc, text, now),
            Session::Queue(s) => s.from_page(svc, text, now),
            Session::Hosted(s) => s.from_page(svc, text, now),
        }
    }

    /// A message packet from a peer.
    pub fn message(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) {
        match self {
            Session::Host(s) => s.message(svc, from, bytes, now),
            Session::Guest(s) => s.message(svc, from, bytes, now),
            Session::Queue(s) => s.message(svc, from, bytes, now),
            Session::Hosted(s) => s.message(svc, from, bytes, now),
        }
    }

    pub fn lobby_changed(&mut self, svc: &mut NetService, c: &LobbyChange, now: f64) {
        match self {
            Session::Host(s) => s.lobby_changed(svc, c, now),
            Session::Guest(s) => s.lobby_changed(svc, c, now),
            Session::Queue(s) => s.lobby_changed(svc, c, now),
            Session::Hosted(s) => s.lobby_changed(svc, c, now),
        }
    }

    pub fn peer_closed(&mut self, svc: &mut NetService, peer: &str, now: f64) {
        match self {
            Session::Host(s) => s.peer_closed(svc, peer, now),
            Session::Guest(s) => s.peer_closed(svc, peer, now),
            Session::Queue(s) => s.peer_closed(svc, peer, now),
            Session::Hosted(s) => s.peer_closed(svc, peer, now),
        }
    }

    /// The session is over for this page: `Closed` with why, the lobbies left.
    pub fn end(&mut self, svc: &mut NetService, why: ClosedWhy, now: f64) {
        // (the match first, while the session still has its peers)
        if !self.base().closed {
            self.set_match(svc, None);
        }
        match self {
            Session::Host(s) => s.end(svc, why, now),
            Session::Guest(s) => s.end(svc, why, now),
            Session::Queue(s) => s.end(svc, why, now),
            Session::Hosted(s) => s.end(svc, why, now),
        }
    }

    /// A backend's answer to one of the session's asks on `via`; false: not the session's.
    pub fn answered(&mut self, svc: &mut NetService, via: Via, ask: Ask, r: &Result<Answer, String>, now: f64) -> bool {
        match self {
            Session::Host(s) => s.answered(svc, via, ask, r, now),
            Session::Guest(s) => s.answered(svc, via, ask, r, now),
            Session::Queue(s) => s.answered(svc, via, ask, r, now),
            Session::Hosted(s) => s.answered(svc, via, ask, r, now),
        }
    }

    /// The backend of `via` is gone: what the session asked of it is let go (no answer comes; a new sign-in's asks
    /// count from the start again).
    pub fn forget(&mut self, via: Via) {
        match self {
            Session::Host(s) => s.forget(via),
            Session::Guest(s) => s.forget(via),
            Session::Queue(s) => s.forget(via),
            Session::Hosted(s) => s.forget(via),
        }
    }

    /// Runs the session's timers due by `now` (its host core's among them).
    pub fn pump(&mut self, svc: &mut NetService, now: f64) {
        match self {
            Session::Host(s) => s.pump(svc, now),
            Session::Guest(s) => s.pump(svc, now),
            Session::Queue(s) => s.pump(svc, now),
            Session::Hosted(s) => s.pump(svc, now),
        }
    }

    pub fn next_due(&self) -> Option<f64> {
        match self {
            Session::Host(s) => s.next_due(),
            Session::Guest(s) => s.next_due(),
            Session::Queue(s) => s.next_due(),
            Session::Hosted(s) => s.next_due(),
        }
    }

    /// A match packet from another player: a human's for the referee in this process, or the referee's for the page.
    pub fn match_packet(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) {
        let me = match self.base().match_way(svc) {
            Some((_, me)) => me,
            None if self.base().via.is_some() => self.base().me.clone(),
            None => return,
        };
        if from.is_empty() || from == me {
            return;
        }
        if self.referee_take(svc, from, bytes, now) {
            return;
        }
        let b = self.base();
        if b.match_route.as_ref().is_some_and(|m| m.referee == from) && packet_fits(bytes) {
            b.from_referee(svc, bytes);
        }
    }

    /// One of the page's match packets: to the referee in this process, or to the referee's process.
    pub fn send_match(&mut self, svc: &mut NetService, bytes: &[u8], now: f64) {
        let b = self.base();
        let (Some(m), Some((via, me))) = (b.match_route.as_ref(), b.match_way(svc)) else { return };
        if bytes.len() > P2P_MAX_PACKET {
            return;
        }
        if m.referee != me {
            svc.send(via, &m.referee, Channel::Match, bytes);
        } else {
            self.referee_take(svc, &me, bytes, now);
        }
    }

    /// The referee in this process takes a match packet (its host core's on Epic: a custom host's, a queue's home; a
    /// relay-hosted session's match path's, when the core chose this player's app).
    fn referee_take(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) -> bool {
        match self {
            Session::Host(s) => match s.h.as_mut() {
                Some(h) => h.referee_take(svc, &s.base, from, bytes, now),
                None => false,
            },
            Session::Guest(s) => s.path_referee_take(svc, from, bytes, now),
            Session::Hosted(s) => s.path_referee_take(svc, from, bytes, now),
            Session::Queue(s) => {
                let base = s.base.clone();
                s.hosting_mut().is_some_and(|h| h.referee_take(svc, &base, from, bytes, now))
            }
        }
    }

    /// The page's match from its `Match` to its `MatchEnd` (or the session's end): the link to the old referee let
    /// go when no lobby member and no other match uses it.
    pub fn set_match(&mut self, svc: &mut NetService, m: Option<MatchRoute>) {
        let keep = self.base().match_route.as_ref().is_some_and(|o| self.has_peer(&o.referee));
        self.base_mut().set_match(svc, m, keep);
    }

    /// The match path of a relay-hosted session or of a guest in a lobby the relay hosts (None: the relay's alone).
    pub fn path(&self) -> Option<&PathSide> {
        match self {
            Session::Guest(s) => s.path.as_deref(),
            Session::Hosted(s) => s.path.as_deref(),
            Session::Host(_) | Session::Queue(_) => None,
        }
    }

    pub fn path_mut(&mut self) -> Option<&mut PathSide> {
        match self {
            Session::Guest(s) => s.path.as_deref_mut(),
            Session::Hosted(s) => s.path.as_deref_mut(),
            Session::Host(_) | Session::Queue(_) => None,
        }
    }

    /// A packet on Epic from `from` for the session's match path (`PathSide::packet`): true when the path took it.
    pub fn path_packet(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) -> bool {
        match self {
            Session::Guest(s) => s.path_packet(svc, from, bytes, now),
            Session::Hosted(s) => s.path_packet(svc, from, bytes, now),
            Session::Host(_) | Session::Queue(_) => false,
        }
    }

    /// Every player the session may link to (its peers: the backend's accept set).
    pub fn peers(&self) -> Vec<String> {
        match self {
            Session::Host(s) => s.peers(),
            Session::Guest(s) => s.peers(),
            Session::Queue(s) => s.peers(),
            Session::Hosted(s) => s.peers(),
        }
    }

    /// The session's first timer: its time and set order.
    pub fn next_key(&self) -> Option<(f64, u64)> {
        match self {
            Session::Host(s) => s.next_key(),
            Session::Guest(s) => s.next_key(),
            Session::Queue(s) => s.next_due().map(|t| (t, u64::MAX)),
            Session::Hosted(s) => s.next_key(),
        }
    }

    /// The player's answer to the session's question (a relay code's join's).
    pub fn asked(&mut self, svc: &mut NetService, yes: bool, now: f64) {
        if let Session::Guest(s) = self {
            s.asked(svc, yes, now);
        }
    }

    /// The notes a queue session left for its page's party since the last call.
    pub fn take_notes(&mut self) -> Vec<PartyNote> {
        match self {
            Session::Queue(s) => s.take_notes(),
            Session::Hosted(s) => s.take_notes(),
            _ => Vec::new(),
        }
    }

    /// A note of the page's party for its session (a queue's alone takes them).
    pub fn note(&mut self, svc: &mut NetService, n: SessionNote, now: f64) {
        match self {
            Session::Queue(s) => s.note(svc, n, now),
            Session::Hosted(s) => s.note(svc, n, now),
            _ => {}
        }
    }
}
