//! Another player's profile and this player's given to others: another player's is asked of
//! their game through the session's host core (profile.get, its `profile` answer), and this game answers such asks
//! with its own (profile.ask, profile.tell), the stats file's under this player's id, name and icon.
//!
//! Deviation: an answer is read by the codec (mvs-protocol `ProfileWire`'s serde), not by a
//! field-by-field read: an answer of another shape is a message the session drops, where a field-by-field read keeps
//! what fits.
//! Deviation: the own profile, the badges' counts and the toast count are the stats file's as the menus read it
//! (mvs-ui own_profile.rs, the stats file's spend_toast), not kept here.

use std::collections::BTreeMap;

use mvs_protocol::messages::ClientMessage;
use mvs_protocol::profile::ProfileWire;

/// (the port's own) How long a request for another player's profile waits: longer than the host core waits for that
/// player's answer (5 s), so the core's null answer comes first.
pub const ANSWER_MS: f64 = 7000.0;
/// (the port's own) The host core that passes an ask on may be anyone's (the queue's host is a stranger), and each
/// answer is up to TELL_MAX bytes of this player's link: an asker is answered once per ASK_AGAIN_MS (a sooner ask gets
/// nothing: the core answers it with no profile), and ASKERS_AT_ONCE askers within that time at the most. A profile
/// had within that time is given again from its copy.
pub const ASK_AGAIN_MS: f64 = 10000.0;
pub const ASKERS_AT_ONCE: usize = 6;
/// The host core's cap on a profile.tell (its PROFILE_MAX, in bytes of the message's JSON): a longer one loses the
/// link.
pub const TELL_MAX: usize = 32768;

/// What a profile ask has come to.
#[derive(Clone, Debug, PartialEq)]
pub enum ProfileAnswer {
    /// Asked, not answered yet.
    Waiting,
    /// The answer: None with no session, for an id not in it, or with no answer in ANSWER_MS.
    Done(Option<Box<ProfileWire>>),
}

/// The profile asks both ways of one session's life (the hub's).
#[derive(Debug, Default)]
pub struct Profiles {
    // askers answered within ASK_AGAIN_MS, by id
    answered: BTreeMap<String, f64>,
    // the profiles of other players answered within ASK_AGAIN_MS, by id (their game would not answer again sooner)
    had: BTreeMap<String, (ProfileWire, f64)>,
    // the asks out: id to the time they give up
    asking: BTreeMap<String, f64>,
    // the asks done, not taken yet
    done: BTreeMap<String, Option<ProfileWire>>,
    /// The askers to answer with this game's profile (taken by the owner, which has the stats file: `tell`).
    pub asks: Vec<String>,
}

impl Profiles {
    /// profile.ask: the asker is answered when mayAnswer lets it.
    pub fn asked(&mut self, from: &str, now: f64) {
        if from.is_empty() || !self.may_answer(from, now) {
            return;
        }
        self.asks.push(from.to_owned());
    }

    fn may_answer(&mut self, to: &str, now: f64) -> bool {
        self.answered.retain(|_, at| now - *at < ASK_AGAIN_MS);
        if self.answered.contains_key(to) || self.answered.len() >= ASKERS_AT_ONCE {
            return false;
        }
        self.answered.insert(to.to_owned(), now);
        true
    }

    /// loadProfile: the kept copy of `id`'s profile when there is one, else the ask's state; `send` takes the
    /// profile.get of a new ask (None: no open session, the answer is None at once).
    pub fn ask(&mut self, id: &str, open: bool, now: f64, send: &mut dyn FnMut(ClientMessage)) -> ProfileAnswer {
        if !open || id.is_empty() {
            return ProfileAnswer::Done(None);
        }
        self.had.retain(|_, (_, at)| now - *at < ASK_AGAIN_MS);
        if let Some((p, _)) = self.had.get(id) {
            return ProfileAnswer::Done(Some(Box::new(p.clone())));
        }
        if let Some(p) = self.done.remove(id) {
            return ProfileAnswer::Done(p.map(Box::new));
        }
        if !self.asking.contains_key(id) {
            self.asking.insert(id.to_owned(), now + ANSWER_MS);
            send(ClientMessage::ProfileGet { id: id.to_owned() });
        }
        ProfileAnswer::Waiting
    }

    /// The host core's `profile` answer.
    pub fn got(&mut self, id: &str, profile: Option<ProfileWire>, now: f64) {
        if self.asking.remove(id).is_none() {
            return;
        }
        if let Some(p) = &profile {
            self.had.insert(id.to_owned(), (p.clone(), now));
        }
        self.done.insert(id.to_owned(), profile);
    }

    /// Asks not answered by their time end with none.
    pub fn pump(&mut self, now: f64) {
        let late: Vec<String> =
            self.asking.iter().filter(|(_, until)| now >= **until).map(|(k, _)| k.clone()).collect();
        for id in late {
            self.asking.remove(&id);
            self.done.insert(id, None);
        }
    }

    pub fn next_due(&self) -> Option<f64> {
        self.asking.values().copied().reduce(f64::min)
    }

    /// The session closed: asks out end with none.
    pub fn closed(&mut self) {
        for (id, _) in std::mem::take(&mut self.asking) {
            self.done.insert(id, None);
        }
        self.asks.clear();
    }
}

/// The profile.tell answering `to` with this game's profile, its oldest matches dropped until it fits TELL_MAX; none
/// when even no match does not.
pub fn tell(to: &str, p: Option<ProfileWire>) -> ClientMessage {
    let profile = p.and_then(|p| {
        (0..=p.matches.len()).rev().find_map(|n| {
            let mut q = p.clone();
            q.matches.truncate(n);
            let m = ClientMessage::ProfileTell { to: to.to_owned(), profile: Some(q.clone()) };
            (m.text().len() <= TELL_MAX).then_some(q)
        })
    });
    ClientMessage::ProfileTell { to: to.to_owned(), profile }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn askers_are_answered_once_per_wait_and_six_at_once() {
        let mut p = Profiles::default();
        p.asked("a", 0.0);
        p.asked("a", 100.0);
        for i in 0..8 {
            p.asked(&format!("b{i}"), 200.0);
        }
        assert_eq!(p.asks.len(), ASKERS_AT_ONCE);
        p.asks.clear();
        p.asked("a", ASK_AGAIN_MS);
        assert_eq!(p.asks, vec!["a".to_owned()]);
    }

    #[test]
    fn an_ask_waits_for_its_answer_or_gives_up() {
        let mut p = Profiles::default();
        let mut sent = Vec::new();
        assert_eq!(p.ask("x", true, 0.0, &mut |m| sent.push(m)), ProfileAnswer::Waiting);
        assert_eq!(p.ask("x", true, 1.0, &mut |m| sent.push(m)), ProfileAnswer::Waiting);
        assert_eq!(sent.len(), 1);
        let wire = ProfileWire { id: "x".into(), ..Default::default() };
        p.got("x", Some(wire.clone()), 2.0);
        assert_eq!(p.ask("x", true, 3.0, &mut |m| sent.push(m)), ProfileAnswer::Done(Some(Box::new(wire.clone()))));
        // kept within ASK_AGAIN_MS
        assert_eq!(p.ask("x", true, 4.0, &mut |m| sent.push(m)), ProfileAnswer::Done(Some(Box::new(wire))));
        assert_eq!(p.ask("y", true, 0.0, &mut |m| sent.push(m)), ProfileAnswer::Waiting);
        p.pump(ANSWER_MS);
        assert_eq!(p.ask("y", true, ANSWER_MS, &mut |m| sent.push(m)), ProfileAnswer::Done(None));
        assert_eq!(p.ask("z", false, 0.0, &mut |m| sent.push(m)), ProfileAnswer::Done(None));
    }

    #[test]
    fn a_tell_drops_old_matches_to_fit() {
        let m = mvs_protocol::profile::MatchRecord { id: "m".repeat(4000), ..Default::default() };
        let p = ProfileWire { matches: vec![m; 20], ..Default::default() };
        let ClientMessage::ProfileTell { profile: Some(q), .. } = tell("a", Some(p)) else { panic!() };
        assert!(q.matches.len() < 20 && !q.matches.is_empty());
        assert!(ClientMessage::ProfileTell { to: "a".into(), profile: Some(q) }.text().len() <= TELL_MAX);
    }
}
