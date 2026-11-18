//! Coming back after a drop: a second connection to the relay beside the first, whose welcome takes the seat over.
//! Both sides keep a ring of the channel-0 data frames they sent as (peer id, bytes) and count those they took; the
//! hello tells the relay this client's count, the welcome tells the client the relay's, and each resends from the
//! other's. The second connection has keys of its own (its start's exchange): what the first one held to send, sealed
//! with the old keys, is dropped with it; the calls held while it was down are sealed as they go on the new one.
//!
//! Deviation: the build has no way back into a match; this resume is the port's own rule.

use std::collections::VecDeque;
use std::io::Write as _;
use std::net::Shutdown;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use mvs_protocol::match_wire::{RESUME_MS, RESUME_TRY_MS};
use mvs_protocol::relay_wire::{ClientMessage, FT_JSON};

use super::{Net, Paced, RelayError, STALL_PING_MS, StartWith, Welcome, socket_why, start};

/// The channel-0 frames a client keeps for a resume, at the most (bytes): a gap past them fails the resume.
/// Unverified: the port's own number (a lobby's messages over 20 s are far fewer).
const RING_BYTES: usize = 1 << 20;
/// A second connection that failed is tried again after this long (ms). Unverified: the port's own number.
const RETRY_MS: f64 = 500.0;

/// A channel-0 data frame sent: to whom, its bytes, and whether its peer's link was closed since (a frame resent
/// then would open the link again: it is not).
struct Sent {
    to: String,
    bytes: Vec<u8>,
    closed: bool,
}

/// The channel-0 data frames sent on the seat: the newest of them kept, and their count.
#[derive(Default)]
pub(super) struct Ring {
    frames: VecDeque<Sent>,
    bytes: usize,
    sent: u64,
}

impl Ring {
    /// A frame sent.
    pub(super) fn sent(&mut self, to: String, bytes: Vec<u8>) {
        self.bytes += bytes.len();
        self.frames.push_back(Sent { to, bytes, closed: false });
        self.sent += 1;
        while self.bytes > RING_BYTES {
            let Some(f) = self.frames.pop_front() else { break };
            self.bytes -= f.bytes.len();
        }
    }

    /// The link to `peer` closed (its closePeer written): the frames to it before are not sent again.
    pub(super) fn closed(&mut self, peer: &str) {
        for f in self.frames.iter_mut().filter(|f| f.to == peer) {
            f.closed = true;
        }
    }

    /// The frames from the `n`th on to peers not closed since, taken out (the count set back to `n`: they are sent
    /// again); None when the ring no longer holds them all.
    fn from(&mut self, n: u64) -> Option<Vec<(String, Vec<u8>)>> {
        let first = self.sent - self.frames.len() as u64;
        if n > self.sent || n < first {
            return None;
        }
        let k = usize::try_from(n - first).ok()?;
        let rest: Vec<_> = self.frames.split_off(k).into_iter().collect();
        self.bytes -= rest.iter().map(|f| f.bytes.len()).sum::<usize>();
        self.sent = n;
        Some(rest.into_iter().filter(|f| !f.closed).map(|f| (f.to, f.bytes)).collect())
    }
}

/// A second connection under way on a thread of its own, the count its hello told, its cancel and its handover: the
/// thread hands its welcome over only while `ended` is false (under its lock), else it leaves the seat with a bye.
pub(super) struct Attempt {
    rx: Receiver<Result<Welcome, RelayError>>,
    got: u64,
    cancel: Arc<AtomicBool>,
    ended: Arc<Mutex<bool>>,
}

impl Attempt {
    /// The client has ended: no prove goes once this is seen (the claim), and a welcome got or yet to come
    /// leaves with a bye, never taking the seat.
    pub(super) fn end(self) {
        self.cancel.store(true, Ordering::Relaxed);
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner) = true;
        while let Ok(r) = self.rx.try_recv() {
            if let Ok(w) = r {
                bye(w);
            }
        }
    }
}

/// A connection this client does not keep: left with a bye, sealed (the relay keeps no seat for it).
fn bye(mut w: Welcome) {
    let body = serde_json::to_vec(&ClientMessage::Bye).expect("a bye serialises");
    let mut f = Vec::new();
    if w.tx.frame_into(&mut f, FT_JSON, &[&body]).is_ok() {
        let _ = w.tcp.set_nonblocking(false);
        let _ = w.tcp.set_write_timeout(Some(Duration::from_millis(200)));
        let _ = w.tcp.write_all(&f);
    }
    let _ = w.tcp.shutdown(Shutdown::Both);
}

impl Net {
    /// The connection has dropped (closed, failed, silent): no reads or writes on it, a resume begins. At a shutdown
    /// it is the end.
    pub(super) fn drop_link(&mut self, why: &str) {
        if self.gone.is_some() || self.down.is_some() {
            return;
        }
        if self.ending {
            return self.lose(why);
        }
        eprintln!("relay: the connection dropped ({why}): connecting again");
        let _ = self.w.tcp.shutdown(Shutdown::Both);
        self.down = Some((self.now(), why.to_owned()));
        self.out.clear();
        self.faults.swapped();
        self.next_try = f64::NEG_INFINITY;
    }

    /// A second connection's answer taken; one started where the connection is down, or the match has heard nothing
    /// for RESUME_TRY_MS; the end where the drop is RESUME_MS old.
    pub(super) fn resume_timers(&mut self, now: f64) {
        if let Some(a) = &self.attempt {
            let got = a.got;
            match a.rx.try_recv() {
                Err(TryRecvError::Empty) => {}
                Ok(Ok(w)) => {
                    self.attempt = None;
                    self.resumed(w, got);
                }
                Ok(Err(e)) => {
                    eprintln!("relay: the second connection failed: {e}");
                    self.attempt = None;
                    self.next_try = now + RETRY_MS;
                }
                Err(TryRecvError::Disconnected) => {
                    self.attempt = None;
                    self.next_try = now + RETRY_MS;
                }
            }
        }
        if self.gone.is_some() {
            return;
        }
        if let Some((since, why)) = &self.down
            && now - since >= RESUME_MS
        {
            let why = why.clone();
            return self.lose(&why);
        }
        // (past RESUME_MS the referee has let this player go: the match ends as it does)
        let stalled = self.down.is_none()
            && self.in_match(now)
            && self.match_heard.is_some_and(|h| now - h >= RESUME_TRY_MS && now - h < RESUME_MS);
        if !stalled {
            self.stall_ping = None;
        }
        if self.attempt.is_some() || now < self.next_try {
            return;
        }
        if self.down.is_some() {
            return self.begin_attempt(now);
        }
        if !stalled {
            return;
        }
        // (a stall: the relay pinged first; one that answers is healthy, the peers are the silent ones, and is asked
        // again a RESUME_TRY_MS later)
        match self.stall_ping {
            None => {
                self.stall_ping = Some(now);
                self.write_json(&ClientMessage::Ping);
            }
            Some(at) if self.last_heard > at => {
                self.stall_ping = None;
                self.next_try = now + RESUME_TRY_MS;
            }
            Some(at) if now - at >= STALL_PING_MS => {
                self.stall_ping = None;
                self.begin_attempt(now);
            }
            Some(_) => {}
        }
    }

    /// A second connection, its hello counting the frames taken. In a cut it waits for the cut's end (its packets
    /// could not pass).
    pub(super) fn begin_attempt(&mut self, now: f64) {
        let hold = self.faults.cut_end(now).map(|end| self.epoch + Duration::from_secs_f64(end / 1000.0));
        let (tx, rx) = mpsc::channel();
        let (address, key, o) = (self.address.clone(), self.key.clone(), self.options.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(Mutex::new(false));
        let c = cancel.clone();
        let x = StartWith {
            host_key: self.host_key.clone(),
            relay_id: self.relay_id.clone(),
            claim: Some(Arc::new(move || !c.load(Ordering::Relaxed))),
            // (no cancel once the prove is sent: a welcome that comes then leaves with a bye, never parks the seat)
            cancel: None,
        };
        let got = self.taken;
        let handover = ended.clone();
        let stop = cancel.clone();
        let spawned = std::thread::Builder::new().name("mvs-net-resume".to_owned()).spawn(move || {
            if let Some(h) = hold {
                let until = h.saturating_duration_since(Instant::now());
                let step = Duration::from_millis(20);
                let mut slept = Duration::ZERO;
                while slept < until && !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(step.min(until - slept));
                    slept += step;
                }
            }
            let r = start(&address, &key, &o, &x, Some(got));
            let ended = handover.lock().unwrap_or_else(PoisonError::into_inner);
            match r {
                Ok(w) if *ended => bye(w),
                r => {
                    if let Err(mpsc::SendError(Ok(w))) = tx.send(r) {
                        bye(w);
                    }
                }
            }
            drop(ended);
        });
        match spawned {
            Ok(_) => self.attempt = Some(Attempt { rx, got, cancel, ended }),
            Err(e) => {
                eprintln!("relay: no second connection: {e}");
                self.next_try = now + RETRY_MS;
            }
        }
    }

    /// The second connection's welcome: it holds the seat. The sockets swapped, the handles named anew, channel 0
    /// resent from the relay's count, the calls held sent; a welcome with no seat kept, or a count the ring cannot
    /// cover, is the end.
    fn resumed(&mut self, w: Welcome, got: u64) {
        let resend = w.resumed.and_then(|n| self.ring.from(n));
        let ready = w.tcp.set_nonblocking(true).and_then(|()| w.tcp.set_read_timeout(None));
        let Some(resend) = resend.filter(|_| ready.is_ok()) else {
            let why = match (w.resumed, ready) {
                (_, Err(e)) => socket_why(&e),
                (None, _) => "the relay did not keep this player's place".to_owned(),
                (Some(_), _) => "the relay's count of this player's messages is past what this client kept".to_owned(),
            };
            // (a new seat: left with a bye, as this client signs out)
            bye(w);
            return self.lose(&why);
        };
        let now = self.now();
        let old = std::mem::replace(&mut self.w, w);
        let _ = old.tcp.shutdown(Shutdown::Both);
        self.down = None;
        // (taken on the old connection since the hello: the relay sends them again)
        self.skip = self.taken - got;
        self.out.clear();
        self.faults.swapped();
        self.out_handles.clear();
        self.in_handles.clear();
        self.in_by_id.clear();
        self.last_heard = now;
        self.last_ping = now;
        self.last_ack = f64::NEG_INFINITY;
        if self.match_heard.is_some() {
            self.match_heard = Some(now);
        }
        self.stall_ping = None;
        self.stats.resumes += 1;
        eprintln!("relay: back on a second connection, {} channel 0 frames sent again", resend.len());
        // (the frames the relay did not take go first, then those that waited)
        for (to, bytes) in resend.into_iter().rev() {
            self.pending_bytes += bytes.len();
            self.pending.push_front(Paced::Data { to, bytes });
        }
        // (a call sent on the old connection had its answer go there: it fails now as one not answered in time, as it
        // would at request_ms; those held, plain frames, go out on the new one sealed with its keys)
        let held = std::mem::take(&mut self.held);
        let lost: Vec<i64> = self.reqs.keys().copied().filter(|id| !held.iter().any(|h| h.0 == *id)).collect();
        for id in lost {
            if let Some(p) = self.reqs.shift_remove(&id) {
                self.answered(p, Err(RelayError::Timeout));
            }
        }
        for (_, f) in held {
            self.write(&f);
        }
        self.probe();
        for f in std::mem::take(&mut self.w.rest) {
            if self.gone.is_some() || self.down.is_some() {
                return;
            }
            self.frame(f.ty, &f.body);
        }
        self.drain();
    }
}

#[cfg(test)]
mod tests {
    use super::{RING_BYTES, Ring};

    #[test]
    fn the_ring_gives_back_what_the_relay_did_not_take() {
        let mut r = Ring::default();
        for k in 0..5u8 {
            r.sent("a".to_owned(), vec![k]);
        }
        assert_eq!(r.from(6), None, "past the count");
        let back = r.from(3).expect("held");
        assert_eq!(back.iter().map(|f| f.1[0]).collect::<Vec<_>>(), [3, 4]);
        r.sent("a".to_owned(), vec![5]);
        r.sent("c".to_owned(), vec![6]);
        r.closed("a");
        assert_eq!(r.from(3).map(|v| v.iter().map(|f| f.1[0]).collect::<Vec<_>>()), Some(vec![6]), "a's link closed");
        assert_eq!(r.from(3).map(|v| v.len()), Some(0), "the count set back");
        // (the oldest go past the ring's bytes)
        for _ in 0..3 {
            r.sent("b".to_owned(), vec![0; RING_BYTES / 2]);
        }
        assert_eq!(r.from(3), None, "no longer held");
        assert_eq!(r.from(5).map(|v| v.len()), Some(1));
    }
}
