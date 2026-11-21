//! A custom lobby's host and guest sessions. Since wire 5 a host
//! session runs on Epic, or on no backend (a local lobby), its core in this process; a relay's and a LAN host's lobbies are hosted by their relay
//! core (`relay_host`). A guest links to the core wherever it runs.
//!
//! The awaited starts (a direct host's open, a direct code's join, a relay code's link of its own) run on a
//! worker each (pending.rs), polled at the service's pump (`poll_pending`); a backend's answer (a lobby made, joined,
//! listed) comes as the backend's event.
//!
//! Deviation: the lookup of a relay's host name is a blocking call, made at the join.

use mvs_host::SessionKind;
use mvs_protocol::relay_wire::{KnownRelay, host_id, is_host_id};
use serde_json::Value;

use crate::backend::{Answer, Ask, BackendKind, Listed, LobbyChange};
use crate::fail::NetFail;
use crate::lan::direct::{DirectJoinOptions, DirectJoinWhy, DirectLink, begin_join};
use crate::lan::direct_code::direct_text;
use crate::link::relay::key::RelayKey;
use crate::link::relay::{ClientOptions, RelayClient, StartWith, other_key};
use crate::pending::Pending;
use crate::service::api::{AskWhat, ClosedWhy, DirectOptions, JoinWhy, LocalWhy, NetEvent, SessionOn};
use crate::service::hosting::{GuestNote, Guesting, Hosting, HostingEvent};
use crate::service::page::{PageId, Timers};
use crate::service::path::{self, PathSide, hello_text};
use crate::service::session::{SessionBase, Via};
use crate::service::{CUSTOM_BUCKET, CUSTOM_MAX, LIST_GAP_MS, LIST_RETRY_MS, NetService, OwnKind, limited, lock};

/// Why a session ends whose lobby could not be made: Limited for the backend's rate.
pub(crate) fn make_why(e: &str) -> ClosedWhy {
    if limited(e) { ClosedWhy::Limited } else { ClosedWhy::MakeFailed { why: NetFail::of(e) } }
}

/// Why a session ends whose join failed: Limited as it is.
fn join_over(why: JoinWhy) -> ClosedWhy {
    ClosedWhy::JoinFailed { why }
}

/// Why a join's backend refused it, in the player's words where the backend's are a result code;
/// None: its rate (Limited).
fn join_why(e: &str) -> Option<JoinWhy> {
    if limited(e) {
        return None;
    }
    if e.contains("NotFound") {
        return Some(JoinWhy::Gone);
    }
    let l = e.to_ascii_lowercase();
    if l.contains("toomanyplayers") || l.contains("lobbyfull") {
        return Some(JoinWhy::Full);
    }
    Some(JoinWhy::Backend { text: e.to_string() })
}

/// The app's name a relay is told (product and version).
pub(crate) fn app_of(o: &DirectOptions) -> String {
    format!("{} {}", o.product, o.version)
}

/// This player's id in a local lobby (its core's host, its matches' referee): no backend gives one there.
const LOCAL_ME: &str = "local";

/// The listing: the core's latest, the one the backend has, whether an update is out or waiting its gap, the lobby's
/// `at` (its create's time, the browser's order).
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
enum HostDue {
    /// The listing's gap is over.
    List,
}

/// A custom lobby this player hosts.
pub struct HostSession {
    pub base: SessionBase,
    pub h: Option<Hosting>,
    /// The create out (its ask).
    creating: Option<Ask>,
    /// The lobby's id on the backend.
    id: String,
    listing: Listing,
    timers: Timers<HostDue>,
}

impl HostSession {
    fn new(base: SessionBase) -> HostSession {
        HostSession {
            base,
            h: None,
            creating: None,
            id: String::new(),
            listing: Listing::default(),
            timers: Timers::default(),
        }
    }

    /// A custom lobby on a signed-in backend whose lobbies it does not host (Epic): its create asked.
    pub fn open(svc: &mut NetService, page: PageId, kind: BackendKind, now: f64) -> HostSession {
        let mut base = SessionBase::new(page, Some(kind), false, svc.config().allow_direct);
        base.me = svc.me_on(Via::Backend(kind));
        if kind == BackendKind::Epic {
            svc.epic_opened(page, false, base.direct, now);
        }
        let mut s = HostSession::new(base);
        s.create(svc);
        s
    }

    /// A local lobby: the host core in this process with no backend under it, for this page's player and bots alone
    /// (`why`: why no connection hosts it). Open at once, with no code; a backend signing in later does not move it.
    // Deviation: the port's own (the build's custom games are all on its servers): a custom game whatever the
    // connection, as the Epic and LAN hosts' in-process core and referee run it.
    pub fn open_local(svc: &mut NetService, page: PageId, why: LocalWhy, now: f64) -> HostSession {
        let mut base = SessionBase::new(page, None, false, false);
        base.me = LOCAL_ME.to_string();
        base.on = Some(SessionOn::Local { why });
        let mut s = HostSession::new(base);
        let mut h = Hosting::new(svc, &s.base, SessionKind::Custom, None, now);
        svc.log("hosting a local lobby");
        let e = s.base.opened();
        svc.post(page, e);
        h.connect_local(svc, &s.base, now);
        s.h = Some(h);
        s.after(svc, now);
        s
    }

    fn create(&mut self, svc: &mut NetService) {
        let Some(b) = self.base.via.and_then(|v| svc.backend(v)) else { return };
        self.creating = Some(lock(&b).create_lobby(CUSTOM_BUCKET, CUSTOM_MAX, None));
    }

    /// The lobby is made: its code, the host core, the page's own link.
    fn created(&mut self, svc: &mut NetService, id: String, now: f64) {
        let kind = self.base.backend.unwrap_or(BackendKind::Epic);
        let code = match svc.code_on(kind, &id) {
            Ok(c) => c,
            Err(e) => {
                self.base.leave_lobby(svc, &id, false);
                return self.base.finish(svc, e.closed(false), now);
            }
        };
        if self.base.closed {
            return self.base.leave_lobby(svc, &id, false);
        }
        self.base.code = code.clone();
        self.id = id.clone();
        self.listing.at = svc.wall();
        let mut h = Hosting::new(svc, &self.base, SessionKind::Custom, Some(code), now);
        h.add_lobby(&id);
        svc.log(&format!("hosting lobby {id}"));
        let e = self.base.opened();
        svc.post(self.base.page, e);
        h.connect_local(svc, &self.base, now);
        self.h = Some(h);
        self.after(svc, now);
    }

    /// What the hosting told the session.
    fn after(&mut self, svc: &mut NetService, now: f64) {
        loop {
            let Some(events) = self.h.as_mut().map(Hosting::take_events) else { return };
            if events.is_empty() {
                return;
            }
            for e in events {
                match e {
                    HostingEvent::Text(t) => {
                        if !self.base.closed {
                            svc.post(self.base.page, NetEvent::Msg(t));
                        }
                    }
                    HostingEvent::LocalClosed(why) | HostingEvent::End(why) => self.end(svc, why, now),
                    HostingEvent::Found(_) => {}
                    HostingEvent::Listing(t) => self.list(svc, t),
                }
            }
        }
    }

    /// The core's listing goes to the backend: one update at a time, LIST_GAP_MS apart, the latest text winning.
    fn list(&mut self, svc: &mut NetService, text: Option<String>) {
        self.listing.want = text;
        let l = &self.listing;
        if l.busy || self.base.closed || l.want == l.has {
            return;
        }
        let Some(b) = self.base.via.and_then(|v| svc.backend(v)) else { return };
        let want = l.want.clone();
        let listed = want.clone().map(|i| Listed { at: l.at, info: Some(i), code: Some(self.base.code.clone()) });
        let ask = lock(&b).set_listed(&self.id, listed);
        self.listing.busy = true;
        self.listing.out = Some((ask, want));
    }

    pub fn in_lobby(&self, id: &str) -> bool {
        self.h.as_ref().is_some_and(|h| h.lobbies.contains_key(id))
    }

    pub fn has_peer(&self, peer: &str) -> bool {
        self.h.as_ref().is_some_and(|h| h.has_peer(peer))
    }

    pub fn peers(&self) -> Vec<String> {
        let me = &self.base.me;
        self.h.as_ref().map(|h| h.peers().filter(|p| *p != me).cloned().collect()).unwrap_or_default()
    }

    pub fn from_page(&mut self, svc: &mut NetService, text: &str, now: f64) {
        if self.base.closed {
            return;
        }
        if let Some(h) = self.h.as_mut() {
            h.local_message(svc, &self.base, text, now);
        }
        self.after(svc, now);
    }

    pub fn message(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) {
        if self.base.closed {
            return;
        }
        if let Some(h) = self.h.as_mut() {
            h.message(svc, &self.base, from, bytes, now);
        }
        self.after(svc, now);
    }

    pub fn lobby_changed(&mut self, svc: &mut NetService, c: &LobbyChange, now: f64) {
        match c {
            LobbyChange::Closed { why, .. } => self.end(svc, ClosedWhy::LobbyGone { text: why.clone() }, now),
            LobbyChange::Members { lobby, members } => {
                if let Some(h) = self.h.as_mut() {
                    h.lobby_changed(svc, &self.base, lobby, members, now);
                }
                self.after(svc, now);
            }
        }
    }

    pub fn peer_closed(&mut self, svc: &mut NetService, peer: &str, now: f64) {
        if let Some(h) = self.h.as_mut() {
            h.peer_closed(svc, &self.base, peer, now);
        }
        self.after(svc, now);
    }

    pub fn end(&mut self, svc: &mut NetService, why: ClosedWhy, now: f64) {
        if self.base.closed {
            return;
        }
        let core_why =
            if why == ClosedWhy::Left { ClosedWhy::Host { why: mvs_host::HostWhy::HostLeft } } else { why.clone() };
        self.base.finish(svc, why, now);
        if let Some(h) = self.h.as_mut() {
            h.close(svc, &self.base, &core_why, now);
        }
        self.after(svc, now);
    }

    /// The backend of `via` is gone: its asks are let go.
    pub fn forget(&mut self, via: Via) {
        if self.base.via == Some(via) {
            self.creating = None;
            self.listing.out = None;
        }
    }

    pub fn answered(&mut self, svc: &mut NetService, via: Via, ask: Ask, r: &Result<Answer, String>, now: f64) -> bool {
        if self.base.via != Some(via) {
            return false;
        }
        if self.creating == Some(ask) {
            self.creating = None;
            match r {
                Ok(Answer::Created(id)) => self.created(svc, id.clone(), now),
                Ok(other) => {
                    svc.log(&format!("custom: the lobby's make answered {other:?}"));
                    self.base.finish(svc, ClosedWhy::BadAnswer, now)
                }
                Err(e) => self.base.finish(svc, make_why(e), now),
            }
            return true;
        }
        if self.listing.out.as_ref().is_some_and(|(a, _)| *a == ask) {
            let (_, want) = self.listing.out.take().expect("out");
            let gap = match r {
                Ok(_) => {
                    self.listing.has = want;
                    LIST_GAP_MS
                }
                Err(e) => {
                    svc.log(&format!("custom: listing {} {e}", self.id));
                    LIST_RETRY_MS
                }
            };
            self.timers.after(now, gap, HostDue::List);
            return true;
        }
        false
    }

    pub fn pump(&mut self, svc: &mut NetService, now: f64) {
        loop {
            let own = self.timers.next_key().filter(|k| k.0 <= now);
            let host = self.h.as_ref().and_then(Hosting::next_key).filter(|k| k.0 <= now);
            match (own, host) {
                (None, None) => break,
                (Some(a), b) if b.is_none_or(|b| a < b) => {
                    let Some((at, _, HostDue::List)) = self.timers.take_due(a.0) else { break };
                    self.listing.busy = false;
                    let want = self.listing.want.clone();
                    self.list(svc, want);
                    let _ = at;
                }
                (_, Some(b)) => {
                    if let Some(h) = self.h.as_mut() {
                        h.pump(svc, &self.base, b.0);
                    }
                    self.after(svc, b.0);
                }
                _ => break,
            }
        }
    }

    pub fn next_due(&self) -> Option<f64> {
        self.next_key().map(|k| k.0)
    }

    pub fn next_key(&self) -> Option<(f64, u64)> {
        [self.timers.next_key(), self.h.as_ref().and_then(Hosting::next_key)]
            .into_iter()
            .flatten()
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
    }
}

/// How a guest reaches the lobby its code names: on a signed-in backend (its lobby id), on a link of its own to a
/// direct host (the code names the lobby, and the owner must be the host that proved the code's key), or on a link of
/// its own to a relay that is not the config's (asked of the player first when `ask`).
#[derive(Clone, Debug)]
pub enum GuestVia {
    On {
        lobby: String,
    },
    /// A hosted lobby on the release's relay, named by its short code (the relay's answer names its id).
    Short {
        code: String,
    },
    Direct {
        o: DirectOptions,
    },
    Relay {
        address: String,
        o: DirectOptions,
        lobby: String,
        ask: bool,
    },
}

/// A join out: its ask and what it checks of the answer.
struct Joining {
    ask: Ask,
    lobby: String,
    host: String,
    direct: bool,
    /// The join named the lobby by its short code: the answer names its id.
    by_short: bool,
}

/// A custom lobby joined by its code.
pub struct GuestSession {
    pub base: SessionBase,
    pub g: Option<Guesting>,
    /// The code as the page will be told it (canonical).
    code: String,
    joining: Option<Joining>,
    /// A relay code's join waiting for the player's answer (a new relay, or a known one's key changed).
    asking: Option<RelayJoin>,
    /// A link of the session's own under way.
    linking: Option<Linking>,
    /// The lobby's core is one the relay hosts (not on Epic, no LAN direct host): its match may go a path of its
    /// own (`mvs_protocol::path`).
    relay_core: bool,
    /// The match's path while the core measures or a player's app referees (None: the relay's alone).
    pub path: Option<Box<PathSide>>,
}

/// A guest's link under way: a direct code's join, or a relay code's link (its address and lobby).
enum Linking {
    Direct(Pending<Result<DirectLink, DirectJoinWhy>>),
    /// A relay's link of its own, shared with the other joins to that relay meanwhile: the service's ticket on it.
    Relay {
        ticket: u32,
        j: RelayJoin,
    },
}

/// A relay code's join on a link of its own: the relay's address (normalRelay), this install's options, the lobby,
/// and the id of the key the relay must prove (the one it proved when the player said yes to it; None: any, kept once
/// it proves one).
#[derive(Clone)]
struct RelayJoin {
    address: String,
    o: DirectOptions,
    lobby: String,
    pin: Option<String>,
}

impl GuestSession {
    pub fn open(
        svc: &mut NetService,
        page: PageId,
        backend: Option<BackendKind>,
        code: &str,
        via: GuestVia,
        now: f64,
    ) -> GuestSession {
        let mut base = SessionBase::new(page, backend, false, svc.config().allow_direct);
        if let Some(k) = backend {
            base.me = svc.me_on(Via::Backend(k));
            if k == BackendKind::Epic {
                svc.epic_opened(page, false, base.direct, now);
            }
        }
        let mut s = GuestSession {
            base,
            g: None,
            code: code.to_string(),
            joining: None,
            asking: None,
            linking: None,
            relay_core: false,
            path: None,
        };
        s.join(svc, via, now);
        s
    }

    fn fail(&mut self, svc: &mut NetService, why: Option<JoinWhy>, now: f64) {
        let w = why.map_or(ClosedWhy::Limited, join_over);
        self.base.finish(svc, w, now);
    }

    fn join(&mut self, svc: &mut NetService, via: GuestVia, now: f64) {
        match via {
            GuestVia::On { lobby } => self.joined(svc, &lobby, "", false),
            GuestVia::Short { code } => {
                self.joined(svc, &code, "", false);
                if let Some(j) = self.joining.as_mut() {
                    j.by_short = true;
                }
            }
            GuestVia::Relay { address, o, lobby, ask } => {
                // (the config's relay by another name: its sign-in, as a link of its own under the same key would
                // end it)
                let own = svc.config().relay.clone();
                if own.is_some_and(|own| svc.same_relay_at(&address, &own)) {
                    if svc.eos_of(BackendKind::Relay).is_none() {
                        let why = svc.off_why(BackendKind::Relay);
                        return self.fail(svc, Some(JoinWhy::RelayOff { address, why }), now);
                    }
                    self.base.backend = Some(BackendKind::Relay);
                    self.base.via = Some(Via::Backend(BackendKind::Relay));
                    self.base.me = svc.me_on(Via::Backend(BackendKind::Relay));
                    return self.joined(svc, &lobby, "", false);
                }
                if ask {
                    svc.ask(self.base.page, AskWhat::NewRelay { address: address.clone() });
                    self.asking = Some(RelayJoin { address, o, lobby, pin: None });
                    return;
                }
                // (a relay said yes to: the key it proved then, if it was kept)
                let pin = svc.known_relay(&address).and_then(|k| k.id.clone());
                self.relay_link(svc, RelayJoin { address, o, lobby, pin }, now);
            }
            GuestVia::Direct { o } => {
                let client = ClientOptions::new(&app_of(&o), svc.epoch());
                let o = DirectJoinOptions { key_file: o.key_file.clone(), loopback: o.loopback, client, cancel: None };
                self.linking = Some(Linking::Direct(begin_join(self.code.clone(), o)));
            }
        }
    }

    /// A link under way (the service polls it).
    pub fn pending(&self) -> bool {
        self.linking.is_some()
    }

    /// The link's start, once it is over: the session's own link, then the join.
    pub fn poll_pending(&mut self, svc: &mut NetService, now: f64) {
        match self.linking.as_mut() {
            Some(Linking::Direct(p)) => {
                let Some(r) = p.poll() else { return };
                self.linking = None;
                let link = match r {
                    Ok(l) => l,
                    Err(why) => return self.fail(svc, Some(JoinWhy::Direct { why }), now),
                };
                // (a session ended while it joined: the link is shut as it is dropped)
                if self.base.closed {
                    return;
                }
                self.base.on = Some(SessionOn::Lan { at: link.at.clone() });
                let n = svc.add_own(Box::new(link.client), OwnKind::DirectJoin);
                self.base.via = Some(Via::Own(n));
                self.base.release = Some(n);
                self.base.me = svc.me_on(Via::Own(n));
                self.joined(svc, &link.lobby, &link.host, true);
            }
            Some(Linking::Relay { ticket, .. }) => {
                let Some(r) = svc.relay_connect_poll(*ticket) else { return };
                let Some(Linking::Relay { j, .. }) = self.linking.take() else { return };
                let (n, id) = match r {
                    Ok(up) => up,
                    Err(text) => {
                        if let Some(proved) = other_key(&text) {
                            return self.key_changed(svc, j, proved.to_string());
                        }
                        let why = JoinWhy::RelayRefused { address: j.address, text };
                        return self.fail(svc, Some(why), now);
                    }
                };
                if self.base.closed {
                    return svc.release(n);
                }
                self.relay_up(svc, n, id, j);
            }
            None => {}
        }
    }

    fn relay_joined(&mut self, svc: &mut NetService, n: u32, lobby: &str) {
        self.base.on = Some(SessionOn::Relay);
        self.base.via = Some(Via::Own(n));
        self.base.release = Some(n);
        self.base.me = svc.me_on(Via::Own(n));
        self.joined(svc, lobby, "", false);
    }

    /// A relay code's link of its own (one other sessions use or are making, or a new one), then the join. A new link
    /// is pinned to `j.pin`; one shared with another join (a relay takes one sign-in per key) is checked against it
    /// once up.
    fn relay_link(&mut self, svc: &mut NetService, j: RelayJoin, now: f64) {
        if let Some((n, id)) = svc.relay_link_in_use(&j.address) {
            return self.relay_up(svc, n, id, j);
        }
        let ticket = match svc.relay_connect_join(&j.address) {
            Some(t) => t,
            None => {
                let key = match RelayKey::load(&j.o.key_file) {
                    Ok(k) => k,
                    Err(e) => {
                        let why = JoinWhy::RelayRefused { address: j.address, text: e.to_string() };
                        return self.fail(svc, Some(why), now);
                    }
                };
                let mut c = ClientOptions::new(&app_of(&j.o), svc.epoch());
                c.faults = svc.config().relay_faults;
                let x = StartWith { relay_id: j.pin.clone(), ..StartWith::default() };
                let p = RelayClient::begin_connect(j.address.clone(), key, c, x);
                svc.relay_connect_begin(&j.address, p)
            }
        };
        self.linking = Some(Linking::Relay { ticket, j });
    }

    /// A relay code's link is up (`n`, the relay proved the key `id`): the key kept for that relay (the player's yes
    /// to a new relay or to its new key, or one said yes to before its key was kept), then the join; a link shared
    /// with a join that expected another key gives it up and asks.
    fn relay_up(&mut self, svc: &mut NetService, n: u32, id: String, j: RelayJoin) {
        if j.pin.as_ref().is_some_and(|p| *p != id) {
            svc.release(n);
            return self.key_changed(svc, j, id);
        }
        svc.know_relay(KnownRelay { address: j.address.clone(), id: Some(id) });
        self.relay_joined(svc, n, &j.lobby);
    }

    /// The relay said yes to proved another key (`proved`) than the one it proved then: the player is asked, and a
    /// yes joins pinned to the new one.
    fn key_changed(&mut self, svc: &mut NetService, j: RelayJoin, proved: String) {
        if self.base.closed {
            return;
        }
        svc.ask(self.base.page, AskWhat::RelayKeyChanged { address: j.address.clone() });
        self.asking = Some(RelayJoin { pin: Some(proved), ..j });
    }

    /// The player's answer to the relay question (a new relay, or a known one's new key).
    pub fn asked(&mut self, svc: &mut NetService, yes: bool, now: f64) {
        let Some(j) = self.asking.take() else { return };
        if self.base.closed {
            return;
        }
        if !yes {
            return self.fail(svc, Some(JoinWhy::RelayNotTaken { address: j.address }), now);
        }
        self.relay_link(svc, j, now);
    }

    /// The lobby's join asked on the session's way (`host`: the owner a direct code's key proved).
    fn joined(&mut self, svc: &mut NetService, lobby: &str, host: &str, direct: bool) {
        let Some(b) = self.base.via.and_then(|v| svc.backend(v)) else { return };
        let ask = lock(&b).join_lobby(lobby);
        self.joining = Some(Joining { ask, lobby: lobby.to_string(), host: host.to_string(), direct, by_short: false });
    }

    fn join_answered(&mut self, svc: &mut NetService, mut j: Joining, r: &Result<Answer, String>, now: f64) {
        let (owner, members) = match r {
            Ok(Answer::Joined { owner, members, lobby }) => {
                // (a join by a short code: the lobby is its id from here on, the code the page shows the short one)
                if j.by_short {
                    let Some(id) = lobby else {
                        // (a relay of wire 6 names the lobby a short code joined: an answer without it is a bad one)
                        return self.base.finish(svc, ClosedWhy::BadAnswer, now);
                    };
                    j.lobby.clone_from(id);
                }
                (owner.clone(), members.clone())
            }
            Ok(_) => (String::new(), Vec::new()),
            Err(e) => return self.fail(svc, join_why(e), now),
        };
        if self.base.closed {
            return self.base.leave_lobby(svc, &j.lobby, false);
        }
        if owner.is_empty() || owner == self.base.me || (j.direct && owner != j.host) {
            self.base.leave_lobby(svc, &j.lobby, false);
            return self.base.finish(svc, ClosedWhy::NoHost, now);
        }
        self.base.code = self.code.clone();
        let e = self.base.opened();
        svc.post(self.base.page, e);
        // (since wire 5 a relay hosts its custom lobbies' cores, and so does a LAN direct host: the link goes to the
        // lobby's host id; on Epic to its owner, whose process runs the core)
        let host =
            if self.base.via == Some(Via::Backend(BackendKind::Epic)) { owner.clone() } else { host_id(&j.lobby) };
        // (a lobby on the relay, joined on the signed-in relay or a relay link of the session's own)
        self.relay_core = !j.direct && matches!(self.base.via, Some(Via::Backend(BackendKind::Relay) | Via::Own(_)));
        self.g = Some(Guesting::new(svc, &self.base, &j.lobby, &host, &owner, &members, now));
    }

    fn notes(&mut self, svc: &mut NetService, notes: Vec<GuestNote>, now: f64) {
        for n in notes {
            match n {
                GuestNote::Text(t) => {
                    // (a lobby on the relay: its core's path texts are the path's first)
                    let path = self.relay_core && !self.base.closed && path::is_path_text(&t);
                    if path && (self.path_told(svc, &t, now) || self.base.closed) {
                        continue;
                    }
                    svc.post(self.base.page, NetEvent::Msg(t));
                }
                GuestNote::Lost(why) => {
                    // (the match first, as over's setMatch(null): the link is done, so its owner is no peer now)
                    let keep = false;
                    self.base.set_match(svc, None, keep);
                    self.end_path(svc, now);
                    self.base.finish(svc, why, now);
                }
            }
        }
    }

    pub fn in_lobby(&self, id: &str) -> bool {
        self.g.as_ref().is_some_and(|g| !g.done && g.lobby == id)
    }

    pub fn has_peer(&self, peer: &str) -> bool {
        self.g.as_ref().is_some_and(|g| !g.done && (g.members.contains(peer) || g.host == peer))
    }

    pub fn peers(&self) -> Vec<String> {
        match &self.g {
            Some(g) if !g.done => {
                let mut out: Vec<String> = g.members.iter().filter(|m| **m != self.base.me).cloned().collect();
                if !out.contains(&g.host) {
                    out.push(g.host.clone());
                }
                out
            }
            _ => Vec::new(),
        }
    }

    pub fn from_page(&mut self, svc: &mut NetService, text: &str, now: f64) {
        if self.base.closed {
            return;
        }
        let Some(g) = &self.g else { return };
        let text = self.canonical_join(text);
        let text =
            if self.relay_core && text.contains("\"hello\"") { hello_text(svc, &self.base, &text) } else { text };
        if !g.send(svc, &self.base, &text) {
            self.end(svc, ClosedWhy::SendFailed, now);
        }
    }

    /// The page's lobby.join names the code as the player typed it; an Epic host core's lobby id is its canonical
    /// form (direct_text), so the page's join of this code is told in that form. A core the relay hosts (or a LAN
    /// direct host) names its lobby by the relay's lobby id, which the code carries: the join is told that id.
    fn canonical_join(&self, text: &str) -> String {
        if !text.contains("lobby.join") {
            return text.to_string();
        }
        let Ok(Value::Object(mut m)) = serde_json::from_str::<Value>(text) else { return text.to_string() };
        let ok = m.get("t").and_then(Value::as_str) == Some("lobby.join")
            && m.get("lobby").and_then(Value::as_str).is_some_and(|l| direct_text(l) == self.base.code);
        if !ok {
            return text.to_string();
        }
        let id = match &self.g {
            Some(g) if is_host_id(&g.host) => g.lobby.clone(),
            _ => self.base.code.clone(),
        };
        m.insert("lobby".into(), Value::String(id));
        Value::Object(m).to_string()
    }

    pub fn message(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) {
        if self.base.closed {
            return;
        }
        let Some(g) = self.g.as_mut() else { return };
        let n = g.message(svc, &self.base, from, bytes, now);
        self.notes(svc, n, now);
    }

    pub fn lobby_changed(&mut self, svc: &mut NetService, c: &LobbyChange, now: f64) {
        if self.base.closed {
            return;
        }
        if let Some(g) = self.g.as_mut() {
            g.lobby_changed(svc, &self.base, c, now);
        }
    }

    pub fn peer_closed(&mut self, svc: &mut NetService, peer: &str, now: f64) {
        let Some(g) = self.g.as_mut() else { return };
        let n = g.peer_closed(svc, &self.base, peer, now);
        self.notes(svc, n, now);
    }

    pub fn end(&mut self, svc: &mut NetService, why: ClosedWhy, now: f64) {
        if self.base.closed {
            return;
        }
        if let Some(Linking::Relay { ticket, .. }) = self.linking.take() {
            svc.relay_connect_cancel(ticket);
        }
        self.end_path(svc, now);
        self.base.finish(svc, why.clone(), now);
        if let Some(g) = self.g.as_mut() {
            g.end(svc, &self.base, &why, now);
        }
    }

    /// The backend of `via` is gone: its join's ask is let go.
    pub fn forget(&mut self, via: Via) {
        if self.base.via == Some(via) {
            self.joining = None;
        }
    }

    pub fn answered(&mut self, svc: &mut NetService, via: Via, ask: Ask, r: &Result<Answer, String>, now: f64) -> bool {
        if self.base.via != Some(via) || self.joining.as_ref().is_none_or(|j| j.ask != ask) {
            return false;
        }
        let j = self.joining.take().expect("joining");
        self.join_answered(svc, j, r, now);
        true
    }

    pub fn pump(&mut self, svc: &mut NetService, now: f64) {
        if self.path.as_deref().and_then(PathSide::next_due).is_some_and(|t| t <= now) {
            let base = &self.base;
            if let Some(p) = self.path.as_mut() {
                p.pump(svc, base, now);
            }
            self.path_said(svc, now);
        }
        // (a session over still pumps its link: its bye's timers run out)
        let Some(g) = self.g.as_mut() else { return };
        let n = g.pump(svc, &self.base, now);
        self.notes(svc, n, now);
    }

    pub fn next_due(&self) -> Option<f64> {
        self.next_key().map(|k| k.0)
    }

    pub fn next_key(&self) -> Option<(f64, u64)> {
        let path = self.path.as_deref().and_then(PathSide::next_key);
        [self.g.as_ref().and_then(Guesting::next_key), path]
            .into_iter()
            .flatten()
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
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

    /// A core text of the match's path (`path::told`): true when it is the path's alone.
    fn path_told(&mut self, svc: &mut NetService, text: &str, now: f64) -> bool {
        let core = self.g.as_ref().map(|g| g.host.clone()).unwrap_or_default();
        let took = path::told(&mut self.path, svc, &self.base, &core, text, now);
        self.path_said(svc, now);
        took
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
}
