//! A hosted session's host side and a joined one's guest side: the
//! host core in this process with the lobbies' members linked to it, and the referee of its matches; or this player
//! a member of another's lobby, linked to its owner's core.
//!
//! The core owns its links and its referee, so what they do inside one of its calls goes through a cell the hosting
//! shares with them: a member's packets are sent at once (the core's order), a member's close is sent its bye at once
//! and taken out of the hosting's lists after the call; the page's own link's texts are handed to the session after
//! the call, in order; a match the referee starts here gets its pump's timer after the call. Since wire 5 a hosting
//! runs on Epic only: a relay hosts its lobbies' cores itself (`relay_host`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use indexmap::{IndexMap, IndexSet};

use mvs_host::{ConnKey, Connection, Core, HostEvent, HostOptions, HostWhy, MatchReferee, RefereeStart, SessionKind};
use mvs_protocol::match_wire::packet_fits;
use mvs_protocol::p2p_frame::{FRAME_BYE, FRAME_OPEN, MessageJoiner, Taken, signal, split_message};
use mvs_protocol::referee::{Referee, SlotNews};

use crate::backend::{Channel, LobbyChange, P2P_MAX_PACKET};
use crate::service::api::ClosedWhy;
use crate::service::page::{TimerId, Timers};
use crate::service::refereed::{Outs, RefOut, Refereed};
use crate::service::session::{SessionBase, Via};
use crate::service::{
    BYE_MS, KNOCK_WAIT_MS, NetService, OPEN_EVERY_MS, OPEN_FOR_MS, REFEREE_PUMP_MS, RELINK_MS, Shared, lock,
};

/// What a hosting tells its session after a call (end, found, listing, and the page's own link).
#[derive(Clone, Debug, PartialEq)]
pub enum HostingEvent {
    /// A text of the core to the page's own link.
    Text(String),
    /// The page's own link closed by the core (the session ends with why).
    LocalClosed(ClosedWhy),
    /// The core ended by itself.
    End(ClosedWhy),
    /// A queue match is found: its humans.
    Found(Vec<String>),
    /// A custom lobby's listing changed (None: private).
    Listing(Option<String>),
}

/// Hosting's own timers.
#[derive(Clone, Debug, PartialEq)]
enum HostingDue {
    /// A member the core closed: kicked from its lobby and its link closed (BYE_MS after its bye).
    Kick(String),
    /// A member never linked: kicked (KNOCK_WAIT_MS).
    Knock(String),
    /// A lobby let go is left (BYE_MS after).
    Leave(Vec<String>),
    /// A refereed match's pump (REFEREE_PUMP_MS).
    Pump(u32),
}

/// The ClosedWhy a core's why stands for: an owner's end carries the session's own why as its JSON.
pub fn closed_of(why: &HostWhy) -> ClosedWhy {
    if let HostWhy::Owner { text } = why
        && let Ok(w) = serde_json::from_str::<ClosedWhy>(text)
    {
        return w;
    }
    ClosedWhy::Host { why: why.clone() }
}

/// The core's why for a session's own: a core's passes as it is, any other as the owner's text (its JSON).
pub fn host_why_of(why: &ClosedWhy) -> HostWhy {
    match why {
        ClosedWhy::Host { why } => why.clone(),
        w => HostWhy::Owner { text: serde_json::to_string(w).unwrap_or_default() },
    }
}

/// What the core's links and referee share with the hosting.
struct Cell {
    /// The clock of the core's call under way (the referee's leaves take it).
    now: f64,
    ended: bool,
    /// The members the core closed in its call, in order, with its why.
    closed: Vec<(String, HostWhy)>,
    /// The page's own link's texts and close.
    local: Vec<HostingEvent>,
    refereed: IndexMap<u32, Refereed>,
    /// The refereed matches started and ended in the call (their pump's timers set and cancelled after it).
    started: Vec<u32>,
    ended_matches: Vec<u32>,
}

type CellRef = Arc<Mutex<Cell>>;

fn cell(c: &CellRef) -> MutexGuard<'_, Cell> {
    c.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A packet on a shared backend; false when it refused it.
fn send_on(b: &Shared, to: &str, ch: Channel, bytes: &[u8]) -> bool {
    lock(b).send(to, ch, bytes)
}

/// A lobby member's link to this process's host core.
struct Member {
    id: String,
    backend: Shared,
    closed: Arc<AtomicBool>,
    cell: CellRef,
}

impl Connection for Member {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        ""
    }
    fn send(&mut self, text: &str) -> Result<(), String> {
        if self.closed.load(Ordering::SeqCst) {
            return Ok(());
        }
        // (a refused packet is the link's fault: the core closes it, 'link error')
        for p in split_message(text.as_bytes(), P2P_MAX_PACKET) {
            if !send_on(&self.backend, &self.id, Channel::Messages, &p) {
                return Err("EOS refused the packet".to_string());
            }
        }
        Ok(())
    }
    fn close(&mut self, why: HostWhy) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // (the bye now, in the core's order; the hosting takes the member out after the core's call)
        let text = serde_json::to_string(&closed_of(&why)).unwrap_or_default();
        send_on(&self.backend, &self.id, Channel::Messages, &signal(FRAME_BYE, text.as_bytes()));
        cell(&self.cell).closed.push((self.id.clone(), why));
    }
}

/// The page's own link to the core.
struct Local {
    id: String,
    closed: bool,
    cell: CellRef,
}

impl Connection for Local {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        ""
    }
    fn send(&mut self, text: &str) -> Result<(), String> {
        cell(&self.cell).local.push(HostingEvent::Text(text.to_string()));
        Ok(())
    }
    fn close(&mut self, why: HostWhy) {
        if self.closed {
            return;
        }
        self.closed = true;
        cell(&self.cell).local.push(HostingEvent::LocalClosed(closed_of(&why)));
    }
}

/// The referee of the core's matches: this process's.
struct HostReferee {
    me: String,
    cell: CellRef,
}

impl HostReferee {
    fn end_match(&mut self, match_id: u32) {
        let mut c = cell(&self.cell);
        if c.refereed.shift_remove(&match_id).is_some() {
            c.ended_matches.push(match_id);
        }
    }
}

impl MatchReferee for HostReferee {
    fn start(&mut self, m: &RefereeStart, now: f64) -> String {
        self.end_match(m.match_id);
        if cell(&self.cell).ended {
            return self.me.clone();
        }
        let _ = now;
        let slots: Vec<usize> = m.humans.iter().map(|h| h.slot as usize).collect();
        let r = Refereed {
            referee: Referee::new(&slots, m.tick0),
            ids: m.humans.iter().map(|h| (h.slot, h.id.clone())).collect(),
            slots: m.humans.iter().map(|h| (h.id.clone(), h.slot)).collect(),
        };
        let mut c = cell(&self.cell);
        c.refereed.insert(m.match_id, r);
        c.started.push(m.match_id);
        self.me.clone()
    }

    fn left(&mut self, match_id: u32, slot: u32) {
        let mut c = cell(&self.cell);
        let now = c.now;
        let Some(r) = c.refereed.get_mut(&match_id) else { return };
        let Some(id) = r.ids.shift_remove(&slot) else { return };
        r.slots.shift_remove(&id);
        r.referee.left(slot as usize, now);
    }

    fn end(&mut self, match_id: u32) {
        self.end_match(match_id);
    }
}

/// A member's link as the hosting keeps it.
struct Link {
    key: ConnKey,
    closed: Arc<AtomicBool>,
    joiner: MessageJoiner,
}

/// The host side of a session.
pub struct Hosting {
    pub core: Core,
    /// Each lobby's members as the backend last told them (this player among them).
    pub lobbies: IndexMap<String, IndexSet<String>>,
    /// Who may be in the lobbies (None: anyone): from a found queue match on, its humans.
    allow: Option<IndexSet<String>>,
    links: IndexMap<String, Link>,
    /// The members whose link the core closed, not linked again while they stay in the lobby.
    done: IndexSet<String>,
    /// The members not linked yet (KNOCK_WAIT_MS).
    waiting: IndexMap<String, TimerId>,
    /// The members whose link ended by itself, by when: not linked again within RELINK_MS.
    left_at: IndexMap<String, f64>,
    timers: Timers<HostingDue>,
    local: Option<ConnKey>,
    ended: bool,
    me: String,
    via: Via,
    backend: Shared,
    cell: CellRef,
    events: Vec<HostingEvent>,
}

impl Hosting {
    /// A host core of `kind` for the session (its `via` must stand), this player its host. `lobby_id`: a custom
    /// lobby's code.
    pub fn new(
        svc: &mut NetService,
        base: &SessionBase,
        kind: SessionKind,
        lobby_id: Option<String>,
        now: f64,
    ) -> Hosting {
        let via = base.via.expect("a hosting's session stands");
        let backend = svc.backend(via).expect("a hosting's backend stands");
        let me = base.me.clone();
        let cell = Arc::new(Mutex::new(Cell {
            now,
            ended: false,
            closed: Vec::new(),
            local: Vec::new(),
            refereed: IndexMap::new(),
            started: Vec::new(),
            ended_matches: Vec::new(),
        }));
        let referee = HostReferee { me: me.clone(), cell: cell.clone() };
        let core = Core::new(
            HostOptions {
                kind,
                host: me.clone(),
                name: None,
                lobby_id,
                times: svc.host_times(),
                referee: Some(Box::new(referee)),
                rand: svc.host_rand(),
                log: svc.host_log(),
                paths: false,
            },
            now,
        );
        Hosting {
            core,
            lobbies: IndexMap::new(),
            allow: None,
            links: IndexMap::new(),
            done: IndexSet::new(),
            waiting: IndexMap::new(),
            left_at: IndexMap::new(),
            timers: Timers::default(),
            local: None,
            ended: false,
            me,
            via,
            backend,
            cell,
            events: Vec::new(),
        }
    }

    /// The page's own link connected to the core.
    pub fn connect_local(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) {
        cell(&self.cell).now = now;
        let l = Local { id: self.me.clone(), closed: false, cell: self.cell.clone() };
        self.local = Some(self.core.connect(Box::new(l), now));
        self.after_core(svc, base, now);
    }

    /// A page's message to the core on its own link.
    pub fn local_message(&mut self, svc: &mut NetService, base: &SessionBase, text: &str, now: f64) {
        let Some(k) = self.local else { return };
        cell(&self.cell).now = now;
        self.core.message(k, text, now);
        self.after_core(svc, base, now);
    }

    /// What the hosting told its session since the last call, in order.
    pub fn take_events(&mut self) -> Vec<HostingEvent> {
        std::mem::take(&mut self.events)
    }

    /// The end of a core call: what its links and referee did, its events.
    fn after_core(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) {
        loop {
            let (closed, local, started, ended) = {
                let mut c = cell(&self.cell);
                (
                    std::mem::take(&mut c.closed),
                    std::mem::take(&mut c.local),
                    std::mem::take(&mut c.started),
                    std::mem::take(&mut c.ended_matches),
                )
            };
            let events = self.core.take_events();
            if closed.is_empty() && local.is_empty() && started.is_empty() && ended.is_empty() && events.is_empty() {
                break;
            }
            self.events.extend(local);
            for (id, _) in closed {
                self.member_dropped(svc, base, &id, true, now);
            }
            for m in ended {
                self.timers.cancel_where(|d| *d == HostingDue::Pump(m));
            }
            for m in started {
                self.timers.every(now, REFEREE_PUMP_MS, HostingDue::Pump(m));
            }
            for e in events {
                match e {
                    HostEvent::Kicked(who) => {
                        if self.links.contains_key(&who) {
                            self.close_member(svc, base, &who, &ClosedWhy::Kicked, now);
                        }
                    }
                    HostEvent::End(why) => self.events.push(HostingEvent::End(closed_of(&why))),
                    HostEvent::Found(humans) => self.events.push(HostingEvent::Found(humans)),
                    HostEvent::Listing(t) => self.events.push(HostingEvent::Listing(t)),
                }
            }
        }
    }

    // ---------- the referee of the core's matches ----------

    /// The referee's outs: a packet to the peer of a slot (this player's own page's in process, another's on channel
    /// 1), a silent human to the core.
    fn referee_outs(&mut self, svc: &mut NetService, base: &SessionBase, match_id: u32, outs: Outs, now: f64) {
        for o in outs.0 {
            match o {
                RefOut::Send(slot, bytes) => {
                    if self.ended {
                        continue;
                    }
                    let id = cell(&self.cell).refereed.get(&match_id).and_then(|r| r.ids.get(&(slot as u32)).cloned());
                    let Some(id) = id else { continue };
                    if id == self.me {
                        base.from_referee(svc, &bytes);
                    } else {
                        send_on(&self.backend, &id, Channel::Match, &bytes);
                    }
                }
                RefOut::News(slot, news) => {
                    cell(&self.cell).now = now;
                    self.core_news(match_id, slot as u32, news, now);
                    self.after_core(svc, base, now);
                }
            }
        }
    }

    fn run_referee(&mut self, svc: &mut NetService, base: &SessionBase, match_id: u32, now: f64) {
        let mut outs = Outs::default();
        {
            let mut c = cell(&self.cell);
            let Some(r) = c.refereed.get_mut(&match_id) else { return };
            r.referee.pump(now, &mut outs);
        }
        self.referee_outs(svc, base, match_id, outs, now);
    }

    /// A match packet from that player to the referee of their match here; false when they play in none (a packet
    /// that does not fit is dropped either way).
    pub fn referee_take(
        &mut self,
        svc: &mut NetService,
        base: &SessionBase,
        from: &str,
        bytes: &[u8],
        now: f64,
    ) -> bool {
        let found = {
            let c = cell(&self.cell);
            c.refereed.iter().find_map(|(m, r)| r.slots.get(from).map(|s| (*m, *s)))
        };
        let Some((m, slot)) = found else { return false };
        if !packet_fits(bytes) {
            return true;
        }
        let mut outs = Outs::default();
        if let Some(r) = cell(&self.cell).refereed.get_mut(&m) {
            r.referee.receive(slot as usize, bytes, now, &mut outs);
        }
        self.referee_outs(svc, base, m, outs, now);
        // (what it ruled goes out now, not at the clock's next pump)
        self.run_referee(svc, base, m, now);
        true
    }

    /// The core takes the referee's news of a slot.
    fn core_news(&mut self, match_id: u32, slot: u32, news: SlotNews, now: f64) {
        match news {
            SlotNews::Away => self.core.away(match_id, slot, now),
            SlotNews::Back => self.core.back(match_id, slot, now),
            SlotNews::Gone => self.core.gone(match_id, slot, now),
        }
    }

    // ---------- the lobbies and their members ----------

    pub fn add_lobby(&mut self, id: &str) {
        if !self.ended {
            self.lobbies.insert(id.to_string(), IndexSet::from([self.me.clone()]));
        }
    }

    pub fn has_peer(&self, id: &str) -> bool {
        self.lobbies.values().any(|m| m.contains(id))
    }

    pub fn linked(&self, id: &str) -> bool {
        self.links.contains_key(id)
    }

    /// Another player is in one of the lobbies (but those of `except`).
    pub fn has_guests(&self, except: &[String]) -> bool {
        self.lobbies.values().flatten().any(|id| *id != self.me && !except.contains(id))
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Every player of the lobbies (this player among them).
    pub fn peers(&self) -> impl Iterator<Item = &String> {
        self.lobbies.values().flatten()
    }

    fn lobby_of(&self, id: &str) -> Option<String> {
        self.lobbies.iter().find(|(_, m)| m.contains(id)).map(|(l, _)| l.clone())
    }

    fn kick(&self, lobby: &str, id: &str) {
        lock(&self.backend).kick(lobby, id);
    }

    pub fn message(&mut self, svc: &mut NetService, base: &SessionBase, from: &str, bytes: &[u8], now: f64) {
        if self.ended {
            return;
        }
        let Some(l) = self.links.get_mut(from) else {
            let knock = bytes.len() == 1 && bytes[0] == FRAME_OPEN;
            if !knock || self.done.contains(from) || self.allow.as_ref().is_some_and(|a| !a.contains(from)) {
                return;
            }
            if self.relink_wait(from, now) > 0.0 {
                return;
            }
            if let Some(t) = self.waiting.shift_remove(from) {
                self.timers.cancel(t);
            }
            let closed = Arc::new(AtomicBool::new(false));
            let m = Member {
                id: from.to_string(),
                backend: self.backend.clone(),
                closed: closed.clone(),
                cell: self.cell.clone(),
            };
            // (in the list before the core's call: its first message goes inside it)
            self.links.insert(from.to_string(), Link { key: ConnKey(0), closed, joiner: MessageJoiner::new() });
            svc.log(&format!("member linked {from}"));
            cell(&self.cell).now = now;
            let key = self.core.connect(Box::new(m), now);
            if let Some(l) = self.links.get_mut(from) {
                l.key = key;
            }
            return self.after_core(svc, base, now);
        };
        match l.joiner.take(bytes) {
            Taken::Open | Taken::Part => {}
            Taken::Message(b) => {
                let key = l.key;
                let text = String::from_utf8_lossy(&b).into_owned();
                cell(&self.cell).now = now;
                self.core.message(key, &text, now);
                self.after_core(svc, base, now);
            }
            Taken::Bye(_) => self.member_dropped(svc, base, from, false, now),
            Taken::Bad => self.close_member(svc, base, from, &ClosedWhy::BadGuestPacket, now),
        }
    }

    /// The hosting closes a member's link: told, out of the lobby BYE_MS later.
    fn close_member(&mut self, svc: &mut NetService, base: &SessionBase, id: &str, why: &ClosedWhy, now: f64) {
        let Some(l) = self.links.get(id) else { return };
        if l.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        base.bye(svc, id, why);
        self.member_gone(svc, base, id, true, now);
    }

    /// A member's link the core closed (its bye sent in the call) or that ended by itself.
    fn member_dropped(&mut self, svc: &mut NetService, base: &SessionBase, id: &str, by_core: bool, now: f64) {
        if let Some(l) = self.links.get(id) {
            l.closed.store(true, Ordering::SeqCst);
        }
        self.member_gone(svc, base, id, by_core, now);
    }

    /// A member's link is over: by_core, the member is kicked from its lobby BYE_MS later;
    /// else it may link again after RELINK_MS.
    fn member_gone(&mut self, svc: &mut NetService, base: &SessionBase, id: &str, by_core: bool, now: f64) {
        let Some(l) = self.links.shift_remove(id) else { return };
        if by_core {
            self.done.insert(id.to_string());
            self.timers.after(now, BYE_MS, HostingDue::Kick(id.to_string()));
        } else {
            self.left_at.insert(id.to_string(), now);
            if self.has_peer(id) {
                self.watch(id, now);
            }
        }
        cell(&self.cell).now = now;
        self.core.closed(l.key, now);
        self.after_core(svc, base, now);
    }

    /// The ms before a member that left may link again (0: it may now).
    fn relink_wait(&mut self, id: &str, now: f64) -> f64 {
        self.left_at.retain(|_, at| now - *at < RELINK_MS);
        self.left_at.get(id).map_or(0.0, |at| at + RELINK_MS - now)
    }

    /// A member without a link: kicked from the lobby when it has not linked within KNOCK_WAIT_MS (of the end of its
    /// relink wait).
    fn watch(&mut self, id: &str, now: f64) {
        if self.ended || id == self.me || self.links.contains_key(id) || self.waiting.contains_key(id) {
            return;
        }
        let ms = KNOCK_WAIT_MS + self.relink_wait(id, now);
        let t = self.timers.after(now, ms, HostingDue::Knock(id.to_string()));
        self.waiting.insert(id.to_string(), t);
    }

    /// Who is in one of the lobbies now (a lobby's close is the session's to take).
    pub fn lobby_changed(
        &mut self,
        svc: &mut NetService,
        base: &SessionBase,
        lobby: &str,
        members: &[String],
        now: f64,
    ) {
        if self.ended || !self.lobbies.contains_key(lobby) {
            return;
        }
        let set: IndexSet<String> = members.iter().cloned().collect();
        self.lobbies.insert(lobby.to_string(), set.clone());
        self.prune(svc, base, now);
        for id in &set {
            if *id == self.me || self.done.contains(id) {
                continue;
            }
            if self.allow.as_ref().is_some_and(|a| !a.contains(id)) {
                if !self.links.contains_key(id) {
                    self.kick(lobby, id);
                }
            } else {
                self.watch(id, now);
            }
        }
    }

    /// The links, the closed ones and the waits of players in none of the lobbies any more.
    fn prune(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) {
        let gone: Vec<String> = self.links.keys().filter(|id| !self.has_peer(id)).cloned().collect();
        for id in gone {
            self.member_dropped(svc, base, &id, false, now);
        }
        let done: Vec<String> = self.done.iter().filter(|id| !self.has_peer(id)).cloned().collect();
        for id in done {
            self.done.shift_remove(&id);
        }
        let waits: Vec<String> = self.waiting.keys().filter(|id| !self.has_peer(id)).cloned().collect();
        for id in waits {
            if let Some(t) = self.waiting.shift_remove(&id) {
                self.timers.cancel(t);
            }
        }
    }

    /// The backend closed one of the lobbies under this player: its members are gone.
    pub fn forget_lobby(&mut self, svc: &mut NetService, base: &SessionBase, id: &str, now: f64) {
        if self.lobbies.shift_remove(id).is_none() {
            return;
        }
        self.prune(svc, base, now);
    }

    /// This player lets one of the lobbies go: its linked members told (the core closes their links), the lobby left
    /// BYE_MS later.
    pub fn drop_lobby(&mut self, svc: &mut NetService, base: &SessionBase, id: &str, why: &ClosedWhy, now: f64) {
        let Some(members) = self.lobbies.shift_remove(id) else { return };
        let ids: Vec<String> =
            self.links.keys().filter(|m| members.contains(*m) && !self.has_peer(m)).cloned().collect();
        for m in ids {
            self.close_member(svc, base, &m, why, now);
        }
        self.prune(svc, base, now);
        self.timers.after(now, BYE_MS, HostingDue::Leave(vec![id.to_string()]));
    }

    /// A found queue match: only its humans stay (every other link told and closed, every other member kicked, now
    /// and as they come).
    pub fn seal(&mut self, svc: &mut NetService, base: &SessionBase, humans: &[String], why: &ClosedWhy, now: f64) {
        let allow: IndexSet<String> = humans.iter().cloned().collect();
        let out: Vec<String> = self.links.keys().filter(|m| !allow.contains(*m)).cloned().collect();
        self.allow = Some(allow.clone());
        for m in out {
            self.close_member(svc, base, &m, why, now);
        }
        let kicks: Vec<(String, String)> = self
            .lobbies
            .iter()
            .flat_map(|(l, ms)| ms.iter().map(move |m| (l.clone(), m.clone())))
            .filter(|(_, m)| *m != self.me && !allow.contains(m) && !self.links.contains_key(m))
            .collect();
        for (l, m) in kicks {
            self.kick(&l, &m);
        }
    }

    /// These members' links told and closed.
    pub fn close_links(&mut self, svc: &mut NetService, base: &SessionBase, ids: &[String], why: &ClosedWhy, now: f64) {
        for id in ids {
            self.close_member(svc, base, id, why, now);
        }
    }

    pub fn peer_closed(&mut self, svc: &mut NetService, base: &SessionBase, peer: &str, now: f64) {
        if self.links.contains_key(peer) {
            self.member_dropped(svc, base, peer, false, now);
        }
    }

    /// The core's end (now, or it ended by itself): every member told, the lobbies left BYE_MS later.
    pub fn close(&mut self, svc: &mut NetService, base: &SessionBase, why: &ClosedWhy, now: f64) {
        if self.ended {
            return;
        }
        self.ended = true;
        cell(&self.cell).ended = true;
        if !self.core.ended() {
            cell(&self.cell).now = now;
            self.core.end(host_why_of(why), now);
            self.after_core(svc, base, now);
        }
        // (the core's matches: let go)
        let refereed: Vec<u32> = cell(&self.cell).refereed.drain(..).map(|(m, _)| m).collect();
        for m in refereed {
            self.timers.cancel_where(|d| *d == HostingDue::Pump(m));
        }
        let ids: Vec<String> = self.links.keys().cloned().collect();
        for id in ids {
            self.close_member(svc, base, &id, why, now);
        }
        for (_, t) in self.waiting.drain(..) {
            self.timers.cancel(t);
        }
        let lobbies: Vec<String> = self.lobbies.keys().cloned().collect();
        if !lobbies.is_empty() {
            self.timers.after(now, BYE_MS, HostingDue::Leave(lobbies));
        }
    }

    // ---------- the clock ----------

    /// Runs the hosting's timers and the core's due by `now`, the earliest first (two at once: the hosting's
    /// first).
    pub fn pump(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) {
        loop {
            let mine = self.timers.next_due().filter(|t| *t <= now);
            let core = self.core.next_due().filter(|t| *t <= now);
            match (mine, core) {
                (None, None) => break,
                (Some(a), c) if c.is_none_or(|c| a <= c) => {
                    let Some((at, _, d)) = self.timers.take_due(a) else { break };
                    self.run_due(svc, base, d, at);
                }
                (_, Some(c)) => {
                    cell(&self.cell).now = c;
                    self.core.pump(c);
                    self.after_core(svc, base, c);
                }
                _ => break,
            }
        }
    }

    fn run_due(&mut self, svc: &mut NetService, base: &SessionBase, d: HostingDue, now: f64) {
        match d {
            HostingDue::Kick(id) => {
                if let Some(lobby) = self.lobby_of(&id) {
                    self.kick(&lobby, &id);
                }
                base.close_peer(svc, &id, false);
            }
            HostingDue::Knock(id) => {
                self.waiting.shift_remove(&id);
                let lobby = self.lobby_of(&id);
                if self.ended || self.links.contains_key(&id) {
                    return;
                }
                let Some(lobby) = lobby else { return };
                svc.log(&format!("member never linked: kicked {id}"));
                self.kick(&lobby, &id);
            }
            HostingDue::Leave(ids) => {
                for id in ids {
                    base.leave_lobby(svc, &id, false);
                }
            }
            HostingDue::Pump(m) => self.run_referee(svc, base, m, now),
        }
    }

    pub fn next_due(&self) -> Option<f64> {
        self.next_key().map(|k| k.0)
    }

    /// The first of the hosting's timers and the core's: its time and set order (the core's last of its time).
    pub fn next_key(&self) -> Option<(f64, u64)> {
        let core = self.core.next_due().map(|t| (t, u64::MAX));
        [self.timers.next_key(), core].into_iter().flatten().min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
    }

    /// The hosting's backend way (its session's).
    pub fn via(&self) -> Via {
        self.via
    }
}

/// What a guest side tells its session after a call.
#[derive(Clone, Debug, PartialEq)]
pub enum GuestNote {
    /// One of the owner's messages.
    Text(String),
    /// The owner's side ended the link.
    Lost(ClosedWhy),
}

#[derive(Clone, Debug, PartialEq)]
enum GuestDue {
    Knock,
    Lost(ClosedWhy),
    /// This side's end: the lobby and the link let go.
    Gone,
}

/// The guest side of a session: linked to `host`, the core's id (the lobby's owner on Epic; a
/// relay's host core, `relay_wire::host_id` or QUEUE_HOST, on a relay), in the lobby `owner` owns.
pub struct Guesting {
    pub host: String,
    pub owner: String,
    pub members: IndexSet<String>,
    pub lobby: String,
    /// The link is over (either side ended it).
    pub done: bool,
    joiner: MessageJoiner,
    linked: bool,
    from: f64,
    opener: Option<TimerId>,
    timers: Timers<GuestDue>,
}

/// The why a host's bye carries (its ClosedWhy's JSON; none: it closed the link; another text: as it is).
pub fn bye_why(body: &[u8]) -> ClosedWhy {
    if body.is_empty() {
        return ClosedWhy::HostClosed;
    }
    let text = String::from_utf8_lossy(body);
    serde_json::from_str::<ClosedWhy>(&text).unwrap_or_else(|_| ClosedWhy::Other { text: text.into_owned() })
}

impl Guesting {
    /// The lobby's join (its owner and members): FRAME_OPEN to the host now and every OPEN_EVERY_MS.
    pub fn new(
        svc: &mut NetService,
        base: &SessionBase,
        lobby: &str,
        host: &str,
        owner: &str,
        members: &[String],
        now: f64,
    ) -> Guesting {
        let mut m: IndexSet<String> = members.iter().cloned().collect();
        m.insert(owner.to_string());
        let mut g = Guesting {
            host: host.to_string(),
            owner: owner.to_string(),
            members: m,
            lobby: lobby.to_string(),
            done: false,
            joiner: MessageJoiner::new(),
            linked: false,
            from: now,
            opener: None,
            timers: Timers::default(),
        };
        g.opener = Some(g.timers.every(now, OPEN_EVERY_MS, GuestDue::Knock));
        let notes = g.knock(svc, base, now);
        debug_assert!(notes.is_empty());
        g
    }

    fn knock(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) -> Vec<GuestNote> {
        if self.done || self.linked {
            if let Some(t) = self.opener.take() {
                self.timers.cancel(t);
            }
            return Vec::new();
        }
        if now - self.from >= OPEN_FOR_MS {
            if let Some(t) = self.opener.take() {
                self.timers.cancel(t);
            }
            self.end(svc, base, &ClosedWhy::HostSilent, now);
            return vec![GuestNote::Lost(ClosedWhy::HostSilent)];
        }
        if let Some(via) = base.via {
            svc.send(via, &self.host, Channel::Messages, &signal(FRAME_OPEN, &[]));
        }
        Vec::new()
    }

    /// A text to the owner's core; false when the backend refused it.
    pub fn send(&self, svc: &NetService, base: &SessionBase, text: &str) -> bool {
        !self.done && base.send_text(svc, &self.host, text)
    }

    pub fn message(
        &mut self,
        svc: &mut NetService,
        base: &SessionBase,
        from: &str,
        bytes: &[u8],
        now: f64,
    ) -> Vec<GuestNote> {
        if self.done || from != self.host {
            return Vec::new();
        }
        match self.joiner.take(bytes) {
            Taken::Message(b) => {
                self.linked = true;
                vec![GuestNote::Text(String::from_utf8_lossy(&b).into_owned())]
            }
            Taken::Bye(why) => self.lost(svc, base, bye_why(&why)),
            Taken::Bad => {
                self.end(svc, base, &ClosedWhy::BadPacket, now);
                vec![GuestNote::Lost(ClosedWhy::BadPacket)]
            }
            Taken::Open | Taken::Part => Vec::new(),
        }
    }

    pub fn lobby_changed(&mut self, svc: &mut NetService, base: &SessionBase, c: &LobbyChange, now: f64) {
        let _ = (svc, base);
        if self.done {
            return;
        }
        match c {
            // (BYE_MS for the host's last messages, a lobby.kicked among them)
            LobbyChange::Closed { why, .. } => {
                let w = if why == "kicked" { ClosedWhy::Kicked } else { ClosedWhy::LobbyClosed };
                self.timers.after(now, BYE_MS, GuestDue::Lost(w));
            }
            LobbyChange::Members { members, .. } => {
                self.members = members.iter().cloned().collect();
                if !self.members.contains(&self.owner) {
                    self.timers.after(now, BYE_MS, GuestDue::Lost(ClosedWhy::HostLeft));
                }
            }
        }
    }

    pub fn peer_closed(&mut self, svc: &mut NetService, base: &SessionBase, peer: &str, now: f64) -> Vec<GuestNote> {
        let _ = now;
        // (while it knocks, the owner's process may refuse the link once: the knock goes on)
        if peer == self.host && self.linked {
            return self.lost(svc, base, ClosedWhy::HostLost);
        }
        Vec::new()
    }

    /// The owner's side ended the link: nothing more is sent to it.
    fn lost(&mut self, svc: &mut NetService, base: &SessionBase, why: ClosedWhy) -> Vec<GuestNote> {
        if self.done {
            return Vec::new();
        }
        self.done = true;
        if let Some(t) = self.opener.take() {
            self.timers.cancel(t);
        }
        base.leave_lobby(svc, &self.lobby, false);
        base.close_peer(svc, &self.host, false);
        vec![GuestNote::Lost(why)]
    }

    /// This side ends the link: the owner told by FRAME_BYE, the lobby and the link let go BYE_MS later.
    pub fn end(&mut self, svc: &mut NetService, base: &SessionBase, why: &ClosedWhy, now: f64) {
        if self.done {
            return;
        }
        self.done = true;
        if let Some(t) = self.opener.take() {
            self.timers.cancel(t);
        }
        base.bye(svc, &self.host, why);
        self.timers.after(now, BYE_MS, GuestDue::Gone);
    }

    pub fn pump(&mut self, svc: &mut NetService, base: &SessionBase, now: f64) -> Vec<GuestNote> {
        let mut notes = Vec::new();
        while let Some((at, _, d)) = self.timers.take_due(now) {
            match d {
                GuestDue::Knock => notes.extend(self.knock(svc, base, at)),
                GuestDue::Lost(w) => notes.extend(self.lost(svc, base, w)),
                GuestDue::Gone => {
                    base.leave_lobby(svc, &self.lobby, false);
                    base.close_peer(svc, &self.host, false);
                }
            }
        }
        notes
    }

    pub fn next_due(&self) -> Option<f64> {
        self.timers.next_due()
    }

    pub fn next_key(&self) -> Option<(f64, u64)> {
        self.timers.next_key()
    }
}
