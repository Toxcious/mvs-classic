//! A relay client as the net service's backend (app/src/relay/client.ts RelayEos as an app/src/eos/api.ts Eos): each
//! call is a tagged request whose answer comes back, in order with the relay's messages, as a
//! [`BackendEvent::Answered`] from `poll`.

use indexmap::IndexSet;
use mvs_protocol::names::clean_name;
use mvs_protocol::relay_wire::{Listed, PresenceStatus, Request, WHO_LIMIT, Who, is_host_id};
use serde_json::Value;

use super::{Cmd, P2P_MAX_PACKET, RelayClient, RelayEvent, WirePresence};
use crate::backend::{
    Answer, Ask, Backend, BackendEvent, BackendKind, Channel, FoundLobby, LobbyChange, Presence, RelayMode,
};

/// What a call's value is read as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Done,
    Created,
    /// A create that asked the relay to host the lobby: its id and its host core's.
    Hosted,
    Found,
    Joined,
    /// The online list, cut to this many.
    Who(u32),
}

/// A 'who' player's id: `r` and 32 lower-case hex digits.
fn player_id(s: &str) -> bool {
    s.len() == 33 && s.starts_with('r') && s[1..].bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// An icon's asset name as it may be shown: 1 to 96 letters, digits, `_` and `-` (else '').
fn icon_name(s: &str) -> bool {
    (1..=96).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// A search answer's lobby, read field by field (None: it has no id, owner or counts). Text that is not text is ''.
fn found_lobby(x: &Value) -> Option<FoundLobby> {
    let text = |k: &str| x.get(k).and_then(Value::as_str).map(str::to_owned);
    let count = |k: &str| x.get(k).and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a count read as a number")]
    let whole = |n: f64| n as u32;
    Some(FoundLobby {
        id: text("id")?,
        owner: text("owner")?,
        members: whole(count("members")?),
        max: whole(count("max")?),
        at: x.get("at").and_then(Value::as_f64).unwrap_or(0.0),
        info: text("info").unwrap_or_default(),
        code: text("code").unwrap_or_default(),
    })
}

/// A call's value as the answer of its kind (a relay's or a direct host's core's); the online list is the relay's alone.
pub(crate) fn answer_of(kind: Kind, v: &Value) -> Result<Answer, String> {
    let bad = |what: &str| format!("the relay's answer to {what} is not one");
    match kind {
        Kind::Done => Ok(Answer::Done),
        Kind::Created => {
            v.get("lobby").and_then(Value::as_str).map(|l| Answer::Created(l.to_owned())).ok_or_else(|| bad("create"))
        }
        Kind::Hosted => {
            let c: mvs_protocol::relay_wire::Created = serde_json::from_value(v.clone()).map_err(|_| bad("create"))?;
            match c.host {
                Some(host) if is_host_id(&host) => Ok(Answer::Hosted { lobby: c.lobby, host, short: c.short }),
                _ => Err(bad("create")),
            }
        }
        Kind::Found => {
            // (a relay is anyone's: each lobby read on its own, so that one bad lobby costs only itself)
            let list = v.get("lobbies").and_then(Value::as_array).ok_or_else(|| bad("search"))?;
            Ok(Answer::Found(list.iter().filter_map(found_lobby).collect()))
        }
        Kind::Joined => {
            let j: mvs_protocol::relay_wire::Joined = serde_json::from_value(v.clone()).map_err(|_| bad("join"))?;
            Ok(Answer::Joined { owner: j.owner, members: j.members, lobby: j.lobby })
        }
        Kind::Who(_) => Err(bad("who")),
    }
}

impl RelayClient {
    /// A call asked; its answer is read as `kind`.
    fn call(&mut self, req: Request, kind: Kind) -> Ask {
        let tag = self.ask(req);
        self.asked.insert(tag, kind);
        Ask(tag)
    }

    fn read_answer(&self, kind: Kind, v: &Value) -> Result<Answer, String> {
        match kind {
            Kind::Who(limit) => {
                // (a relay is anyone's: each player is read field by field, its name cleaned and checked as an invite's)
                let mut out: Vec<Who> = Vec::new();
                let list = v.get("players").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
                for x in list {
                    let id = x.get("id").and_then(Value::as_str);
                    let name = clean_name(x.get("name").and_then(Value::as_str).unwrap_or(""));
                    let icon = x.get("icon").and_then(Value::as_str);
                    let busy = x.get("busy").and_then(Value::as_bool);
                    if let (Some(id), Some(icon), Some(busy)) = (id, icon, busy)
                        && player_id(id)
                        && !name.is_empty()
                        && !self.refused_words.refuses(&name)
                    {
                        let icon = if icon_name(icon) { icon.to_owned() } else { String::new() };
                        // (an unknown or missing status is none: a relay of before it, or a later build's word)
                        let status = x.get("status").and_then(Value::as_str).and_then(PresenceStatus::of);
                        out.push(Who { id: id.to_owned(), name, icon, busy, status });
                    }
                    if out.len() >= limit.min(WHO_LIMIT) as usize {
                        break;
                    }
                }
                Ok(Answer::Who(out))
            }
            plain => answer_of(plain, v),
        }
    }

    /// One of the relay's events as the backend's (None: not a backend's).
    fn backend_event(&mut self, e: RelayEvent) -> Option<BackendEvent> {
        Some(match e {
            RelayEvent::Lobby { lobby, members: Some(members), .. } => {
                BackendEvent::Lobby(LobbyChange::Members { lobby, members })
            }
            RelayEvent::Lobby { lobby, closed, .. } => {
                BackendEvent::Lobby(LobbyChange::Closed { lobby, why: closed.unwrap_or_default() })
            }
            RelayEvent::Packet { from, channel, bytes } => {
                self.linked.insert(from.clone());
                let channel = if channel == 0 { Channel::Messages } else { Channel::Match };
                BackendEvent::Packet { from, channel, bytes }
            }
            RelayEvent::PeerClosed(p) => {
                self.linked.shift_remove(&p);
                BackendEvent::PeerClosed(p)
            }
            RelayEvent::Invite { from, payload } => BackendEvent::Invite { from, payload },
            // (a match this connection referees itself is its own session's)
            RelayEvent::Slot { .. } => return None,
            RelayEvent::Listing { lobby, info } => BackendEvent::Listing { lobby, info },
            RelayEvent::Answer { tag, result } => {
                let kind = self.asked.remove(&tag)?;
                let r = match result {
                    Ok(v) => {
                        let r = self.read_answer(kind, &v);
                        // (a lobby made for hosting whose answer names no host core is of no use: left at once)
                        if kind == Kind::Hosted
                            && r.is_err()
                            && let Some(lobby) = v.get("lobby").and_then(Value::as_str)
                        {
                            let req = Request::Leave { lobby: lobby.to_owned() };
                            let _ = self.cmds.send(Cmd::Request { req, reply: None });
                        }
                        r
                    }
                    Err(e) => Err(e.to_string()),
                };
                BackendEvent::Answered(Ask(tag), r)
            }
            RelayEvent::SignedOut(why) => {
                self.linked.clear();
                BackendEvent::SignedOut(why)
            }
        })
    }
}

impl Backend for RelayClient {
    fn me(&self) -> &str {
        RelayClient::me(self)
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Relay
    }

    fn relay_proto(&self) -> Option<u32> {
        RelayClient::proto(self)
    }

    fn create_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Ask {
        self.call(Request::Create { bucket: bucket.to_owned(), max, listed, host: false }, Kind::Created)
    }

    fn host_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Option<Ask> {
        Some(self.call(Request::Create { bucket: bucket.to_owned(), max, listed, host: true }, Kind::Hosted))
    }

    fn search_lobbies(&mut self, bucket: &str, limit: u32, full: bool) -> Ask {
        self.call(Request::Search { bucket: bucket.to_owned(), limit, full: full.then_some(true) }, Kind::Found)
    }

    fn set_listed(&mut self, id: &str, listed: Option<Listed>) -> Ask {
        self.call(Request::List { lobby: id.to_owned(), listed }, Kind::Done)
    }

    fn join_lobby(&mut self, id: &str) -> Ask {
        self.call(Request::Join { lobby: id.to_owned() }, Kind::Joined)
    }

    fn leave_lobby(&mut self, id: &str) -> Ask {
        if self.dead.get() {
            // (a connection that has ended has left every lobby: answered done, as the TS's leave)
            let tag = self.next_tag;
            self.next_tag += 1;
            self.made.push_back(BackendEvent::Answered(Ask(tag), Ok(Answer::Done)));
            return Ask(tag);
        }
        self.call(Request::Leave { lobby: id.to_owned() }, Kind::Done)
    }

    fn kick(&mut self, lobby: &str, member: &str) {
        let req = Request::Kick { lobby: lobby.to_owned(), member: member.to_owned() };
        let _ = self.cmds.send(Cmd::Request { req, reply: None });
    }

    fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool {
        if to.is_empty() || to == self.me || bytes.len() > P2P_MAX_PACKET || self.dead.get() {
            return false;
        }
        let ch = match channel {
            Channel::Messages => super::CHANNEL_MESSAGES,
            Channel::Match => super::CHANNEL_MATCH,
        };
        let sent = self.cmds.send(Cmd::Send { to: to.to_owned(), channel: ch, bytes: bytes.to_vec() }).is_ok();
        if sent {
            self.linked.insert(to.to_owned());
        }
        sent
    }

    fn close_peer(&mut self, peer: &str) {
        self.linked.shift_remove(peer);
        RelayClient::close_peer(self, peer);
    }

    fn relayed(&self, peer: &str) -> Option<bool> {
        self.linked.contains(peer).then_some(true)
    }

    fn linked_peers(&self) -> Vec<String> {
        self.linked.iter().cloned().collect()
    }

    fn set_relay_mode(&mut self, _mode: RelayMode) -> Result<(), String> {
        // (every link goes through the relay)
        Ok(())
    }

    fn set_accept(&mut self, ids: IndexSet<String>) {
        let _ = self.cmds.send(Cmd::Accept(ids));
    }

    fn invite(&mut self, to: &str, payload: &str) -> Ask {
        self.call(Request::Invite { to: to.to_owned(), payload: payload.to_owned() }, Kind::Done)
    }

    fn presence(&mut self) -> Option<&mut dyn Presence> {
        if self.by_address { Some(self) } else { None }
    }

    fn rtt(&self) -> Option<f64> {
        RelayClient::rtt(self)
    }

    fn udp(&self) -> Option<bool> {
        RelayClient::udp(self)
    }

    fn poll(&mut self, _now: f64) -> Option<BackendEvent> {
        loop {
            if let Some(e) = self.made.pop_front() {
                return Some(e);
            }
            let e = RelayClient::poll_all(self)?;
            if let Some(b) = self.backend_event(e) {
                return Some(b);
            }
        }
    }

    fn shutdown(&mut self) -> Ask {
        let tag = self.next_tag;
        self.next_tag += 1;
        // (the thread is waited for at the drop, the bye with it)
        let _ = self.cmds.send(Cmd::Shutdown);
        self.dead.set(true);
        self.linked.clear();
        self.made.push_back(BackendEvent::Answered(Ask(tag), Ok(Answer::Done)));
        Ask(tag)
    }
}

impl Presence for RelayClient {
    fn set(&mut self, p: WirePresence) -> Ask {
        let tag = self.next_tag;
        self.next_tag += 1;
        self.asked.insert(tag, Kind::Done);
        if self.cmds.send(Cmd::Presence { tag, p }).is_err() {
            self.made.push_back(BackendEvent::Answered(Ask(tag), Err(self.closed().to_string())));
            self.asked.remove(&tag);
        }
        Ask(tag)
    }

    fn who(&mut self, limit: u32) -> Ask {
        self.call(Request::Who { limit }, Kind::Who(limit))
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, answer_of};
    use crate::backend::Answer;
    use serde_json::json;

    #[test]
    fn a_hosted_create_names_its_host_core() {
        let v = json!({ "lobby": "ab12", "host": "ab12.host" });
        assert_eq!(
            answer_of(Kind::Hosted, &v),
            Ok(Answer::Hosted { lobby: "ab12".into(), host: "ab12.host".into(), short: None })
        );
        assert!(answer_of(Kind::Hosted, &json!({ "lobby": "ab12" })).is_err(), "no host core");
        assert!(answer_of(Kind::Hosted, &json!({ "lobby": "ab12", "host": "r00" })).is_err(), "not a host id");
        assert_eq!(answer_of(Kind::Created, &v), Ok(Answer::Created("ab12".into())));
    }
}
