//! The game's port to the net service: the game asks for sessions and match packets on
//! it and hears the sign-in, its sessions' events and the match's packets, each event with the number of the session
//! it is of.

use mvs_net::service::PagePort;
use mvs_net::service::api::{ClosedWhy, NetEvent, NetRequest, SessionTarget, StatusWhy};

/// What the game's port runs over: the service's channels (`PagePort`), or a test's.
pub trait PageLink {
    /// A request to the service; false when the service is gone.
    fn post(&mut self, m: NetRequest) -> bool;
    /// The events that came since the last call, in order; None when the service is gone.
    fn poll(&mut self) -> Option<Vec<NetEvent>>;
}

impl PageLink for PagePort {
    fn post(&mut self, m: NetRequest) -> bool {
        self.tx.send(m).is_ok()
    }

    fn poll(&mut self) -> Option<Vec<NetEvent>> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(e) => out.push(e),
                Err(std::sync::mpsc::TryRecvError::Empty) => return Some(out),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return if out.is_empty() { None } else { Some(out) };
                }
            }
        }
    }
}

/// The sign-in as the service last told it.
#[derive(Clone, Debug, PartialEq)]
pub struct NetStatus {
    pub online: bool,
    pub me: String,
    pub why: StatusWhy,
}

/// The game's port. The service answers every open with exactly one `Closed`, in the order of the opens, and a
/// session's events come between its open and its `Closed`: an event is of the session after the
/// last one closed (0: the sign-in, an invite, a browse's answer).
pub struct NetPort {
    link: Option<Box<dyn PageLink>>,
    pub status: NetStatus,
    /// The way to the relay is the slower one (NetEvent::Way): signed in by TCP alone.
    pub slow: bool,
    opened: u32,
    closed: u32,
}

impl NetPort {
    /// A port over `link` (None: no service, online play off).
    pub fn new(link: Option<Box<dyn PageLink>>) -> NetPort {
        // Deviation: 'the online process has not started' is SigningIn (no backend's reason yet).
        NetPort {
            link,
            status: NetStatus { online: false, me: String::new(), why: StatusWhy::SigningIn },
            slow: false,
            opened: 0,
            closed: 0,
        }
    }

    /// The service is there.
    pub fn has_link(&self) -> bool {
        self.link.is_some()
    }

    /// Asks for a session: its number, which its events carry.
    pub fn open(&mut self, target: SessionTarget) -> u32 {
        self.post(NetRequest::Open(target));
        self.opened += 1;
        self.opened
    }

    pub fn post(&mut self, m: NetRequest) {
        if let Some(l) = &mut self.link
            && !l.post(m)
        {
            self.link = None;
        }
    }

    /// The events since the last call with the session each is of; a service gone ends every open session
    /// (`Stopped`) and the sign-in.
    pub fn poll(&mut self) -> Vec<(NetEvent, u32)> {
        let Some(l) = &mut self.link else { return Vec::new() };
        let got = l.poll();
        let mut out = Vec::new();
        for e in got.clone().unwrap_or_default() {
            out.push(self.take(e));
        }
        if got.is_none() {
            self.link = None;
            self.slow = false;
            while self.closed < self.opened {
                out.push(self.take(NetEvent::Closed(ClosedWhy::Stopped)));
            }
            out.push(self.take(NetEvent::Status { online: false, me: String::new(), why: StatusWhy::SigningIn }));
        }
        out
    }

    fn take(&mut self, e: NetEvent) -> (NetEvent, u32) {
        if let NetEvent::Status { online, me, why } = &e {
            self.status = NetStatus { online: *online, me: me.clone(), why: why.clone() };
        }
        if let NetEvent::Way { slow } = &e {
            self.slow = *slow;
        }
        let of = match e {
            NetEvent::Status { .. } | NetEvent::Invited { .. } | NetEvent::Browsed { .. } | NetEvent::Way { .. } => 0,
            _ => self.closed + 1,
        };
        if matches!(e, NetEvent::Closed(_)) {
            self.closed += 1;
        }
        (e, of)
    }
}

#[cfg(test)]
pub mod fake {
    //! A port of the tests: what was posted, and the events given next.
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[derive(Default)]
    pub struct Wire {
        pub posted: Vec<NetRequest>,
        pub events: Vec<NetEvent>,
        pub gone: bool,
    }

    pub struct FakeLink(pub Rc<RefCell<Wire>>);

    impl PageLink for FakeLink {
        fn post(&mut self, m: NetRequest) -> bool {
            let mut w = self.0.borrow_mut();
            w.posted.push(m);
            !w.gone
        }

        fn poll(&mut self) -> Option<Vec<NetEvent>> {
            let mut w = self.0.borrow_mut();
            let e = std::mem::take(&mut w.events);
            if w.gone && e.is_empty() { None } else { Some(e) }
        }
    }

    pub fn port() -> (NetPort, Rc<RefCell<Wire>>) {
        let w = Rc::new(RefCell::new(Wire::default()));
        (NetPort::new(Some(Box::new(FakeLink(w.clone())))), w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_numbered_by_session() {
        let (mut p, w) = fake::port();
        assert_eq!(p.open(SessionTarget::Host), 1);
        assert_eq!(p.open(SessionTarget::Queue), 2);
        w.borrow_mut().events = vec![
            NetEvent::Msg("a".into()),
            NetEvent::Closed(ClosedWhy::Replaced),
            NetEvent::Status { online: true, me: "me".into(), why: StatusWhy::Online },
            NetEvent::Msg("b".into()),
        ];
        let of: Vec<u32> = p.poll().into_iter().map(|e| e.1).collect();
        assert_eq!(of, [1, 1, 0, 2]);
        assert!(p.status.online && !p.slow);
        w.borrow_mut().events = vec![NetEvent::Way { slow: true }];
        assert_eq!(p.poll()[0].1, 0);
        assert!(p.slow);
        w.borrow_mut().gone = true;
        let e = p.poll();
        assert!(matches!(e[0], (NetEvent::Closed(ClosedWhy::Stopped), 2)));
        assert!(!p.status.online && !p.has_link() && !p.slow);
    }
}
