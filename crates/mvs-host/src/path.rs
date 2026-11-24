//! A match's path (`mvs_protocol::path`): on a core that measures (`HostOptions::paths`), each pre-match of 2+ humans
//! asks whether to measure, sends the humans each other's Epic ids (path.peers), takes their reports (path.report)
//! and chooses who referees the match: the relay, or one human's app. The choice goes to every human (pm.path) and
//! to the match at its start; at the go a chosen human still in the match referees it (ref.go, play.rs).
//!
//! Deviation: the port's own rule (the build's matches go through its own servers).

use mvs_protocol::messages::{PathPeer, ServerMessage};
use mvs_protocol::path::{PATH_WAIT_MS, PathChoice, PathInput, choose, should_measure};
use serde_json::{Map, Value};

use crate::core::{Core, Due, PmId};
use crate::link::ConnKey;
use crate::timers::TimerId;

/// The most characters of an Epic id a hello may carry (a product user id is 32).
const EPIC_MAX: usize = 64;
/// The most links a path.report is read for (a match has at most 8 humans, each with a link to the 7 others).
const REPORT_LINKS: usize = 8;

/// A pre-match's path: the humans measured, by human index (their slot's order), and what they reported.
pub(crate) struct PathState {
    pub keys: Vec<ConnKey>,
    pub slots: Vec<i32>,
    pub epic: Vec<Option<String>>,
    /// each human's round trip to the relay (ms): its hello's or queue.join's, then its report's
    pub relay: Vec<Option<f64>>,
    /// `links[i][j]`: the round trip human i reported to human j (ms)
    pub links: Vec<Vec<Option<f64>>>,
    pub reported: Vec<bool>,
    /// the wait for the reports (PATH_WAIT_MS), while it runs
    pub wait: Option<TimerId>,
    /// the choice is made (pm.path sent)
    pub decided: bool,
    /// the human chosen to referee; None: the relay
    pub chosen: Option<Chosen>,
}

/// A human chosen to referee a match: its link and Epic id, and every measured human's Epic id by link (ref.go's
/// humans: the ids the links were measured on, not a later hello's).
#[derive(Clone, Debug)]
pub(crate) struct Chosen {
    pub key: ConnKey,
    pub epic: String,
    pub epics: Vec<(ConnKey, String)>,
}

/// A hello's Epic id: printable ASCII of 1..=EPIC_MAX characters; anything else none.
fn epic_of(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    let ok = !s.is_empty() && s.len() <= EPIC_MAX && s.bytes().all(|b| b.is_ascii_graphic());
    ok.then(|| s.to_string())
}

/// A round trip as a game sends it (ms): a finite number, 0 or more.
pub(crate) fn rtt_of(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|r| r.is_finite() && *r >= 0.0)
}

/// A round trip as pm.path carries it: whole ms.
fn whole_ms(ms: f64) -> u32 {
    // (a finite round trip of 0 or more, clamped before the cast)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = ms.round().clamp(0.0, f64::from(u32::MAX)) as u32;
    n
}

impl Core {
    /// A hello's path fields: the game's Epic id, its round trip to the relay and its Direct setting (each hello's,
    /// the latest wins; a game signed in to Epic after its hello is counted at its next one).
    pub(crate) fn path_hello(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let c = self.cm(k);
        c.epic = epic_of(m.get("epic"));
        c.relay_rtt = rtt_of(m.get("rtt"));
        c.direct = m.get("direct").and_then(Value::as_bool);
    }

    /// The pre-match's path starts (queue.found, pm.begin, a set's rematch's pm.begin): on a core that measures,
    /// a pre-match of 2+ humans is measured (path.peers to each human, the reports waited for PATH_WAIT_MS) when
    /// `should_measure` says so; else its path is chosen at once (the relay) and told.
    pub(crate) fn path_begin(&mut self, pm: PmId) {
        if !self.paths {
            return;
        }
        if let Some(w) = self.pm_mut(pm).path.take().and_then(|p| p.wait) {
            self.cancel(w);
        }
        let mut keys = self.pm(pm).humans.clone();
        keys.sort_by_key(|&k| self.c(k).slot);
        let n = keys.len();
        if n < 2 {
            return;
        }
        let slots: Vec<i32> = keys.iter().map(|&k| self.c(k).slot).collect();
        let epic: Vec<Option<String>> = keys.iter().map(|&k| self.c(k).epic.clone()).collect();
        let relay: Vec<Option<f64>> = keys.iter().map(|&k| self.c(k).relay_rtt).collect();
        // (two humans that give one Epic id cannot be told apart on Epic: the relay referees, unmeasured)
        let twice = epic.iter().enumerate().any(|(i, e)| e.is_some() && epic[..i].contains(e));
        let measure = !twice && should_measure(&relay, &epic.iter().map(Option::is_some).collect::<Vec<bool>>());
        let direct = self.pm(pm).lobby && keys.iter().all(|&k| self.c(k).direct == Some(true));
        let mut state = PathState {
            keys: keys.clone(),
            slots: slots.clone(),
            epic: epic.clone(),
            relay,
            links: vec![vec![None; n]; n],
            reported: vec![false; n],
            wait: None,
            decided: false,
            chosen: None,
        };
        if !measure {
            self.pm_mut(pm).path = Some(state);
            return self.path_decide(pm);
        }
        state.wait = Some(self.later(PATH_WAIT_MS, Due::PathWait(pm), false));
        self.pm_mut(pm).path = Some(state);
        for (i, &k) in keys.iter().enumerate() {
            let peers: Vec<PathPeer> = (0..n)
                .filter(|&j| j != i)
                .map(|j| PathPeer {
                    slot: u32::try_from(slots[j]).expect("a human's slot"),
                    epic: epic[j].clone().expect("measured: every human on Epic"),
                })
                .collect();
            self.send(k, &ServerMessage::PathPeers { peers, direct });
        }
        let line = format!("path measure {}", self.pm(pm).id);
        self.log(&line);
    }

    /// path.report { relay, peers: [{ slot, rtt }] }: a measured human's round trips, its first report alone. The
    /// choice is made once every human's is in.
    pub(crate) fn on_path_report(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let Some(pm) = self.c(k).pm else { return };
        let Some(st) = self.pms.get_mut(&pm).filter(|p| p.alive).and_then(|p| p.path.as_mut()) else { return };
        if st.decided || st.wait.is_none() {
            return;
        }
        let Some(i) = st.keys.iter().position(|&x| x == k) else { return };
        if st.reported[i] {
            return;
        }
        st.reported[i] = true;
        if let Some(r) = rtt_of(m.get("relay")) {
            st.relay[i] = Some(r);
        }
        if let Some(Value::Array(peers)) = m.get("peers") {
            for l in peers.iter().take(REPORT_LINKS) {
                let slot = l.get("slot").and_then(Value::as_i64);
                let Some(j) = st.slots.iter().position(|&s| Some(i64::from(s)) == slot) else { continue };
                if j != i {
                    st.links[i][j] = rtt_of(l.get("rtt"));
                }
            }
        }
        if st.reported.iter().all(|r| *r) {
            self.path_decide(pm);
        }
    }

    /// The wait for a pre-match's reports is over: the choice is made with those that came.
    pub(crate) fn path_wait(&mut self, pm: PmId, id: TimerId) {
        let Some(st) = self.pms.get_mut(&pm).filter(|p| p.alive).and_then(|p| p.path.as_mut()) else { return };
        if st.wait != Some(id) || st.decided {
            return;
        }
        st.wait = None;
        self.path_decide(pm);
    }

    /// The match is about to start: a path still waiting for reports is chosen with those it has.
    pub(crate) fn path_settle(&mut self, pm: PmId) {
        if self.pm(pm).path.as_ref().is_some_and(|p| !p.decided) {
            self.path_decide(pm);
        }
    }

    /// The pre-match's referee chosen (`path::choose`) and told to every human: pm.path { referee (-1 the relay),
    /// ms, far } by slot (a bot's ms none, far false).
    fn path_decide(&mut self, pm: PmId) {
        let size = self.pm(pm).players.len();
        let Some(st) = self.pm_mut(pm).path.as_mut() else { return };
        st.decided = true;
        let wait = st.wait.take();
        let o = choose(&PathInput { relay_rtt: st.relay.clone(), links: st.links.clone() });
        let mut ms: Vec<Option<u32>> = vec![None; size];
        let mut far = vec![false; size];
        for (i, &s) in st.slots.iter().enumerate() {
            let Some(at) = usize::try_from(s).ok().filter(|&s| s < size) else { continue };
            ms[at] = o.rtt_to_referee[i].map(whole_ms);
            far[at] = o.far[i];
        }
        let referee = match o.choice {
            PathChoice::Relay => -1,
            PathChoice::Player(i) => {
                let epics = st.keys.iter().zip(&st.epic).map(|(&k, e)| (k, e.clone().unwrap_or_default())).collect();
                st.chosen =
                    Some(Chosen { key: st.keys[i], epic: st.epic[i].clone().expect("a candidate is on Epic"), epics });
                st.slots[i]
            }
        };
        if let Some(w) = wait {
            self.cancel(w);
        }
        let worst = |w: Option<f64>| w.map_or("-".to_string(), |w| format!("{}", whole_ms(w)));
        let line = format!(
            "path {} referee {referee} relay {} best {}",
            self.pm(pm).id,
            worst(o.relay_worst),
            worst(o.worst.iter().flatten().copied().reduce(f64::min))
        );
        self.log(&line);
        let msg = ServerMessage::PmPath { referee, ms, far };
        let text = msg.text();
        for k in self.pm(pm).humans.clone() {
            self.send_text(k, &text);
        }
    }

    /// The pre-match's chosen referee, for its match (start_match): None the relay.
    pub(crate) fn path_chosen(&self, pm: PmId) -> Option<Chosen> {
        self.pm(pm).path.as_ref().and_then(|p| p.chosen.clone())
    }
}
