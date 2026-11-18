//! The faults a relay client puts on its own link ([`NetFaults`], the dev switch): what is held, when it goes, what
//! is dropped. The net thread asks before each read and write; its random draws come from a generator seeded by the
//! player's id, the same draws for the same key.

use std::collections::VecDeque;
use std::net::SocketAddr;

use super::NetFaults;

pub(super) struct Faults {
    f: NetFaults,
    rng: u64,
    /// The online match's go (ms of the client's clock): the cut counts from it.
    pub(super) go: Option<f64>,
    /// TCP bytes held either way (in order), and datagrams (each with its own time).
    pub(super) tcp_out: VecDeque<(f64, Vec<u8>)>,
    pub(super) tcp_in: VecDeque<(f64, Vec<u8>)>,
    pub(super) udp_out: Vec<(f64, Vec<u8>)>,
    pub(super) udp_in: Vec<(f64, Vec<u8>, SocketAddr)>,
}

/// A fault's seconds and milliseconds at the most (a cut, its start, a delay, a jitter): past them, this long.
const MOST_S: f64 = 3600.0;

/// A fault's number within [0, `most`] (not a number: 0).
fn bounded(x: f64, most: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x.clamp(0.0, most) }
}

/// FNV-1a of the id: the generator's seed (never 0, which xorshift keeps at 0).
fn seed_of(id: &str) -> u64 {
    let h = id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    h | 1
}

impl Faults {
    pub(super) fn new(f: NetFaults, id: &str) -> Faults {
        let f = NetFaults {
            cut_s: bounded(f.cut_s, MOST_S),
            cut_at_s: bounded(f.cut_at_s, MOST_S),
            loss: bounded(f.loss, 1.0),
            delay_ms: bounded(f.delay_ms, MOST_S * 1000.0),
            jitter_ms: bounded(f.jitter_ms, MOST_S * 1000.0),
        };
        Faults {
            f,
            rng: seed_of(id),
            go: None,
            tcp_out: VecDeque::new(),
            tcp_in: VecDeque::new(),
            udp_out: Vec::new(),
            udp_in: Vec::new(),
        }
    }

    /// A draw in [0, 1) (xorshift64*).
    fn draw(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        #[allow(clippy::cast_precision_loss, reason = "53 bits of the draw")]
        let d = (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64;
        d / (1u64 << 53) as f64
    }

    fn delays(&self) -> bool {
        self.f.delay_ms > 0.0 || self.f.jitter_ms > 0.0
    }

    /// One way's wait: the delay and up to the jitter.
    fn wait(&mut self) -> f64 {
        let j = if self.f.jitter_ms > 0.0 { self.draw() * self.f.jitter_ms } else { 0.0 };
        self.f.delay_ms + j
    }

    /// The cut's span (ms of the client's clock), once the match has gone.
    fn cut_span(&self) -> Option<(f64, f64)> {
        let go = self.go?;
        (self.f.cut_s > 0.0).then(|| {
            let from = go + self.f.cut_at_s * 1000.0;
            (from, from + self.f.cut_s * 1000.0)
        })
    }

    /// The link is cut now.
    pub(super) fn cut(&self, now: f64) -> bool {
        self.cut_span().is_some_and(|(a, b)| now >= a && now < b)
    }

    /// The end of the cut under way now.
    pub(super) fn cut_end(&self, now: f64) -> Option<f64> {
        self.cut_span().filter(|(a, b)| now >= *a && now < *b).map(|(_, b)| b)
    }

    /// Nothing held and no fault that holds or drops (a cut's start alone is none).
    pub(super) fn idle(&self) -> bool {
        self.f.cut_s == 0.0
            && self.f.loss == 0.0
            && !self.delays()
            && self.tcp_out.is_empty()
            && self.tcp_in.is_empty()
            && self.udp_out.is_empty()
            && self.udp_in.is_empty()
    }

    /// When TCP bytes written now go (None: at once), in order behind those held.
    pub(super) fn tcp_wait(&mut self, now: f64) -> Option<f64> {
        if !self.delays() && self.tcp_out.is_empty() {
            return None;
        }
        let at = now + self.wait();
        Some(self.tcp_out.back().map_or(at, |b| b.0.max(at)))
    }

    /// When TCP bytes read now are taken (None: at once), in order behind those held.
    pub(super) fn tcp_wait_in(&mut self, now: f64) -> Option<f64> {
        if !self.delays() && self.tcp_in.is_empty() {
            return None;
        }
        let at = now + self.wait();
        Some(self.tcp_in.back().map_or(at, |b| b.0.max(at)))
    }

    /// When a datagram sent or taken now goes on: None when it is dropped (a cut, a loss), `now` at once.
    pub(super) fn udp_wait(&mut self, now: f64) -> Option<f64> {
        if self.cut(now) || (self.f.loss > 0.0 && self.draw() < self.f.loss) {
            return None;
        }
        Some(if self.delays() { now + self.wait() } else { now })
    }

    /// The datagrams to the relay that are due (those due in a cut are dropped).
    pub(super) fn due_out(&mut self, now: f64) -> Vec<Vec<u8>> {
        let cut = self.cut(now);
        let (due, held): (Vec<_>, Vec<_>) = std::mem::take(&mut self.udp_out).into_iter().partition(|d| d.0 <= now);
        self.udp_out = held;
        if cut { Vec::new() } else { due.into_iter().map(|d| d.1).collect() }
    }

    /// The datagrams from the relay that are due, in their times' order (those due in a cut are dropped).
    pub(super) fn due_in(&mut self, now: f64) -> Vec<(Vec<u8>, SocketAddr)> {
        let cut = self.cut(now);
        let (mut due, held): (Vec<_>, Vec<_>) = std::mem::take(&mut self.udp_in).into_iter().partition(|d| d.0 <= now);
        self.udp_in = held;
        if cut {
            return Vec::new();
        }
        due.sort_by(|a, b| a.0.total_cmp(&b.0));
        due.into_iter().map(|d| (d.1, d.2)).collect()
    }

    /// A connection swapped for a new one: what the old one held is gone with it, its datagrams too (sealed with its
    /// keys and token, of no use on the new one).
    pub(super) fn swapped(&mut self) {
        self.tcp_out.clear();
        self.tcp_in.clear();
        self.udp_out.clear();
        self.udp_in.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{Faults, NetFaults};

    #[test]
    fn the_cut_counts_from_the_go() {
        let mut f = Faults::new(NetFaults { cut_s: 2.0, cut_at_s: 1.0, ..NetFaults::default() }, "r1");
        assert!(!f.cut(1500.0), "no go yet: no cut");
        f.go = Some(1000.0);
        assert!(!f.cut(1999.0) && f.cut(2000.0) && f.cut(3999.0) && !f.cut(4000.0));
        assert_eq!(f.cut_end(2500.0), Some(4000.0));
        assert_eq!(f.udp_wait(2500.0), None, "a datagram in the cut is dropped");
    }

    #[test]
    fn loss_delay_and_jitter_are_drawn_the_same_for_one_id() {
        let fs = NetFaults { loss: 0.25, delay_ms: 40.0, jitter_ms: 20.0, ..NetFaults::default() };
        let run = || {
            let mut f = Faults::new(fs, "rabc");
            (0..2000).map(|_| f.udp_wait(0.0)).collect::<Vec<_>>()
        };
        let a = run();
        assert_eq!(a, run(), "the same draws for the same id");
        let lost = a.iter().filter(|w| w.is_none()).count();
        assert!((400..600).contains(&lost), "a quarter lost: {lost}");
        assert!(a.iter().flatten().all(|w| (40.0..60.0).contains(w)), "delay plus up to the jitter");
    }

    #[test]
    fn a_cut_start_alone_is_idle_and_no_fault_is_unbounded() {
        assert!(Faults::new(NetFaults { cut_at_s: 10.0, ..NetFaults::default() }, "r3").idle());
        let mut f = Faults::new(NetFaults { cut_s: f64::INFINITY, cut_at_s: f64::NAN, ..NetFaults::default() }, "r4");
        assert!(!f.idle());
        f.go = Some(0.0);
        assert_eq!(f.cut_end(1.0), Some(3_600_000.0), "a cut of an hour from the go");
    }

    #[test]
    fn tcp_bytes_keep_their_order() {
        let mut f = Faults::new(NetFaults { delay_ms: 50.0, jitter_ms: 30.0, ..NetFaults::default() }, "r2");
        let mut last = 0.0;
        for k in 0..100 {
            let at = f.tcp_wait(f64::from(k)).expect("held");
            assert!(at >= last && at >= f64::from(k) + 50.0);
            f.tcp_out.push_back((at, Vec::new()));
            last = at;
        }
    }
}
