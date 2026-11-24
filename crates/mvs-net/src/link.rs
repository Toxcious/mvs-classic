//! The links that carry a match's packets between a peer's session and the match's referee: the [`MatchLink`] a
//! session's caller talks to, and [`SimNet`], an in-memory network with a referee on it for the tests (latency, jitter,
//! loss, a cut, a mute or deaf peer). The relay client on its net thread is the other link.

use mvs_protocol::referee::{Referee, RefereeOut, SlotNews};

pub mod relay;

/// One peer's way to the match's referee (`SessionConfig.send` and the packets that reach `Session.receive`).
pub trait MatchLink {
    /// A packet to the referee.
    fn send(&mut self, bytes: &[u8], now: f64);
    /// The next packet from the referee that has come by `now`, if any.
    fn recv(&mut self, now: f64) -> Option<Vec<u8>>;
}

/// One way of a simulated link (a peer to the referee, or back).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Way {
    /// ms a packet takes at the least.
    pub latency: f64,
    /// ms added at random, uniform in [0, jitter): packets can pass each other.
    pub jitter: f64,
    /// The share of packets lost, 0..1.
    pub loss: f64,
    /// Nothing gets through (a cut, a mute peer's way up, a deaf peer's way down): what is sent meanwhile waits and
    /// arrives once the way is open again, if it ever is.
    pub cut: bool,
}

impl Way {
    pub const PERFECT: Way = Way { latency: 0.0, jitter: 0.0, loss: 0.0, cut: false };
}

/// A peer's two ways.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ways {
    /// The peer to the referee.
    pub up: Way,
    /// The referee to the peer.
    pub down: Way,
}

/// A packet on its way, due at `at` ms.
#[derive(Clone, Debug)]
struct InFlight {
    at: f64,
    /// The order it was sent in (packets due at the same ms keep it).
    seq: u64,
    bytes: Vec<u8>,
    /// Sent while its way was cut: it waits for the way to open.
    held: bool,
}

/// What the referee sent in one call: the packets by slot, and its news of the slots.
#[derive(Default)]
struct Sent {
    packets: Vec<(usize, Vec<u8>)>,
    news: Vec<(usize, SlotNews)>,
}

impl RefereeOut for Sent {
    fn send(&mut self, slot: usize, bytes: &[u8]) {
        self.packets.push((slot, bytes.to_vec()));
    }

    fn news(&mut self, slot: usize, n: SlotNews) {
        self.news.push((slot, n));
    }
}

/// An in-memory network: a [`Referee`] for the match's humans and, per human slot, its ways to and from it. Time is
/// the callers' (ms): [`SimNet::pump`] delivers what is due to the referee, pumps it, and puts what it sends on the
/// ways down; a peer's [`SimLink`] sends up and takes what has come down. The randomness (jitter, loss) is a seeded
/// stream of its own, so a run repeats.
#[derive(Debug)]
pub struct SimNet {
    referee: Referee,
    /// By slot: its ways, what is on its way up and down.
    ways: Vec<Option<Ways>>,
    up: Vec<Vec<InFlight>>,
    down: Vec<Vec<InFlight>>,
    seq: u64,
    rng: u64,
    /// The referee's news of the slots, in order.
    news: Vec<(usize, SlotNews)>,
    /// Every packet the referee sent, by slot, before the ways (`log_sent`).
    log: Option<Vec<(usize, Vec<u8>)>>,
}

impl SimNet {
    /// A network for a match of `slots` slots and its human slots `humans` (each with `ways`), its referee's tick 0 at
    /// `tick0` ms, its randomness from `seed`.
    pub fn new(slots: usize, humans: &[usize], ways: Ways, tick0: f64, seed: u64) -> SimNet {
        let referee = Referee::new(slots, humans, tick0);
        let n = referee.match_slots();
        let mut by_slot = vec![None; n];
        for &h in humans {
            if let Some(w) = by_slot.get_mut(h) {
                *w = Some(ways);
            }
        }
        SimNet {
            referee,
            ways: by_slot,
            up: vec![Vec::new(); n],
            down: vec![Vec::new(); n],
            seq: 0,
            rng: seed ^ 0x9e37_79b9_7f4a_7c15,
            news: Vec::new(),
            log: None,
        }
    }

    /// The ways of one slot from now on (what is already on its way keeps its time; what waits on a way that opens
    /// arrives now).
    pub fn set_ways(&mut self, slot: usize, ways: Ways) {
        let Some(w) = self.ways.get_mut(slot) else { return };
        *w = Some(ways);
        if !ways.up.cut {
            self.up[slot].iter_mut().for_each(|p| p.held = false);
        }
        if !ways.down.cut {
            self.down[slot].iter_mut().for_each(|p| p.held = false);
        }
    }

    /// Delivers to the referee what has come up by `now`, then pumps it: what it sends goes on the ways down.
    pub fn pump(&mut self, now: f64) {
        let mut sent = Sent::default();
        self.deliver_into(now, &mut sent);
        self.referee.pump(now, &mut sent);
        self.sent(sent, now);
    }

    /// Delivers to the referee what has come up by `now`, without a pump (a referee's owner that takes packets as
    /// they come and pumps on its own beat): what it sends (checksums passed on) goes on the ways down.
    pub fn deliver(&mut self, now: f64) {
        let mut sent = Sent::default();
        self.deliver_into(now, &mut sent);
        self.sent(sent, now);
    }

    /// Delivers to the referee what has come up by `now`, a pump after each packet (as the relay's core and an app's
    /// referee pump after every packet they take): what it sends goes on the ways down.
    pub fn deliver_pumped(&mut self, now: f64) {
        while let Some((s, i)) = self.next_up(now) {
            let mut sent = Sent::default();
            let p = self.up[s].remove(i);
            self.referee.receive(s, &p.bytes, now, &mut sent);
            self.referee.pump(now, &mut sent);
            self.sent(sent, now);
        }
    }

    fn deliver_into(&mut self, now: f64, sent: &mut Sent) {
        while let Some((s, i)) = self.next_up(now) {
            let p = self.up[s].remove(i);
            self.referee.receive(s, &p.bytes, now, sent);
        }
    }

    /// The earliest due by `now` of every slot's way up, in the order they arrive: its slot and index.
    fn next_up(&self, now: f64) -> Option<(usize, usize)> {
        (0..self.up.len())
            .filter_map(|s| due(&self.up[s], now).map(|i| (s, i)))
            .min_by(|a, b| order(&self.up[a.0][a.1], &self.up[b.0][b.1]))
    }

    /// What the referee sent onto the ways down.
    fn sent(&mut self, sent: Sent, now: f64) {
        self.news.extend(&sent.news);
        for (slot, bytes) in sent.packets {
            if let Some(log) = &mut self.log {
                log.push((slot, bytes.clone()));
            }
            self.put(slot, false, bytes, now);
        }
    }

    /// A packet onto a slot's way (`up`: towards the referee), lost or due after the way's time.
    fn put(&mut self, slot: usize, up: bool, bytes: Vec<u8>, now: f64) {
        let Some(ways) = self.ways.get(slot).copied().flatten() else { return };
        let way = if up { ways.up } else { ways.down };
        if way.loss > 0.0 && self.random() < way.loss {
            return;
        }
        let at = now + way.latency + self.random() * way.jitter;
        self.seq += 1;
        let p = InFlight { at, seq: self.seq, bytes, held: way.cut };
        let way = if up { &mut self.up } else { &mut self.down };
        way[slot].push(p);
    }

    /// A uniform draw in [0, 1) (splitmix64).
    #[allow(clippy::cast_precision_loss, reason = "53 bits of the draw")]
    fn random(&mut self) -> f64 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    /// The slot's player left (`Referee::left`).
    pub fn left(&mut self, slot: usize, now: f64) {
        self.referee.left(slot, now);
    }

    /// The referee's news of the slots so far (away, back, gone), in order.
    pub fn news(&self) -> &[(usize, SlotNews)] {
        &self.news
    }

    pub fn referee(&self) -> &Referee {
        &self.referee
    }

    /// From now on, keep every packet the referee sends (before the ways lose or hold it): the checks' record of the
    /// ruled streams.
    pub fn log_sent(&mut self) {
        self.log.get_or_insert_with(Vec::new);
    }

    /// The packets the referee sent since the last call, by slot, in order (`log_sent`).
    pub fn take_sent(&mut self) -> Vec<(usize, Vec<u8>)> {
        self.log.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// Slot `slot`'s end of the network.
    pub fn link(&mut self, slot: usize) -> SimLink<'_> {
        SimLink { net: self, slot }
    }
}

/// The order packets arrive in: by time, then as sent.
fn order(a: &InFlight, b: &InFlight) -> std::cmp::Ordering {
    a.at.total_cmp(&b.at).then(a.seq.cmp(&b.seq))
}

/// The index of the earliest packet of a way that has come by `now`.
fn due(way: &[InFlight], now: f64) -> Option<usize> {
    way.iter().enumerate().filter(|(_, p)| !p.held && p.at <= now).min_by(|a, b| order(a.1, b.1)).map(|(i, _)| i)
}

/// One peer's end of a [`SimNet`].
#[derive(Debug)]
pub struct SimLink<'a> {
    net: &'a mut SimNet,
    slot: usize,
}

impl MatchLink for SimLink<'_> {
    fn send(&mut self, bytes: &[u8], now: f64) {
        self.net.put(self.slot, true, bytes.to_vec(), now);
    }

    fn recv(&mut self, now: f64) -> Option<Vec<u8>> {
        let way = self.net.down.get_mut(self.slot)?;
        let i = due(way, now)?;
        Some(way.remove(i).bytes)
    }
}
