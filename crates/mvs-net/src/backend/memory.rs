//! The backend API in one process, for the tests (app/src/eos/memory.ts): any number of players signed in to one
//! `MemoryNet`, their lobbies and links held there. What a real backend does on its own comes later on the net's
//! clock (a delivery, a lobby's change, a link closed), as the service's polls move it on:
//!   channel 0 (messages)  in order, none lost, at the next turn of the clock
//!   channel 1 (a match's) after `delay_ms` (+ up to `jitter_ms`), each lost at the rate `loss`, so they may overtake
//!   a link                opened by the first send; the other side's accept set decides (refused: the packet is
//!                         dropped and the sender told the link closed), closed by either side's close_peer (the
//!                         other told), or when either signs out; direct when both sides' relay mode allowed it at
//!                         its open, else relayed (Force wins), and it keeps that kind for its life
//!   a lobby               members told of each other's coming and going `lobby_lag_ms` after it; the owner's leave
//!                         destroys it, a kick closes it for the kicked
//!   a search              answered `search_lag_ms` after the call; a listed lobby shows in it `list_delay_ms` after
//!                         its create, an unlisted or full one at once no more
//!   presence              each player's last one, listed by `who` when `listed` (the asker left out, by name); an
//!                         invite to a player whose presence says `invites: false` fails as one to a player who is
//!                         not signed in
//! Unverified: the SDK's own order of these notifications against each other is not known here; the tests take the
//! orders this makes and the lags as the worst cases to cover.
//!
//! The net's clock is the latest `now` a poll or `pump` gave it; a call schedules from it. A poll runs the due
//! deliveries one at a time and returns as soon as its own backend has an event, so the service sees each in turn.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use indexmap::{IndexMap, IndexSet};

use super::{
    Answer, Ask, Backend, BackendEvent, BackendKind, CODE_BYTES, Channel, FoundLobby, INFO_BYTES, Listed, LobbyChange,
    P2P_MAX_PACKET, Presence, RelayMode, RelayPresence, Who,
};
use mvs_protocol::messages::ServerMessage;
use mvs_protocol::p2p_frame::{FRAME_OPEN, split_message};
use mvs_protocol::relay_wire::{QUEUE_HOST, is_host_id};

/// The net's lags, losses and draws.
#[derive(Default)]
pub struct MemoryNetOptions {
    /// Channel 1's one-way time (ms), its spread and the share of packets lost.
    pub delay_ms: f64,
    pub jitter_ms: f64,
    pub loss: f64,
    /// A lobby's change reaches its other members this long after it (ms).
    pub lobby_lag_ms: f64,
    /// A listed lobby is found by a search this long after its create (ms).
    pub list_delay_ms: f64,
    /// A search is answered this long after the call (ms).
    pub search_lag_ms: f64,
    /// Lobby ids as a relay writes them (l and 20 hex digits), so that a relay lobby code holds them.
    pub relay_ids: bool,
    /// A relay's (or LAN's) hosted cores, a hosted lobby's host id and the queue's QUEUE_HOST: Some, each answers a
    /// member's first knock with the first message of a core of that protocol (`server`) and says nothing more; None,
    /// they never answer. `MemoryNet::set_core_proto` changes it (a relay redeployed).
    pub core_proto: Option<u32>,
    /// The loss and jitter draws in [0, 1) (the system's by default).
    pub random: Option<Box<dyn FnMut() -> f64 + Send>>,
}

/// A listed lobby's listing: the net's time from which a search finds it, and its attributes.
#[derive(Clone, Debug, PartialEq)]
pub struct MemoryListing {
    pub from: f64,
    pub at: f64,
    pub info: String,
    pub code: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryLobby {
    pub id: String,
    pub bucket: String,
    pub owner: String,
    pub max: u32,
    pub members: Vec<String>,
    /// None: found by its id alone.
    pub listed: Option<MemoryListing>,
}

/// The packets each channel carried and lost, for the tests' checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sent {
    pub messages: u32,
    pub match_packets: u32,
    pub lost: u32,
    pub refused: u32,
}

/// One backend made by `MemoryNet::player`: its events and its link rules.
struct End {
    events: VecDeque<BackendEvent>,
    accept: IndexSet<String>,
    relay_mode: RelayMode,
    relay_modes: Vec<RelayMode>,
    refuse_relay_mode: bool,
    gone: bool,
}

/// What a delivery does when it is due (`Core`: a hosted core's packet to a member, `core_proto`).
enum Act {
    Members { lobby: String, id: String, members: Vec<String> },
    Closed { id: String, lobby: String, why: String },
    Unlinked { a: String, b: String, tell: String, other: String },
    Linked { a: String, b: String, me: String, other: String, relayed: bool },
    Refused { end: usize, to: String },
    Core { end: usize, from: String, bytes: Vec<u8> },
    Give { from: String, to: String, channel: Channel, bytes: Vec<u8> },
    Search { end: usize, ask: Ask, bucket: String, limit: u32, full: bool },
    Invite { end: usize, from: String, payload: String },
    SignedOut { end: usize, why: String },
}

struct Due {
    at: f64,
    n: u64,
    act: Act,
}

struct NetState {
    o: MemoryNetOptions,
    t: f64,
    /// The signed-in players, by id: their end.
    players: IndexMap<String, usize>,
    ends: Vec<End>,
    lobbies: IndexMap<String, MemoryLobby>,
    /// The open links, by their two ends: whether each is relayed.
    links: IndexMap<String, bool>,
    presences: IndexMap<String, RelayPresence>,
    presence_sets: Vec<(String, RelayPresence)>,
    sent: Sent,
    n: u32,
    due: Vec<Due>,
    due_n: u64,
    /// The hosted cores' members that had their first message (`core_proto`): the member and the core's id.
    core_told: IndexSet<(String, String)>,
}

fn link_key(a: &str, b: &str) -> String {
    if a < b { format!("{a}|{b}") } else { format!("{b}|{a}") }
}

const NOT_CONFIGURED: &str = "EOS_NotConfigured";
const NOT_FOUND: &str = "EOS_NotFound";
const INVALID: &str = "EOS_InvalidParameters";

impl NetState {
    fn later(&mut self, ms: f64, act: Act) {
        let n = self.due_n;
        self.due_n += 1;
        self.due.push(Due { at: self.t + ms.max(0.0), n, act });
    }

    fn push(&mut self, id: &str, e: BackendEvent) {
        if let Some(&end) = self.players.get(id) {
            self.ends[end].events.push_back(e);
        }
    }

    fn random(&mut self) -> f64 {
        match &mut self.o.random {
            Some(r) => r(),
            None => rand::random::<f64>(),
        }
    }

    fn new_lobby_id(&mut self) -> String {
        self.n += 1;
        if self.o.relay_ids { format!("l{:020x}", self.n) } else { format!("lobby{:08x}", self.n) }
    }

    /// Tells a lobby's members (but `skip`) who is in it now.
    fn tell_members(&mut self, lobby: &str, skip: &str) {
        let members = self.lobbies[lobby].members.clone();
        for id in &members {
            if id != skip {
                let act = Act::Members { lobby: lobby.to_owned(), id: id.clone(), members: members.clone() };
                self.later(self.o.lobby_lag_ms, act);
            }
        }
    }

    fn tell_closed(&mut self, id: &str, lobby: &str, why: &str) {
        let act = Act::Closed { id: id.to_owned(), lobby: lobby.to_owned(), why: why.to_owned() };
        self.later(self.o.lobby_lag_ms, act);
    }

    /// Ends a link and tells `tell` (the other end).
    fn drop_link(&mut self, a: &str, b: &str, tell: &str) {
        if self.links.shift_remove(&link_key(a, b)).is_none() {
            return;
        }
        let other = if tell == a { b } else { a };
        let act = Act::Unlinked { a: a.to_owned(), b: b.to_owned(), tell: tell.to_owned(), other: other.to_owned() };
        self.later(0.0, act);
    }

    fn linked(&self, a: &str, b: &str) -> bool {
        self.links.contains_key(&link_key(a, b))
    }

    fn mode(&self, id: &str) -> Option<RelayMode> {
        self.players.get(id).map(|&e| self.ends[e].relay_mode)
    }

    fn open(&mut self, a: &str, b: &str) {
        let relayed = self.mode(a) != Some(RelayMode::Allow) || self.mode(b) != Some(RelayMode::Allow);
        self.links.insert(link_key(a, b), relayed);
        for (me, other) in [(a, b), (b, a)] {
            let act =
                Act::Linked { a: a.to_owned(), b: b.to_owned(), me: me.to_owned(), other: other.to_owned(), relayed };
            self.later(0.0, act);
        }
    }

    /// A lobby's listing: found `list_delay_ms` from now, or from `from` (a listed lobby's new attributes).
    fn listing(&self, l: &Listed, from: Option<f64>) -> Result<MemoryListing, String> {
        if l.info.as_ref().is_some_and(|s| s.len() > INFO_BYTES)
            || l.code.as_ref().is_some_and(|s| s.len() > CODE_BYTES)
        {
            return Err(INVALID.to_owned());
        }
        Ok(MemoryListing {
            from: from.unwrap_or(self.t + self.o.list_delay_ms),
            at: l.at,
            info: l.info.clone().unwrap_or_default(),
            code: l.code.clone().unwrap_or_default(),
        })
    }

    /// The next due delivery by `now`, run (false: none).
    fn step(&mut self, now: f64) -> bool {
        let mut next: Option<usize> = None;
        for (i, d) in self.due.iter().enumerate() {
            if d.at <= now
                && next.is_none_or(|j| d.at < self.due[j].at || (d.at == self.due[j].at && d.n < self.due[j].n))
            {
                next = Some(i);
            }
        }
        let Some(i) = next else {
            self.t = self.t.max(now);
            return false;
        };
        let d = self.due.swap_remove(i);
        self.t = self.t.max(d.at);
        self.run(d.act);
        true
    }

    fn run(&mut self, act: Act) {
        match act {
            Act::Members { lobby, id, members } => {
                let here = self.lobbies.get(&lobby).is_some_and(|l| l.members.contains(&id));
                if here {
                    self.push(&id, BackendEvent::Lobby(LobbyChange::Members { lobby, members }));
                }
            }
            Act::Closed { id, lobby, why } => self.push(&id, BackendEvent::Lobby(LobbyChange::Closed { lobby, why })),
            Act::Unlinked { a, b, tell, other } => {
                self.push(&a, BackendEvent::Link { peer: b.clone(), relayed: None });
                self.push(&b, BackendEvent::Link { peer: a, relayed: None });
                self.push(&tell, BackendEvent::PeerClosed(other));
            }
            Act::Linked { a, b, me, other, relayed } => {
                if self.links.get(&link_key(&a, &b)) == Some(&relayed) {
                    self.push(&me, BackendEvent::Link { peer: other, relayed: Some(relayed) });
                }
            }
            Act::Refused { end, to } => self.ends[end].events.push_back(BackendEvent::PeerClosed(to)),
            Act::Core { end, from, bytes } => {
                self.ends[end].events.push_back(BackendEvent::Packet { from, channel: Channel::Messages, bytes });
            }
            Act::Give { from, to, channel, bytes } => {
                if self.linked(&from, &to) {
                    self.push(&to, BackendEvent::Packet { from, channel, bytes });
                }
            }
            Act::Search { end, ask, bucket, limit, full } => {
                let answer = if self.ends[end].gone {
                    Err(NOT_CONFIGURED.to_owned())
                } else {
                    Ok(self.search(end, &bucket, limit, full))
                };
                self.ends[end].events.push_back(BackendEvent::Answered(ask, answer));
            }
            Act::Invite { end, from, payload } => {
                self.ends[end].events.push_back(BackendEvent::Invite { from, payload })
            }
            Act::SignedOut { end, why } => self.ends[end].events.push_back(BackendEvent::SignedOut(why)),
        }
    }

    fn search(&self, end: usize, bucket: &str, limit: u32, full: bool) -> Answer {
        let me = self.players.iter().find(|(_, e)| **e == end).map(|(id, _)| id.as_str());
        let mut out = Vec::new();
        for l in self.lobbies.values() {
            if out.len() >= limit as usize {
                break;
            }
            let Some(listed) = &l.listed else { continue };
            if l.bucket != bucket
                || listed.from > self.t
                || Some(l.owner.as_str()) == me
                || (!full && l.members.len() >= l.max as usize)
            {
                continue;
            }
            out.push(FoundLobby {
                id: l.id.clone(),
                owner: l.owner.clone(),
                members: u32::try_from(l.members.len()).expect("a lobby's members fit u32"),
                max: l.max,
                at: listed.at,
                info: listed.info.clone(),
                code: listed.code.clone(),
            });
        }
        Answer::Found(out)
    }
}

/// Every player and lobby of one test.
#[derive(Clone)]
pub struct MemoryNet(Arc<Mutex<NetState>>);

impl MemoryNet {
    /// A net whose clock starts at `now`.
    pub fn new(o: MemoryNetOptions, now: f64) -> MemoryNet {
        MemoryNet(Arc::new(Mutex::new(NetState {
            o,
            t: now,
            players: IndexMap::new(),
            ends: Vec::new(),
            lobbies: IndexMap::new(),
            links: IndexMap::new(),
            presences: IndexMap::new(),
            presence_sets: Vec::new(),
            sent: Sent::default(),
            n: 0,
            due: Vec::new(),
            due_n: 0,
            core_told: IndexSet::new(),
        })))
    }

    /// The protocol the hosted cores' first message names from now on (`MemoryNetOptions::core_proto`).
    pub fn set_core_proto(&self, proto: Option<u32>) {
        self.state().o.core_proto = proto;
    }

    fn state(&self) -> MutexGuard<'_, NetState> {
        self.0.lock().expect("the memory net's lock")
    }

    /// A player signed in under `id` (a new one by default), as a backend of `kind`.
    ///
    /// # Panics
    /// When `id` is signed in already.
    pub fn player(&self, id: Option<&str>, kind: BackendKind) -> MemoryBackend {
        let mut s = self.state();
        let id = match id {
            Some(id) => id.to_owned(),
            None => {
                s.n += 1;
                format!("pu{:08x}", s.n)
            }
        };
        assert!(!s.players.contains_key(&id), "memory net: {id} is signed in already");
        let end = s.ends.len();
        s.ends.push(End {
            events: VecDeque::new(),
            accept: IndexSet::new(),
            relay_mode: RelayMode::Force,
            relay_modes: Vec::new(),
            refuse_relay_mode: false,
            gone: false,
        });
        s.players.insert(id.clone(), end);
        MemoryBackend { net: self.clone(), me: id, end, kind, asks: 0 }
    }

    /// Runs every delivery due by `now`, in time order.
    pub fn pump(&self, now: f64) {
        let mut s = self.state();
        while s.step(now) {}
    }

    /// When the next delivery is due (None: none).
    pub fn next_due(&self) -> Option<f64> {
        self.state().due.iter().map(|d| d.at).reduce(f64::min)
    }

    /// The net's clock.
    pub fn now(&self) -> f64 {
        self.state().t
    }

    pub fn lobbies(&self) -> Vec<MemoryLobby> {
        self.state().lobbies.values().cloned().collect()
    }

    /// The open links, by their two ends ("a|b"): whether each is relayed.
    pub fn links(&self) -> Vec<(String, bool)> {
        self.state().links.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }

    /// Each signed-in player's last presence.
    pub fn presences(&self) -> Vec<(String, RelayPresence)> {
        self.state().presences.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Every presence set, in order.
    pub fn presence_sets(&self) -> Vec<(String, RelayPresence)> {
        self.state().presence_sets.clone()
    }

    pub fn sent(&self) -> Sent {
        self.state().sent
    }
}

/// One signed-in player of a `MemoryNet`.
pub struct MemoryBackend {
    net: MemoryNet,
    me: String,
    end: usize,
    kind: BackendKind,
    asks: u32,
}

impl MemoryBackend {
    fn ask(&mut self) -> Ask {
        self.asks += 1;
        Ask(self.asks)
    }

    /// A call's answer, at once (the TS's promise resolved in the call's own turn).
    fn answer(&mut self, s: &mut NetState, r: Result<Answer, String>) -> Ask {
        let ask = self.ask();
        s.ends[self.end].events.push_back(BackendEvent::Answered(ask, r));
        ask
    }

    fn live(&self, s: &NetState) -> Result<(), String> {
        if s.ends[self.end].gone { Err(NOT_CONFIGURED.to_owned()) } else { Ok(()) }
    }

    /// The relay control set, and each change of it.
    pub fn relay_mode(&self) -> RelayMode {
        self.net.state().ends[self.end].relay_mode
    }

    pub fn relay_modes(&self) -> Vec<RelayMode> {
        self.net.state().ends[self.end].relay_modes.clone()
    }

    /// `set_relay_mode` fails from here on, as EOS refusing it.
    pub fn refuse_relay_mode(&mut self, on: bool) {
        self.net.state().ends[self.end].refuse_relay_mode = on;
    }

    /// Shut down or signed out: every call fails.
    pub fn gone(&self) -> bool {
        self.net.state().ends[self.end].gone
    }

    /// The sign-in is lost: the player leaves every lobby and link, and is told.
    pub fn sign_out(&mut self, why: &str) {
        let mut s = self.net.state();
        self.drop_all(&mut s);
        s.later(0.0, Act::SignedOut { end: self.end, why: why.to_owned() });
    }

    fn leave(&self, s: &mut NetState, id: &str) {
        let Some(l) = s.lobbies.get_mut(id) else { return };
        if !l.members.contains(&self.me) {
            return;
        }
        if l.owner == self.me {
            let l = s.lobbies.shift_remove(id).expect("the lobby is there");
            for m in &l.members {
                if *m != self.me {
                    s.tell_closed(m, id, "destroyed");
                }
            }
            return;
        }
        l.members.retain(|m| *m != self.me);
        s.tell_members(id, &self.me);
    }

    fn drop_all(&self, s: &mut NetState) {
        if s.ends[self.end].gone {
            return;
        }
        let mine: Vec<String> =
            s.lobbies.values().filter(|l| l.members.contains(&self.me)).map(|l| l.id.clone()).collect();
        for id in mine {
            self.leave(s, &id);
        }
        let others: Vec<String> = s.players.keys().filter(|id| **id != self.me).cloned().collect();
        for id in others {
            s.drop_link(&self.me, &id, &id);
        }
        s.ends[self.end].gone = true;
        s.players.shift_remove(&self.me);
        s.presences.shift_remove(&self.me);
    }
}

impl Backend for MemoryBackend {
    fn me(&self) -> &str {
        &self.me
    }

    fn kind(&self) -> BackendKind {
        self.kind
    }

    fn create_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Ask {
        let net = self.net.clone();
        let mut s = net.state();
        let r = self.live(&s).and_then(|()| {
            let id = s.new_lobby_id();
            let listed = listed.map(|l| s.listing(&l, None)).transpose()?;
            let l = MemoryLobby {
                id: id.clone(),
                bucket: bucket.to_owned(),
                owner: self.me.clone(),
                max,
                members: vec![self.me.clone()],
                listed,
            };
            s.lobbies.insert(id.clone(), l);
            Ok(Answer::Created(id))
        });
        self.answer(&mut s, r)
    }

    /// A relay's or LAN's player (an Epic one hosts nothing): the lobby made as `create_lobby` makes it, answered with
    /// its host id. Unverified: the net runs no host core, so the lobby's host answers its members' knocks with a
    /// core's first message at the most (`core_proto`).
    fn host_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Option<Ask> {
        if self.kind == BackendKind::Epic {
            return None;
        }
        let ask = self.create_lobby(bucket, max, listed);
        let net = self.net.clone();
        let mut s = net.state();
        let end = &mut s.ends[self.end];
        for e in end.events.iter_mut().rev() {
            if let BackendEvent::Answered(a, r) = e
                && *a == ask
            {
                if let Ok(Answer::Created(id)) = r {
                    let host = mvs_protocol::relay_wire::host_id(id);
                    *r = Ok(Answer::Hosted { lobby: id.clone(), host, short: None });
                }
                break;
            }
        }
        Some(ask)
    }

    fn search_lobbies(&mut self, bucket: &str, limit: u32, full: bool) -> Ask {
        let net = self.net.clone();
        let mut s = net.state();
        if let Err(e) = self.live(&s) {
            return self.answer(&mut s, Err(e));
        }
        let ask = self.ask();
        let lag = s.o.search_lag_ms;
        s.later(lag, Act::Search { end: self.end, ask, bucket: bucket.to_owned(), limit, full });
        ask
    }

    /// Unverified: that a listed lobby's new attributes show at once, and a lobby listed again lags as a new one.
    fn set_listed(&mut self, id: &str, listed: Option<Listed>) -> Ask {
        let net = self.net.clone();
        let mut s = net.state();
        let r = self.live(&s).and_then(|()| {
            let l = s.lobbies.get(id).ok_or(NOT_FOUND.to_owned())?;
            if l.owner != self.me {
                return Err("EOS_Lobby_NotOwner".to_owned());
            }
            let from = l.listed.as_ref().map(|x| x.from);
            let listed = listed.map(|x| s.listing(&x, from)).transpose()?;
            s.lobbies[id].listed = listed;
            Ok(Answer::Done)
        });
        self.answer(&mut s, r)
    }

    fn join_lobby(&mut self, id: &str) -> Ask {
        let net = self.net.clone();
        let mut s = net.state();
        let r = self.live(&s).and_then(|()| {
            let l = s.lobbies.get_mut(id).ok_or(NOT_FOUND.to_owned())?;
            if !l.members.contains(&self.me) {
                if l.members.len() >= l.max as usize {
                    return Err("EOS_Lobby_TooManyPlayers".to_owned());
                }
                l.members.push(self.me.clone());
                s.tell_members(id, &self.me);
            }
            let l = &s.lobbies[id];
            Ok(Answer::Joined { owner: l.owner.clone(), members: l.members.clone(), lobby: None })
        });
        self.answer(&mut s, r)
    }

    fn leave_lobby(&mut self, id: &str) -> Ask {
        let net = self.net.clone();
        let mut s = net.state();
        self.leave(&mut s, id);
        self.answer(&mut s, Ok(Answer::Done))
    }

    fn kick(&mut self, lobby: &str, member: &str) {
        let mut s = self.net.state();
        let Some(l) = s.lobbies.get_mut(lobby) else { return };
        if l.owner != self.me || member == self.me || !l.members.iter().any(|m| m == member) {
            return;
        }
        l.members.retain(|m| m != member);
        s.tell_closed(member, lobby, "kicked");
        s.tell_members(lobby, &self.me);
    }

    fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool {
        let mut s = self.net.state();
        if self.kind != BackendKind::Epic
            && (is_host_id(to) || to == QUEUE_HOST)
            && let Some(proto) = s.o.core_proto
        {
            // (a knock: the core's first message, once per member)
            if bytes == [FRAME_OPEN] && s.core_told.insert((self.me.clone(), to.to_owned())) {
                let text = ServerMessage::Server { proto, name: "memory".to_owned() }.text();
                for p in split_message(text.as_bytes(), P2P_MAX_PACKET) {
                    s.later(0.0, Act::Core { end: self.end, from: to.to_owned(), bytes: p });
                }
            }
            return true;
        }
        let peer = s.players.get(to).copied();
        let Some(peer) = peer else { return false };
        if s.ends[self.end].gone || to == self.me || bytes.len() > P2P_MAX_PACKET {
            return false;
        }
        if !s.linked(&self.me, to) {
            if !s.ends[peer].accept.contains(&self.me) {
                s.sent.refused += 1;
                // (EOS gives up on a request the other side does not accept; the sender hears its link closed)
                s.later(0.0, Act::Refused { end: self.end, to: to.to_owned() });
                return true;
            }
            s.open(&self.me, to);
        }
        let give = Act::Give { from: self.me.clone(), to: to.to_owned(), channel, bytes: bytes.to_vec() };
        if channel == Channel::Match {
            s.sent.match_packets += 1;
            if s.random() < s.o.loss {
                s.sent.lost += 1;
                return true;
            }
            let ms = s.o.delay_ms + s.random() * s.o.jitter_ms;
            s.later(ms, give);
        } else {
            s.sent.messages += 1;
            s.later(0.0, give);
        }
        true
    }

    fn close_peer(&mut self, peer: &str) {
        self.net.state().drop_link(&self.me, peer, peer);
    }

    fn relayed(&self, peer: &str) -> Option<bool> {
        self.net.state().links.get(&link_key(&self.me, peer)).copied()
    }

    fn linked_peers(&self) -> Vec<String> {
        let s = self.net.state();
        s.players.keys().filter(|id| **id != self.me && s.linked(&self.me, id)).cloned().collect()
    }

    fn set_relay_mode(&mut self, mode: RelayMode) -> Result<(), String> {
        let mut s = self.net.state();
        let e = &mut s.ends[self.end];
        if e.refuse_relay_mode {
            return Err("EOS relay setting: EOS_Disabled".to_owned());
        }
        if mode != e.relay_mode {
            e.relay_mode = mode;
            e.relay_modes.push(mode);
        }
        Ok(())
    }

    fn set_accept(&mut self, ids: IndexSet<String>) {
        self.net.state().ends[self.end].accept = ids;
    }

    fn invite(&mut self, to: &str, payload: &str) -> Ask {
        let net = self.net.clone();
        let mut s = net.state();
        let r = self.live(&s).and_then(|()| {
            let end = s.players.get(to).copied();
            let refused = s.presences.get(to).is_some_and(|p| !p.invites);
            let Some(end) = end.filter(|_| !refused) else { return Err(NOT_FOUND.to_owned()) };
            if payload.encode_utf16().count() > 500 {
                return Err(INVALID.to_owned());
            }
            s.later(0.0, Act::Invite { end, from: self.me.clone(), payload: payload.to_owned() });
            Ok(Answer::Done)
        });
        self.answer(&mut s, r)
    }

    fn presence(&mut self) -> Option<&mut dyn Presence> {
        Some(self)
    }

    fn poll(&mut self, now: f64) -> Option<BackendEvent> {
        let mut s = self.net.state();
        loop {
            if let Some(e) = s.ends[self.end].events.pop_front() {
                return Some(e);
            }
            if !s.step(now) {
                return None;
            }
        }
    }

    fn shutdown(&mut self) -> Ask {
        let net = self.net.clone();
        let mut s = net.state();
        self.drop_all(&mut s);
        self.answer(&mut s, Ok(Answer::Done))
    }
}

impl Presence for MemoryBackend {
    fn set(&mut self, p: RelayPresence) -> Ask {
        let net = self.net.clone();
        let mut s = net.state();
        let r = self.live(&s).map(|()| {
            s.presences.insert(self.me.clone(), p.clone());
            s.presence_sets.push((self.me.clone(), p));
            Answer::Done
        });
        self.answer(&mut s, r)
    }

    fn who(&mut self, limit: u32) -> Ask {
        let net = self.net.clone();
        let mut s = net.state();
        let r = self.live(&s).map(|()| {
            let mut out: Vec<Who> = s
                .presences
                .iter()
                .filter(|(id, p)| **id != self.me && p.listed)
                .map(|(id, p)| Who {
                    id: id.clone(),
                    name: p.name.clone(),
                    icon: p.icon.clone(),
                    busy: p.busy,
                    status: p.status,
                })
                .collect();
            // (JS's < on strings: by UTF-16 code units)
            out.sort_by(|a, b| a.name.encode_utf16().cmp(b.name.encode_utf16()));
            out.truncate(limit as usize);
            Answer::Who(out)
        });
        self.answer(&mut s, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(b: &mut MemoryBackend, now: f64) -> Vec<BackendEvent> {
        std::iter::from_fn(|| b.poll(now)).collect()
    }

    fn presence(name: &str, invites: bool) -> RelayPresence {
        RelayPresence { name: name.into(), icon: String::new(), busy: false, listed: true, invites, status: None }
    }

    #[test]
    fn lobbies_links_and_packets() {
        let net = MemoryNet::new(
            MemoryNetOptions { lobby_lag_ms: 10.0, list_delay_ms: 2000.0, search_lag_ms: 50.0, ..Default::default() },
            1000.0,
        );
        let mut a = net.player(None, BackendKind::Epic);
        let mut b = net.player(None, BackendKind::Epic);
        assert_eq!((a.me(), b.me()), ("pu00000001", "pu00000002"));

        let ask = a.create_lobby("mvs:custom", 4, Some(Listed { at: 5.0, info: Some("x".into()), code: None }));
        assert_eq!(
            drain(&mut a, 1000.0),
            vec![BackendEvent::Answered(ask, Ok(Answer::Created("lobby00000003".into())))]
        );
        // (not found before its listing's delay)
        let ask = b.search_lobbies("mvs:custom", 10, false);
        assert_eq!(drain(&mut b, 1050.0), vec![BackendEvent::Answered(ask, Ok(Answer::Found(vec![])))]);
        net.pump(3000.0);
        let ask = b.search_lobbies("mvs:custom", 10, false);
        let found = FoundLobby {
            id: "lobby00000003".into(),
            owner: a.me().into(),
            members: 1,
            max: 4,
            at: 5.0,
            info: "x".into(),
            code: String::new(),
        };
        assert_eq!(drain(&mut b, 3100.0), vec![BackendEvent::Answered(ask, Ok(Answer::Found(vec![found])))]);

        let ask = b.join_lobby("lobby00000003");
        let members = vec![a.me().to_owned(), b.me().to_owned()];
        assert_eq!(
            drain(&mut b, 3100.0),
            vec![BackendEvent::Answered(
                ask,
                Ok(Answer::Joined { owner: a.me().into(), members: members.clone(), lobby: None })
            )]
        );
        assert_eq!(
            drain(&mut a, 3110.0),
            vec![BackendEvent::Lobby(LobbyChange::Members { lobby: "lobby00000003".into(), members })]
        );

        // a send to a player who does not accept: dropped, the sender told the link closed
        assert!(b.send(a.me(), Channel::Messages, b"hi"));
        assert_eq!(drain(&mut b, 3110.0), vec![BackendEvent::PeerClosed(a.me().into())]);
        assert_eq!(net.sent().refused, 1);

        a.set_accept([b.me().to_owned()].into_iter().collect());
        assert!(b.send(a.me(), Channel::Messages, b"hi"));
        assert_eq!(b.relayed(a.me()), Some(true));
        let ev = drain(&mut a, 3110.0);
        assert_eq!(
            ev,
            vec![
                BackendEvent::Link { peer: b.me().into(), relayed: Some(true) },
                BackendEvent::Packet { from: b.me().into(), channel: Channel::Messages, bytes: b"hi".to_vec() }
            ]
        );
        assert_eq!(drain(&mut b, 3110.0), vec![BackendEvent::Link { peer: a.me().into(), relayed: Some(true) }]);
        assert_eq!(a.linked_peers(), vec![b.me().to_owned()]);

        // the owner's sign-out destroys the lobby and the link
        a.sign_out("lost");
        let ev = drain(&mut b, 3200.0);
        assert_eq!(
            ev,
            vec![
                BackendEvent::Link { peer: a.me().into(), relayed: None },
                BackendEvent::PeerClosed(a.me().into()),
                BackendEvent::Lobby(LobbyChange::Closed { lobby: "lobby00000003".into(), why: "destroyed".into() }),
            ]
        );
        assert_eq!(drain(&mut a, 3200.0), vec![BackendEvent::SignedOut("lost".into())]);
        let ask = a.create_lobby("x", 2, None);
        assert_eq!(drain(&mut a, 3200.0), vec![BackendEvent::Answered(ask, Err(NOT_CONFIGURED.into()))]);
    }

    #[test]
    fn match_channel_relay_modes_presence_and_invites() {
        let draws = [0.5, 0.25, 0.01];
        let mut at = 0;
        let random = Box::new(move || {
            let r = draws[at % draws.len()];
            at += 1;
            r
        });
        let o =
            MemoryNetOptions { delay_ms: 20.0, jitter_ms: 8.0, loss: 0.1, random: Some(random), ..Default::default() };
        let net = MemoryNet::new(o, 0.0);
        let mut a = net.player(Some("a"), BackendKind::Relay);
        let mut b = net.player(Some("b"), BackendKind::Relay);
        a.set_relay_mode(RelayMode::Allow).unwrap();
        b.set_relay_mode(RelayMode::Allow).unwrap();
        assert_eq!(a.relay_modes(), vec![RelayMode::Allow]);
        b.set_accept(["a".to_owned()].into_iter().collect());
        // draws 0.5 (kept) and 0.25 (2 ms of jitter): due at 22 ms; then 0.01: lost
        assert!(a.send("b", Channel::Match, &[1]));
        assert!(a.send("b", Channel::Match, &[2]));
        assert_eq!(net.sent(), Sent { messages: 0, match_packets: 2, lost: 1, refused: 0 });
        assert_eq!(drain(&mut b, 21.0), vec![BackendEvent::Link { peer: "a".into(), relayed: Some(false) }]);
        assert_eq!(
            drain(&mut b, 22.0),
            vec![BackendEvent::Packet { from: "a".into(), channel: Channel::Match, bytes: vec![1] }]
        );
        a.refuse_relay_mode(true);
        assert!(a.set_relay_mode(RelayMode::Force).is_err());

        b.presence().unwrap().set(presence("Zed", false));
        let mut c = net.player(Some("c"), BackendKind::Relay);
        c.presence().unwrap().set(presence("Amy", true));
        let ask = a.presence().unwrap().who(10);
        let who = |id: &str, name: &str| Who {
            id: id.into(),
            name: name.into(),
            icon: String::new(),
            busy: false,
            status: None,
        };
        assert_eq!(
            drain(&mut a, 22.0).pop(),
            Some(BackendEvent::Answered(ask, Ok(Answer::Who(vec![who("c", "Amy"), who("b", "Zed")]))))
        );

        let ask = a.invite("b", "p");
        assert_eq!(drain(&mut a, 22.0), vec![BackendEvent::Answered(ask, Err(NOT_FOUND.into()))]);
        let ask = a.invite("c", "p");
        assert_eq!(drain(&mut a, 22.0), vec![BackendEvent::Answered(ask, Ok(Answer::Done))]);
        assert_eq!(drain(&mut c, 22.0).pop(), Some(BackendEvent::Invite { from: "a".into(), payload: "p".into() }));
    }
}
