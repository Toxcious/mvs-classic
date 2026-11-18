//! The direct host's own player on its relay core: a player of the core in the listener
//! thread's process, no socket. The host's other players reach the same core through its listener. The links and their
//! refusal are the relay client's: opened by the first packet either way; a packet from a peer with no link is taken
//! when the accept set says so, else dropped and the link closed, at most once a second per peer.
//! The core runs on the listener thread: a call goes there as a command, and what the core tells this player (its
//! messages, its packets, the answer of each call after the messages that call made) comes back as events, in the
//! order the core made them, so the game's sessions never run inside the core.
//! The core hosts the host's lobby's session (wire 5): the own player makes it with `host_lobby` and is its member
//! like the others; the core's host and its referees are the core's own, whose packets are taken from the start (as
//! the relay client takes them).

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use indexmap::{IndexMap, IndexSet};
use mvs_protocol::relay_wire::{Created, Listed, Request, ServerMessage, is_host_id};
use mvs_relay::core::{CorePlayer, Taken};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::backend::{
    Answer, Ask, Backend, BackendEvent, BackendKind, Channel, LobbyChange, P2P_MAX_PACKET, Presence, RelayMode,
};
use crate::fail::{HOST_CLOSED, UNWRITABLE};
use crate::link::relay::backend::{Kind, answer_of};

/// A refused peer's packets close its link again at most this often (the relay client's REFUSE_EVERY_MS).
const REFUSE_EVERY_MS: f64 = 1000.0;

/// A host core's id or one of its referees' (`{host}.m{n}`): the relay core's own, not a player that links.
fn core_own(id: &str) -> bool {
    is_host_id(id)
        || id
            .rsplit_once(".m")
            .is_some_and(|(h, n)| is_host_id(h) && !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}

/// What the game side asks of the listener thread.
pub(crate) enum LCmd {
    /// A lobby call; its answer is an [`LEvent::Answer`] of the tag (none: not answered).
    Request {
        tag: Option<u32>,
        req: Value,
    },
    Send {
        to: String,
        channel: u8,
        bytes: Vec<u8>,
    },
    ClosePeer(String),
    /// The own player out of the core.
    Leave,
    /// The own player out of the core, the listener closed, the thread ended.
    Close,
}

/// What the core tells the own player, and the answers to its calls.
pub(crate) enum LEvent {
    Told(ServerMessage),
    Packet {
        channel: u8,
        from: String,
        bytes: Vec<u8>,
    },
    /// The core let this player go (over a limit, or signed in elsewhere).
    Dropped(String),
    /// The host was closed on purpose: the own player is out without a sign-out.
    Closed,
    Answer {
        tag: u32,
        result: Result<Value, String>,
    },
}

/// The own player as the core reaches it.
pub(crate) struct LocalPlayer {
    pub events: Sender<LEvent>,
}

impl CorePlayer for LocalPlayer {
    fn send(&mut self, m: &ServerMessage) -> Taken {
        let _ = self.events.send(LEvent::Told(m.clone()));
        Taken::Yes
    }

    fn packet(&mut self, channel: u8, from: &str, bytes: &[u8]) -> Taken {
        let _ = self.events.send(LEvent::Packet { channel, from: from.to_owned(), bytes: bytes.to_vec() });
        Taken::Yes
    }

    fn refuse(&mut self, why: &str) {
        let _ = self.events.send(LEvent::Dropped(why.to_owned()));
    }
}

/// The host's own player: a [`Backend`] over the listener thread's core.
pub struct LocalBackend {
    me: String,
    cmds: UnboundedSender<LCmd>,
    events: Receiver<LEvent>,
    gone: bool,
    links: IndexSet<String>,
    refused_at: IndexMap<String, f64>,
    /// Who may open a link (None: nobody, as a player with no accept function).
    accept: Option<IndexSet<String>>,
    next_tag: u32,
    asked: HashMap<u32, Kind>,
    /// The calls of `host_lobby`: their answer read as `Created` with its host.
    hosting: IndexSet<u32>,
    made: VecDeque<BackendEvent>,
}

impl std::fmt::Debug for LocalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalBackend").field("me", &self.me).field("gone", &self.gone).finish_non_exhaustive()
    }
}

impl LocalBackend {
    pub(crate) fn new(me: String, cmds: UnboundedSender<LCmd>, events: Receiver<LEvent>) -> LocalBackend {
        LocalBackend {
            me,
            cmds,
            events,
            gone: false,
            links: IndexSet::new(),
            refused_at: IndexMap::new(),
            accept: None,
            next_tag: 1,
            asked: HashMap::new(),
            hosting: IndexSet::new(),
            made: VecDeque::new(),
        }
    }

    /// Out of the core (every lobby left, every link closed); a sign-out is made unless `quiet`.
    fn lose(&mut self, why: &str, quiet: bool) {
        if self.gone {
            return;
        }
        self.gone = true;
        self.links.clear();
        let _ = self.cmds.send(LCmd::Leave);
        // (a call not answered yet never will be: answered as refused, in the order asked)
        self.hosting.clear();
        let mut tags: Vec<u32> = self.asked.drain().map(|(tag, _)| tag).collect();
        tags.sort_unstable();
        for tag in tags {
            self.made.push_back(BackendEvent::Answered(Ask(tag), Err(HOST_CLOSED.to_owned())));
        }
        if !quiet {
            self.made.push_back(BackendEvent::SignedOut(why.to_owned()));
        }
    }

    /// A lobby call: the core answers at once; the answer follows the messages the call made.
    fn call(&mut self, req: &Request, kind: Kind) -> Ask {
        let tag = self.next_tag;
        self.next_tag += 1;
        if self.gone {
            self.made.push_back(BackendEvent::Answered(Ask(tag), Err(HOST_CLOSED.to_owned())));
            return Ask(tag);
        }
        let Ok(req) = serde_json::to_value(req) else {
            self.made.push_back(BackendEvent::Answered(Ask(tag), Err(UNWRITABLE.to_owned())));
            return Ask(tag);
        };
        self.asked.insert(tag, kind);
        if self.cmds.send(LCmd::Request { tag: Some(tag), req }).is_err() {
            // (the host's thread has ended: what it told before its end is taken first, so that a close on purpose
            // stays quiet; then this call and every other one waiting are refused)
            while let Ok(e) = self.events.try_recv() {
                if !self.gone
                    && let Some(b) = self.take(e, 0.0)
                {
                    self.made.push_back(b);
                }
            }
            self.lose(HOST_CLOSED, false);
        }
        Ask(tag)
    }

    fn take(&mut self, e: LEvent, now: f64) -> Option<BackendEvent> {
        match e {
            LEvent::Told(ServerMessage::Lobby { lobby, members: Some(members), .. }) => {
                Some(BackendEvent::Lobby(LobbyChange::Members { lobby, members }))
            }
            LEvent::Told(ServerMessage::Lobby { lobby, closed: Some(why), .. }) => {
                Some(BackendEvent::Lobby(LobbyChange::Closed { lobby, why }))
            }
            LEvent::Told(ServerMessage::Invite { from, payload }) => Some(BackendEvent::Invite { from, payload }),
            LEvent::Told(ServerMessage::PeerClosed { id }) => {
                self.links.shift_remove(&id);
                Some(BackendEvent::PeerClosed(id))
            }
            LEvent::Told(ServerMessage::Listing { lobby, info }) => Some(BackendEvent::Listing { lobby, info }),
            LEvent::Told(_) => None,
            LEvent::Packet { channel, from, bytes } => {
                if !self.links.contains(&from) && !core_own(&from) {
                    if !self.accept.as_ref().is_some_and(|a| a.contains(&from)) {
                        if self.refused_at.get(&from).is_none_or(|&last| now - last >= REFUSE_EVERY_MS) {
                            self.refused_at.insert(from.clone(), now);
                            let _ = self.cmds.send(LCmd::ClosePeer(from));
                        }
                        return None;
                    }
                    self.links.insert(from.clone());
                    self.refused_at.shift_remove(&from);
                }
                let channel = if channel == 0 { Channel::Messages } else { Channel::Match };
                Some(BackendEvent::Packet { from, channel, bytes })
            }
            LEvent::Dropped(why) => {
                self.lose(&why, false);
                None
            }
            LEvent::Closed => {
                self.lose(HOST_CLOSED, true);
                None
            }
            LEvent::Answer { tag, result } => {
                let kind = self.asked.remove(&tag)?;
                if self.hosting.shift_remove(&tag) {
                    let r = result.and_then(|v| {
                        let c: Created = serde_json::from_value(v)
                            .map_err(|_| "the core's answer to create is not one".to_owned())?;
                        match c.host {
                            Some(host) if is_host_id(&host) => {
                                Ok(Answer::Hosted { lobby: c.lobby, host, short: c.short })
                            }
                            _ => Err("the core hosts no session for the lobby".to_owned()),
                        }
                    });
                    return Some(BackendEvent::Answered(Ask(tag), r));
                }
                let r = result.and_then(|v| answer_of(kind, &v));
                Some(BackendEvent::Answered(Ask(tag), r))
            }
        }
    }
}

impl Backend for LocalBackend {
    fn me(&self) -> &str {
        &self.me
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Lan
    }

    fn create_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Ask {
        self.call(&Request::Create { bucket: bucket.to_owned(), max, listed, host: false }, Kind::Created)
    }

    fn host_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Option<Ask> {
        let ask = self.call(&Request::Create { bucket: bucket.to_owned(), max, listed, host: true }, Kind::Created);
        if self.asked.contains_key(&ask.0) {
            self.hosting.insert(ask.0);
        }
        Some(ask)
    }

    fn search_lobbies(&mut self, bucket: &str, limit: u32, full: bool) -> Ask {
        self.call(&Request::Search { bucket: bucket.to_owned(), limit, full: full.then_some(true) }, Kind::Found)
    }

    fn set_listed(&mut self, id: &str, listed: Option<Listed>) -> Ask {
        self.call(&Request::List { lobby: id.to_owned(), listed }, Kind::Done)
    }

    fn join_lobby(&mut self, id: &str) -> Ask {
        self.call(&Request::Join { lobby: id.to_owned() }, Kind::Joined)
    }

    fn leave_lobby(&mut self, id: &str) -> Ask {
        if self.gone {
            let tag = self.next_tag;
            self.next_tag += 1;
            self.made.push_back(BackendEvent::Answered(Ask(tag), Ok(Answer::Done)));
            return Ask(tag);
        }
        self.call(&Request::Leave { lobby: id.to_owned() }, Kind::Done)
    }

    fn kick(&mut self, lobby: &str, member: &str) {
        if self.gone {
            return;
        }
        let req = Request::Kick { lobby: lobby.to_owned(), member: member.to_owned() };
        if let Ok(req) = serde_json::to_value(&req) {
            let _ = self.cmds.send(LCmd::Request { tag: None, req });
        }
    }

    fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool {
        if self.gone || to.is_empty() || to == self.me || bytes.len() > P2P_MAX_PACKET {
            return false;
        }
        self.links.insert(to.to_owned());
        let channel = match channel {
            Channel::Messages => 0,
            Channel::Match => 1,
        };
        // (the core may drop this player for it, over a limit: the drop's sign-out follows)
        let _ = self.cmds.send(LCmd::Send { to: to.to_owned(), channel, bytes: bytes.to_vec() });
        !self.gone
    }

    fn close_peer(&mut self, peer: &str) {
        if self.gone {
            return;
        }
        self.links.shift_remove(peer);
        let _ = self.cmds.send(LCmd::ClosePeer(peer.to_owned()));
    }

    fn relayed(&self, peer: &str) -> Option<bool> {
        self.links.contains(peer).then_some(false)
    }

    fn linked_peers(&self) -> Vec<String> {
        self.links.iter().cloned().collect()
    }

    fn set_relay_mode(&mut self, _mode: RelayMode) -> Result<(), String> {
        // (a LAN link is direct)
        Ok(())
    }

    fn set_accept(&mut self, ids: IndexSet<String>) {
        self.accept = Some(ids);
    }

    fn invite(&mut self, to: &str, payload: &str) -> Ask {
        self.call(&Request::Invite { to: to.to_owned(), payload: payload.to_owned() }, Kind::Done)
    }

    fn presence(&mut self) -> Option<&mut dyn Presence> {
        None
    }

    fn poll(&mut self, now: f64) -> Option<BackendEvent> {
        loop {
            if let Some(e) = self.made.pop_front() {
                return Some(e);
            }
            let e = match self.events.try_recv() {
                Ok(e) => e,
                Err(TryRecvError::Empty) => return None,
                // (the listener thread has ended: the host is closed)
                Err(TryRecvError::Disconnected) => {
                    if self.gone {
                        return None;
                    }
                    self.lose(HOST_CLOSED, false);
                    continue;
                }
            };
            if self.gone {
                continue;
            }
            if let Some(b) = self.take(e, now) {
                return Some(b);
            }
        }
    }

    fn shutdown(&mut self) -> Ask {
        let tag = self.next_tag;
        self.next_tag += 1;
        self.lose("shut down", true);
        self.made.push_back(BackendEvent::Answered(Ask(tag), Ok(Answer::Done)));
        Ask(tag)
    }
}
