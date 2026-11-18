//! A queue match's end on the server: the game's result of the match
//! (match.result), its rematch vote (match.vote), everyone's votes as they come (match.votes), the set's outcome
//! (match.set) and, after an all-yes vote, the rematch's pre-match (pm.begin with `rematch`, its session the
//! connection's hub's). A queue match is a game of a best-of-3 set (the Free For All queue's set is one game). The
//! session is made at the match's start and lives across the podium and its end-of-game steps; what came is kept as
//! its state, which a step that binds late reads (`QueueEndView`).
//!
//! Deviation: listeners are the state the view carries and `QeNews` items; the timers the owner's `now`.

use mvs_protocol::messages::{ClientMessage, PmEndWhy, ServerMessage};
use mvs_protocol::online::{PmKind, QueueMode};
use mvs_protocol::profile::MatchReportPlayer;
use mvs_protocol::queue_rules::{REMATCH_MS, RESULT_MS, VOTE_MS, queue_team_style};
use mvs_ui::menu::online::{QueueEndView, SetOutcome};

use super::pre_match::{PreMatchBegin, PreMatchHub};

/// Unverified: (the port's own) the backstops of a server that says nothing more: match.set at the latest RESULT_MS
/// plus VOTE_MS after the game's result, the rematch's pm.begin REMATCH_MS after match.set {rematch: true}; each with
/// SLACK_MS of margin. Past the first the set is taken as over (no rematch), past the second the wait ends as at a
/// pm.end 'error'.
pub const SLACK_MS: f64 = 10000.0;

/// What changed, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum QeNews {
    Vote {
        slot: u32,
        yes: bool,
    },
    Set(SetOutcome),
    /// The rematch's pre-match began: the hub's current.
    Begin,
    End {
        why: PmEndWhy,
        name: String,
    },
}

pub struct NetQueueEnd {
    pub mode: QueueMode,
    pub team_style: u8,
    view: QueueEndView,
    voted: bool,
    timer: Option<f64>,
    closed: bool,
}

impl NetQueueEnd {
    /// The set's session at the match's start: a pm.end the hub kept from before this match is no end of its set.
    pub fn new(mode: QueueMode, hub: &mut PreMatchHub) -> NetQueueEnd {
        hub.lost_end = None;
        NetQueueEnd {
            mode,
            team_style: queue_team_style(mode),
            view: QueueEndView::default(),
            voted: false,
            timer: None,
            closed: false,
        }
    }

    /// What it heard so far.
    pub fn view(&self) -> &QueueEndView {
        &self.view
    }

    fn wait_over(&self) -> bool {
        self.view.rematch_begun || self.view.ended.is_some()
    }

    /// A message of the connection.
    pub fn take(&mut self, m: &ServerMessage, hub: &mut PreMatchHub, now: f64) -> Vec<QeNews> {
        let mut out = Vec::new();
        if self.closed {
            return out;
        }
        match m {
            ServerMessage::MatchVotes { slot, yes } => self.got_vote(*slot, *yes, &mut out),
            ServerMessage::MatchSet { rematch, wins, game } => {
                self.got_set(SetOutcome { rematch: *rematch, wins: wins.clone(), game: *game }, hub, now, &mut out);
            }
            _ => {}
        }
        out
    }

    /// A pre-match the hub began: a queue's with `rematch` is this set's next game.
    pub fn begun(&mut self, b: &PreMatchBegin, hub: &mut PreMatchHub, now: f64) -> Vec<QeNews> {
        let mut out = Vec::new();
        if self.closed || b.kind != PmKind::Queue {
            return out;
        }
        let Some(r) = &b.rematch else { return out };
        if self.wait_over() || self.view.set.as_ref().is_some_and(|s| !s.rematch) {
            return out;
        }
        // (a rematch's pm.begin with no match.set heard: the set is taken as all yes)
        if self.view.set.is_none() {
            let s = SetOutcome { rematch: true, wins: r.wins.clone(), game: r.game.saturating_sub(1) };
            self.got_set(s, hub, now, &mut out);
        }
        if self.wait_over() {
            return out;
        }
        self.view.rematch_begun = true;
        self.timer = None;
        out.push(QeNews::Begin);
        out
    }

    /// A pm.end no open pre-match took: the end of the rematch's wait (else the hub keeps it).
    pub fn lost_end(&mut self, why: PmEndWhy, name: String, hub: &mut PreMatchHub) -> Vec<QeNews> {
        let mut out = Vec::new();
        if self.waiting() {
            self.got_end(why, name, &mut out);
        } else {
            hub.lost_end = Some((why, name));
        }
        out
    }

    // In the rematch's wait.
    fn waiting(&self) -> bool {
        !self.closed && !self.wait_over() && self.view.set.as_ref().is_some_and(|s| s.rematch)
    }

    fn got_vote(&mut self, slot: u32, yes: bool, out: &mut Vec<QeNews>) {
        if self.view.set.is_some() {
            return;
        }
        self.view.votes.push((slot, yes));
        out.push(QeNews::Vote { slot, yes });
    }

    fn got_set(&mut self, s: SetOutcome, hub: &mut PreMatchHub, now: f64, out: &mut Vec<QeNews>) {
        // (a repeated match.set: no second wait)
        if self.view.set.is_some() {
            return;
        }
        self.view.set = Some(s.clone());
        out.push(QeNews::Set(s.clone()));
        self.timer = None;
        if !s.rematch {
            return;
        }
        // the rematch's wait: a leave in it ends the set with a pm.end and no pm.begin (one kept from before is
        // handed over at once)
        self.timer = Some(now + REMATCH_MS + SLACK_MS);
        if let Some((why, name)) = hub.lost_end.take() {
            self.got_end(why, name, out);
        }
    }

    fn got_end(&mut self, why: PmEndWhy, name: String, out: &mut Vec<QeNews>) {
        if self.wait_over() || !self.view.set.as_ref().is_some_and(|s| s.rematch) {
            return;
        }
        self.view.ended = Some((why, name.clone()));
        self.timer = None;
        out.push(QeNews::End { why, name });
    }

    /// match.result: the winning team as this game's sim saw it (-1 none), each slot's numbers and the map.
    pub fn send_result(
        &mut self,
        team: i32,
        players: Option<Vec<MatchReportPlayer>>,
        map: &str,
        now: f64,
    ) -> ClientMessage {
        if self.view.set.is_none() && self.timer.is_none() {
            self.timer = Some(now + RESULT_MS + VOTE_MS + SLACK_MS);
        }
        ClientMessage::MatchResult { team, players, map: (!map.is_empty()).then(|| map.to_owned()) }
    }

    /// match.vote: once; the first vote counts (the server's rule too).
    pub fn vote(&mut self, yes: bool) -> Option<ClientMessage> {
        if self.voted || self.view.set.is_some() || self.closed {
            return None;
        }
        self.voted = true;
        Some(ClientMessage::MatchVote { yes })
    }

    /// Out of the set (PLAY AGAIN's ResetMatchFlow): pm.leave.
    pub fn leave() -> ClientMessage {
        ClientMessage::PmLeave
    }

    /// The backstops due by `now`.
    pub fn pump(&mut self, hub: &mut PreMatchHub, now: f64) -> Vec<QeNews> {
        let mut out = Vec::new();
        if self.closed || self.timer.is_none_or(|t| now < t) {
            return out;
        }
        self.timer = None;
        if self.view.set.is_none() {
            self.got_set(SetOutcome { rematch: false, wins: Vec::new(), game: 0 }, hub, now, &mut out);
        } else {
            self.got_end(PmEndWhy::Error, String::new(), &mut out);
        }
        out
    }

    pub fn next_due(&self) -> Option<f64> {
        if self.closed { None } else { self.timer }
    }

    /// The connection's loss: before match.set the set is over; in the rematch's wait it ends as at a pm.end 'error'.
    pub fn connection_lost(&mut self, hub: &mut PreMatchHub, now: f64) -> Vec<QeNews> {
        let mut out = Vec::new();
        if self.closed {
            return out;
        }
        if self.view.set.is_none() {
            self.got_set(SetOutcome { rematch: false, wins: Vec::new(), game: 0 }, hub, now, &mut out);
        } else {
            self.got_end(PmEndWhy::Error, String::new(), &mut out);
        }
        out
    }

    /// Closed: the set's end screen is gone (or the match left with none).
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Nothing more is heard.
    pub fn close(&mut self) {
        self.closed = true;
        self.timer = None;
    }
}

#[cfg(test)]
mod tests {
    use super::super::tables::Tables;
    use super::*;
    use serde_json::json;

    fn msg(v: serde_json::Value) -> ServerMessage {
        serde_json::from_value(v).unwrap()
    }

    fn begin(id: &str, kind: &str, rematch: bool) -> ServerMessage {
        let mut v = json!({ "t": "pm.begin", "id": id, "kind": kind, "mode": "1v1", "teamStyle": 0, "map": "", "me": 0, "players": [
            { "id": "a", "name": "Ann", "team": 0, "character": "", "skin": "", "bot": false },
            { "id": "b", "name": "Bob", "team": 1, "character": "", "skin": "", "bot": false } ] });
        if rematch {
            v["rematch"] = json!({ "game": 2, "wins": [1, 0] });
        }
        msg(v)
    }

    /// The connection as online.rs routes it: the hub first, then the set's session.
    struct Rig {
        hub: PreMatchHub,
        qe: NetQueueEnd,
        tables: Tables,
        got: Vec<String>,
        now: f64,
    }

    impl Rig {
        fn new() -> Rig {
            let mut hub = PreMatchHub::default();
            let qe = NetQueueEnd::new(QueueMode::OneVOne, &mut hub);
            Rig { hub, qe, tables: Tables::default(), got: Vec::new(), now: 0.0 }
        }

        fn note(&mut self, news: Vec<QeNews>) {
            for n in news {
                self.got.push(match n {
                    QeNews::Vote { .. } => continue,
                    QeNews::Set(s) => format!(
                        "set {} {} {}",
                        s.rematch,
                        s.wins.iter().map(u32::to_string).collect::<Vec<_>>().join("-"),
                        s.game
                    ),
                    QeNews::Begin => format!("begin {}", self.hub.current.as_ref().unwrap().begin.id),
                    QeNews::End { why, name } => format!("end {why:?} {name}"),
                });
            }
        }

        fn recv(&mut self, m: ServerMessage) {
            let (now, t) = (self.now, self.tables.clone());
            let news = match self.hub.take(&m, &t) {
                Some(super::super::pre_match::HubNews::Begun) => {
                    let b = self.hub.current.as_ref().unwrap().begin.clone();
                    self.qe.begun(&b, &mut self.hub, now)
                }
                Some(super::super::pre_match::HubNews::LostEnd(why, name)) => {
                    self.qe.lost_end(why, name, &mut self.hub)
                }
                None => self.qe.take(&m, &mut self.hub, now),
            };
            self.note(news);
        }

        fn step(&mut self, ms: f64) {
            self.now += ms;
            let n = self.qe.pump(&mut self.hub, self.now);
            self.note(n);
        }

        fn joined(&self) -> String {
            self.got.join(",")
        }
    }

    #[test]
    fn result_and_votes() {
        let mut r = Rig::new();
        let p = MatchReportPlayer { slot: 0, damage: 120, ringouts: 2, deaths: 1, score: 2, ..Default::default() };
        let m = r.qe.send_result(1, Some(vec![p]), "M005_Map", 0.0);
        assert!(m.text().starts_with(r#"{"t":"match.result","team":1,"players":[{"slot":0,"damage":120"#));
        assert!(m.text().ends_with(r#""map":"M005_Map"}"#));
        r.recv(msg(json!({ "t": "match.votes", "slot": 1, "yes": true })));
        assert_eq!(r.qe.vote(true), Some(ClientMessage::MatchVote { yes: true }));
        assert!(r.qe.vote(false).is_none());
        r.recv(msg(json!({ "t": "match.votes", "slot": 0, "yes": true })));
        assert_eq!(r.qe.view().votes, [(1, true), (0, true)]);
        r.recv(msg(json!({ "t": "match.set", "rematch": false, "wins": [2, 0], "game": 2 })));
        r.recv(msg(json!({ "t": "match.votes", "slot": 1, "yes": false })));
        r.recv(msg(json!({ "t": "match.set", "rematch": true, "wins": [1, 1], "game": 2 })));
        assert!(r.joined() == "set false 2-0 2" && r.qe.view().votes.len() == 2);
        let mut r2 = Rig::new();
        r2.recv(msg(json!({ "t": "match.set", "rematch": false, "wins": [0, 2], "game": 2 })));
        assert!(r2.qe.vote(true).is_none());
    }

    #[test]
    fn the_backstops() {
        let mut r = Rig::new();
        r.qe.send_result(1, None, "", 0.0);
        r.step(RESULT_MS + VOTE_MS + SLACK_MS - 1.0);
        assert_eq!(r.joined(), "");
        r.step(1.0);
        assert_eq!(r.joined(), "set false  0");
        let mut r = Rig::new();
        r.recv(msg(json!({ "t": "match.set", "rematch": true, "wins": [1, 0], "game": 1 })));
        r.step(REMATCH_MS + SLACK_MS - 1.0);
        assert_eq!(r.joined(), "set true 1-0 1");
        r.step(1.0);
        assert_eq!(r.joined(), "set true 1-0 1,end Error ");
    }

    #[test]
    fn the_rematch() {
        let mut r = Rig::new();
        r.qe.send_result(0, None, "", 0.0);
        r.recv(msg(json!({ "t": "match.set", "rematch": true, "wins": [1, 0], "game": 1 })));
        r.recv(begin("pm9", "queue", true));
        r.step(REMATCH_MS + SLACK_MS + 1000.0);
        assert_eq!(r.joined(), "set true 1-0 1,begin pm9");
        r.recv(begin("pm10", "queue", true));
        assert_eq!(r.joined(), "set true 1-0 1,begin pm9");
        // the rematch's own pm.end is its session's
        r.recv(msg(json!({ "t": "pm.end", "why": "left", "name": "Bob" })));
        assert!(r.hub.lost_end.is_none() && r.joined() == "set true 1-0 1,begin pm9");
        // another kind, no rematch
        let mut r = Rig::new();
        r.recv(msg(json!({ "t": "match.set", "rematch": true, "wins": [1, 0], "game": 1 })));
        r.recv(begin("pm9", "lobby", true));
        r.recv(begin("pm9", "queue", false));
        assert_eq!(r.joined(), "set true 1-0 1");
        // no match.set heard
        let mut r = Rig::new();
        r.recv(begin("pm9", "queue", true));
        assert_eq!(r.joined(), "set true 1-0 1,begin pm9");
        // after a set with no rematch
        let mut r = Rig::new();
        r.recv(msg(json!({ "t": "match.set", "rematch": false, "wins": [1, 1], "game": 2 })));
        r.recv(begin("pm9", "queue", true));
        assert_eq!(r.joined(), "set false 1-1 2");
    }

    #[test]
    fn the_ends_of_the_wait() {
        let mut r = Rig::new();
        r.recv(msg(json!({ "t": "match.set", "rematch": true, "wins": [1, 0], "game": 1 })));
        r.recv(msg(json!({ "t": "pm.end", "why": "left", "name": "Bob" })));
        r.recv(begin("pm9", "queue", true));
        assert_eq!(r.joined(), "set true 1-0 1,end Left Bob");
        let mut r = Rig::new();
        r.recv(msg(json!({ "t": "pm.end", "why": "error", "name": "" })));
        r.recv(msg(json!({ "t": "match.set", "rematch": true, "wins": [1, 0], "game": 1 })));
        assert_eq!(r.joined(), "set true 1-0 1,end Error ");
        // a pm.end from before the set's session
        let mut hub = PreMatchHub { lost_end: Some((PmEndWhy::Left, "Old".into())), ..Default::default() };
        let qe = NetQueueEnd::new(QueueMode::OneVOne, &mut hub);
        let mut r = Rig { hub, qe, tables: Tables::default(), got: Vec::new(), now: 0.0 };
        r.recv(msg(json!({ "t": "match.set", "rematch": true, "wins": [1, 0], "game": 1 })));
        assert_eq!(r.joined(), "set true 1-0 1");
    }

    #[test]
    fn the_connection_lost_and_closed() {
        let mut r = Rig::new();
        r.qe.send_result(0, None, "", 0.0);
        let n = r.qe.connection_lost(&mut r.hub, 0.0);
        r.note(n);
        r.step(RESULT_MS + VOTE_MS + SLACK_MS);
        assert_eq!(r.joined(), "set false  0");
        let mut u = Rig::new();
        u.recv(msg(json!({ "t": "match.set", "rematch": true, "wins": [0, 1], "game": 1 })));
        let n = u.qe.connection_lost(&mut u.hub, 0.0);
        u.note(n);
        u.step(REMATCH_MS + SLACK_MS);
        assert_eq!(u.joined(), "set true 0-1 1,end Error ");
        let mut c = Rig::new();
        c.qe.close();
        c.recv(msg(json!({ "t": "match.set", "rematch": true, "wins": [1, 0], "game": 1 })));
        assert!(c.got.is_empty());
    }
}
