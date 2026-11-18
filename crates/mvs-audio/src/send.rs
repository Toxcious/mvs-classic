//! The game side's way to the mixer: commands go over the ring in the order sent, and what the ring has no room for
//! waits here, in order, for a later frame. Nothing that stops, holds or lets go of a sound is ever lost: a post is
//! dropped (counted, its end given) when [`BACKLOG`] commands wait or, live, once it waited [`STALE`]; a value (an
//! emitter's place, a game parameter, the listener) gives way to a later one of its own with no post or music command
//! between them. The bank merge runs here too, off the mixer thread: a scope's hold or release, or a music event's
//! bank, gives a new [`BankView`] that goes to the mixer in its place among the commands (`Cmd::Banks`); live, on a
//! thread of its own (here from the moment that thread is gone), the commands after it waiting for it. The views sent
//! are kept here until the mixer lets them go, so that no view is ever freed on the mixer thread.

use std::collections::{HashSet, VecDeque};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::mpsc::{self, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mvs_data::sound::{EventBank, Id};

use crate::ctl::bank::{BankView, Banks};
use crate::{Cmd, GameObj, PostId, Stamped};

/// How often the warning of a full ring is printed at most.
const WARN_EVERY: Duration = Duration::from_secs(1);

/// Past this many commands waiting, a post is dropped (the rest never are: stops, scopes, the merges).
const BACKLOG: usize = 1 << 16;

/// Live, a post that waited this long for room on the ring or for a merge is dropped: no late burst of sounds after a
/// stall.
const STALE: Duration = Duration::from_secs(1);

/// The backlog is thinned (its stale posts and the values given way dropped) when it reaches this many commands, then
/// twice as many as the last thinning kept.
const THIN_AT: usize = 4096;

/// A command waiting for room on the ring (a post with the time it was sent, live), or the place of a bank merge not
/// done yet.
enum Queued {
    Cmd { s: Stamped, at: Option<Instant> },
    Banks { seq: u64, tick: u32 },
}

/// What a value command sets: a later one of the same, with no post or music command between, makes it moot.
#[derive(PartialEq, Eq, Hash)]
enum Value {
    Emitter(GameObj),
    Rtpc(Id, GameObj),
    Listener,
}

fn value(c: &Cmd) -> Option<Value> {
    match *c {
        Cmd::SetEmitter { obj, .. } => Some(Value::Emitter(obj)),
        Cmd::SetRtpc { id, obj, .. } => Some(Value::Rtpc(id, obj)),
        Cmd::SetListener(_) => Some(Value::Listener),
        _ => None,
    }
}

/// What the bank thread is asked, in order.
enum Op {
    Hold {
        scope: u64,
        banks: Vec<(String, Arc<EventBank>)>,
    },
    Unhold {
        scope: u64,
    },
    /// A post of an event (a loose bank's is a use of it).
    Touch(Id),
    /// A view the mixer is done with, dropped there.
    Drop(#[allow(dead_code, reason = "held only to be dropped on the bank thread")] Arc<BankView>),
    /// A merge that fails (the checks).
    #[cfg(test)]
    Fail,
}

/// The merge: here (offline and the checks, in order with the calls) or on the bank thread (live); gone when a merge
/// failed on both (the mixer keeps the last view it had).
enum Merge {
    Here(Box<Banks>),
    Thread(MergeThread),
    Gone,
}

/// A hold the bank thread was given, still standing (its scope and banks): what a merge here starts from when that
/// thread is gone.
type Given = (u64, Vec<(String, Arc<EventBank>)>);

/// The holder of the music events' banks (no scope's name hash has the top bit: `crate::mix::media::music_owner`).
const MUSIC_HOLDER: u64 = 1 << 63;

struct MergeThread {
    ops: Option<mpsc::Sender<Op>>,
    /// A view per merge op in order (None: the merge did not move).
    done: mpsc::Receiver<Option<Arc<BankView>>>,
    /// The ops sent and the views taken, by count.
    sent: u64,
    taken: u64,
    thread: Option<JoinHandle<()>>,
    given: Vec<Given>,
}

impl Merge {
    /// Sends an op; the place of its view among the commands when it gives one later (the bank thread's).
    fn op(&mut self, op: Op) -> Option<u64> {
        match self {
            Merge::Here(b) => {
                apply(b, op);
                None
            }
            Merge::Thread(t) => {
                match &op {
                    Op::Hold { scope, banks } => {
                        t.given.retain(|(s, _)| s != scope);
                        t.given.push((*scope, banks.clone()));
                    }
                    Op::Unhold { scope } => t.given.retain(|(s, _)| s != scope),
                    _ => {}
                }
                let counted = !matches!(op, Op::Touch(_) | Op::Drop(_));
                if let Some(tx) = &t.ops {
                    let _ = tx.send(op);
                }
                counted.then(|| {
                    t.sent += 1;
                    t.sent
                })
            }
            Merge::Gone => None,
        }
    }
}

/// Applies an op; the new view for a hold, a release or an add that moved the merge.
fn apply(b: &mut Banks, op: Op) -> Option<Arc<BankView>> {
    match op {
        Op::Hold { scope, banks } => b.hold(scope, banks),
        Op::Unhold { scope } => {
            if !b.unhold(scope) {
                return None;
            }
        }
        Op::Touch(ev) => {
            b.touch(ev);
            return None;
        }
        Op::Drop(_) => return None,
        #[cfg(test)]
        Op::Fail => panic!("a failed merge"),
    }
    Some(Arc::new(b.view()))
}

/// The game side's sender of commands.
pub(crate) struct Sender {
    tx: rtrb::Producer<Stamped>,
    backlog: VecDeque<Queued>,
    merge: Merge,
    live: bool,
    /// The backlog's length at which it is thinned next.
    thin_at: usize,
    /// The views sent to the mixer, each dropped here once the mixer let it go.
    views: Vec<Arc<BankView>>,
    /// The music events' banks held so far ([`MUSIC_HOLDER`]).
    music: Vec<(String, Arc<EventBank>)>,
    /// Posts dropped (too many commands waiting, or a post waiting too long) and their ids, for their ends.
    pub(crate) dropped: u32,
    pub(crate) ends: Vec<PostId>,
    /// The ring was full at the last flush; the warning's last print and the drops it told.
    full: bool,
    warned_at: Option<Instant>,
    warned: u32,
}

impl Sender {
    /// Live: the merge on a thread of its own; else here, at once.
    pub(crate) fn new(tx: rtrb::Producer<Stamped>, live: bool) -> Sender {
        let merge = if live {
            let (ops, rx) = mpsc::channel::<Op>();
            let (done_tx, done) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .name("audio-banks".into())
                .spawn(move || {
                    let mut b = Banks::default();
                    while let Ok(op) = rx.recv() {
                        let counted = !matches!(op, Op::Touch(_) | Op::Drop(_));
                        let v = apply(&mut b, op);
                        if counted && done_tx.send(v).is_err() {
                            return;
                        }
                    }
                })
                .expect("audio: the bank thread starts");
            Merge::Thread(MergeThread {
                ops: Some(ops),
                done,
                sent: 0,
                taken: 0,
                thread: Some(thread),
                given: Vec::new(),
            })
        } else {
            Merge::Here(Box::default())
        };
        Sender {
            tx,
            backlog: VecDeque::new(),
            merge,
            live,
            thin_at: THIN_AT,
            views: Vec::new(),
            music: Vec::new(),
            dropped: 0,
            ends: Vec::new(),
            full: false,
            warned_at: None,
            warned: 0,
        }
    }

    /// Sends a command, or queues it behind those waiting.
    pub(crate) fn send(&mut self, tick: u32, cmd: Cmd) {
        let mut at = None;
        if let Cmd::Post(p) = &cmd {
            // (a loose bank's use counts at the post, in order with the holds and releases)
            self.merge.op(Op::Touch(p.event));
            if self.backlog.len() >= BACKLOG {
                self.drop_post(p.id);
                self.warn();
                return;
            }
            at = self.live.then(Instant::now);
        }
        self.backlog.push_back(Queued::Cmd { s: Stamped { tick, cmd }, at });
        if self.backlog.len() >= self.thin_at {
            self.thin();
        }
        self.flush();
    }

    fn drop_post(&mut self, id: PostId) {
        self.dropped += 1;
        self.ends.push(id);
    }

    /// A merge op: its view takes its place among the commands.
    fn merge(&mut self, tick: u32, op: Op) {
        match &mut self.merge {
            Merge::Here(b) => {
                if let Some(v) = apply(b, op) {
                    self.backlog.push_back(Queued::Cmd { s: Stamped { tick, cmd: Cmd::Banks(v) }, at: None });
                }
            }
            m => {
                if let Some(seq) = m.op(op) {
                    self.backlog.push_back(Queued::Banks { seq, tick });
                }
            }
        }
        self.flush();
    }

    /// A scope holds these banks.
    pub(crate) fn hold(&mut self, tick: u32, scope: u64, banks: Vec<(String, Arc<EventBank>)>) {
        self.merge(tick, Op::Hold { scope, banks });
    }

    /// A scope lets its banks go.
    pub(crate) fn unhold(&mut self, tick: u32, scope: u64) {
        self.merge(tick, Op::Unhold { scope });
    }

    /// A music event's banks, held from now on with those of the music events before it:
    /// never loose, so a scope let go does not take the music's hierarchy out
    /// from under it.
    pub(crate) fn hold_music(&mut self, tick: u32, banks: Vec<(String, Arc<EventBank>)>) {
        let before = self.music.len();
        for (n, b) in banks {
            if !self.music.iter().any(|(m, _)| *m == n) {
                self.music.push((n, b));
            }
        }
        if self.music.len() > before {
            self.merge(tick, Op::Hold { scope: MUSIC_HOLDER, banks: self.music.clone() });
        }
    }

    /// Commands waiting for room on the ring or for a merge.
    pub(crate) fn waiting(&self) -> usize {
        self.backlog.len()
    }

    /// Sends what waits, in order, as far as the ring has room and the merges are done; once a frame. A post that
    /// waited too long is dropped; the views the mixer let go are dropped.
    pub(crate) fn flush(&mut self) {
        self.full = false;
        let mut now = None;
        while let Some(front) = self.backlog.front_mut() {
            match front {
                &mut Queued::Banks { seq, tick } => {
                    let Merge::Thread(t) = &mut self.merge else { unreachable!("a merge waits on the bank thread") };
                    debug_assert_eq!(seq, t.taken + 1);
                    match t.done.try_recv() {
                        Ok(v) => {
                            t.taken += 1;
                            match v {
                                Some(v) => *front = Queued::Cmd { s: Stamped { tick, cmd: Cmd::Banks(v) }, at: None },
                                None => {
                                    self.backlog.pop_front();
                                }
                            }
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => self.merge_here(),
                    }
                }
                Queued::Cmd { s, at } => {
                    if let Cmd::Post(p) = &s.cmd
                        && let Some(a) = at
                        && now.get_or_insert_with(Instant::now).duration_since(*a) >= STALE
                    {
                        let id = p.id;
                        self.backlog.pop_front();
                        self.drop_post(id);
                        continue;
                    }
                    if self.tx.is_full() {
                        self.full = true;
                        break;
                    }
                    let Some(Queued::Cmd { s, .. }) = self.backlog.pop_front() else {
                        unreachable!("the front is a command")
                    };
                    if let Cmd::Banks(v) = &s.cmd {
                        self.views.push(v.clone());
                    }
                    if self.tx.push(s).is_err() {
                        unreachable!("the ring had room");
                    }
                }
            }
        }
        if self.backlog.is_empty() {
            self.thin_at = THIN_AT;
        }
        self.let_go();
        self.warn();
    }

    /// Drops the stale posts and the values given way from the backlog (a stall of the mixer: the backlog's growth
    /// bounded by the posts of the last [`STALE`] and the commands that stop and hold).
    fn thin(&mut self) {
        let now = Instant::now();
        let mut seen = HashSet::new();
        let mut stale = Vec::new();
        let mut kept = VecDeque::with_capacity(self.backlog.len());
        while let Some(q) = self.backlog.pop_back() {
            if let Queued::Cmd { s, at } = &q {
                if let Cmd::Post(p) = &s.cmd
                    && at.is_some_and(|a| now.duration_since(a) >= STALE)
                {
                    stale.push(p.id);
                    continue;
                }
                match value(&s.cmd) {
                    Some(k) => {
                        if !seen.insert(k) {
                            continue;
                        }
                    }
                    None if matches!(s.cmd, Cmd::Post(_) | Cmd::Music(_)) => seen.clear(),
                    None => {}
                }
            }
            kept.push_front(q);
        }
        for id in stale.into_iter().rev() {
            self.drop_post(id);
        }
        self.backlog = kept;
        self.thin_at = (2 * self.backlog.len()).max(THIN_AT);
    }

    /// The bank thread is gone (a merge failed there): merges here from now on, from the holds it was given
    /// that still stand (the loose banks it kept are lost). The first merge waiting gets the view that makes; those
    /// after it were in it, and go.
    fn merge_here(&mut self) {
        let Merge::Thread(t) = std::mem::replace(&mut self.merge, Merge::Gone) else { return };
        eprintln!("audio: the bank thread stopped: the banks are merged on the game thread from now on");
        let given = t.given;
        let built = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut b = Banks::default();
            for (scope, banks) in given {
                b.hold(scope, banks);
            }
            let v = Arc::new(b.view());
            (Box::new(b), v)
        }));
        let mut view = match built {
            Ok((b, v)) => {
                self.merge = Merge::Here(b);
                Some(v)
            }
            Err(_) => {
                eprintln!("audio: the bank merge failed on the game thread too: the mixer keeps the banks it has");
                None
            }
        };
        self.backlog.retain_mut(|q| {
            let Queued::Banks { tick, .. } = *q else { return true };
            match view.take() {
                Some(v) => {
                    *q = Queued::Cmd { s: Stamped { tick, cmd: Cmd::Banks(v) }, at: None };
                    true
                }
                None => false,
            }
        });
    }

    /// Drops the views the mixer let go (the bank thread's job when there is one).
    fn let_go(&mut self) {
        let mut k = 0;
        while k < self.views.len() {
            if Arc::strong_count(&self.views[k]) == 1 {
                let v = self.views.swap_remove(k);
                self.merge.op(Op::Drop(v));
            } else {
                k += 1;
            }
        }
    }

    /// One line a second at most while commands wait for room on the ring, with the posts dropped since the last.
    fn warn(&mut self) {
        if !self.full && self.dropped == self.warned {
            return;
        }
        if self.warned_at.is_some_and(|t| t.elapsed() < WARN_EVERY) {
            return;
        }
        self.warned_at = Some(Instant::now());
        eprintln!(
            "audio: the mixer's command ring is full: {} commands wait, {} posts dropped ({} in all)",
            self.backlog.len(),
            self.dropped - self.warned,
            self.dropped
        );
        self.warned = self.dropped;
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        if let Merge::Thread(t) = &mut self.merge {
            t.ops = None;
            if let Some(h) = t.thread.take() {
                let _ = h.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GameObj, Post};

    fn post(id: u32, event: Id) -> Cmd {
        Cmd::Post(Post { id: PostId(id), event, obj: Some(GameObj::GLOBAL), key: None, stop_key: None, at: None })
    }

    fn bank(id: Id, act: Id) -> Vec<(String, Arc<EventBank>)> {
        let b = EventBank { name: format!("e{id}"), id, actions: vec![act], ..EventBank::default() };
        vec![(format!("e{id}"), Arc::new(b))]
    }

    /// Flushes until `n` commands came over the ring.
    fn take(s: &mut Sender, rx: &mut rtrb::Consumer<Stamped>, n: usize) -> Vec<Stamped> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while got.len() < n {
            assert!(Instant::now() < deadline, "the merge never came: {got:?}");
            s.flush();
            got.extend(std::iter::from_fn(|| rx.pop().ok()));
            std::thread::yield_now();
        }
        got
    }

    /// Live: a scope's merge goes on the bank thread and the post sent after it waits for it, in order.
    #[test]
    fn live_merge_keeps_the_order() {
        let (tx, mut rx) = rtrb::RingBuffer::new(16);
        let mut s = Sender::new(tx, true);
        s.hold(1, 3, bank(5, 9));
        s.send(2, post(1, 5));
        let got = take(&mut s, &mut rx, 2);
        let Cmd::Banks(v) = &got[0].cmd else { panic!("{:?}", got[0]) };
        assert_eq!(v.event(5), Some(&[9][..]));
        assert!(matches!(got[1].cmd, Cmd::Post(_)));
        // a release of a scope that held nothing moves nothing
        s.unhold(3, 99);
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.waiting() > 0 {
            assert!(Instant::now() < deadline);
            s.flush();
        }
        assert!(rx.pop().is_err());
    }

    /// A session's order (the menus' music preload, a 4-fighter match's scope, then the Lab's scope in its place),
    /// live and offline: the match's banks let go turn loose past LOOSE_BANKS, and the music event's bank, loaded
    /// first, stays in the merge (held, not loose); then the music's next scope adds no merge.
    #[test]
    fn the_music_bank_outlives_a_big_scope_let_go() {
        use crate::ctl::bank::LOOSE_BANKS;
        for live in [false, true] {
            let (tx, mut rx) = rtrb::RingBuffer::new(64);
            let mut s = Sender::new(tx, live);
            const MATCH: u64 = 3;
            s.hold_music(0, bank(1, 2));
            let big: Vec<_> = (0..LOOSE_BANKS as u32 + 50).flat_map(|k| bank(100 + k, 7)).collect();
            s.hold(1, MATCH, big);
            s.hold(2, MATCH, bank(10, 11));
            s.send(3, post(1, 1));
            let got = take(&mut s, &mut rx, 4);
            let Cmd::Banks(v) = &got[2].cmd else { panic!("{:?}", got[2]) };
            assert_eq!(v.event(1), Some(&[2][..]), "live {live}: the music event's bank left the merge");
            assert_eq!(v.event(10), Some(&[11][..]));
            assert!(v.event(100).is_none(), "live {live}: the oldest loose bank stayed");
            assert!(matches!(got[3].cmd, Cmd::Post(_)));
            // (the music event's bank again: held already, no merge)
            s.hold_music(4, bank(1, 2));
            assert_eq!(s.waiting(), 0);
            s.flush();
            assert!(rx.pop().is_err());
        }
    }

    /// Live: a stop and a release sent behind a merge not done yet wait for it and keep their order.
    #[test]
    fn a_stop_behind_a_merge_is_kept() {
        let (tx, mut rx) = rtrb::RingBuffer::new(16);
        let mut s = Sender::new(tx, true);
        s.hold(1, 3, bank(5, 9));
        s.send(1, Cmd::StopAll { fade: 0.0, keep: None });
        s.send(1, Cmd::StopKey { key: crate::Key::new("k"), fade: 0.0, curve: None });
        s.unhold(2, 3);
        s.send(2, Cmd::Unscope { name: 3 });
        let got = take(&mut s, &mut rx, 5);
        let Cmd::Banks(held) = &got[0].cmd else { panic!("{:?}", got[0]) };
        assert!(held.event(5).is_some());
        assert!(matches!(got[1].cmd, Cmd::StopAll { .. }));
        assert!(matches!(got[2].cmd, Cmd::StopKey { .. }));
        let Cmd::Banks(after) = &got[3].cmd else { panic!("{:?}", got[3]) };
        // (released, the bank is loose: still loaded)
        assert!(after.event(5).is_some());
        assert!(matches!(got[4].cmd, Cmd::Unscope { .. }));
        assert_eq!(s.waiting(), 0);
    }

    /// Live: the bank thread failing a merge does not hold the commands: the merge goes on here.
    #[test]
    fn a_failed_bank_thread_merges_here() {
        let (tx, mut rx) = rtrb::RingBuffer::new(16);
        let mut s = Sender::new(tx, true);
        s.hold(1, 3, bank(5, 9));
        s.merge(1, Op::Fail);
        s.send(2, post(1, 5));
        let got = take(&mut s, &mut rx, 3);
        assert!(matches!(got[0].cmd, Cmd::Banks(_)));
        let Cmd::Banks(v) = &got[1].cmd else { panic!("{:?}", got[1]) };
        assert_eq!(v.event(5), Some(&[9][..]));
        assert!(matches!(got[2].cmd, Cmd::Post(_)));
        assert!(matches!(s.merge, Merge::Here(_)));
        s.hold(3, 4, bank(6, 8));
        let got = take(&mut s, &mut rx, 1);
        let Cmd::Banks(v) = &got[0].cmd else { panic!("{:?}", got[0]) };
        assert_eq!(v.event(6), Some(&[8][..]));
    }

    /// The mixer not reading: values give way to later ones of their own, but not across a post; nothing else goes.
    #[test]
    fn a_stalled_mixer_keeps_the_latest_values() {
        let (tx, mut rx) = rtrb::RingBuffer::new(4);
        let mut s = Sender::new(tx, false);
        let at = |x: f64| Cmd::SetEmitter { obj: GameObj(1), pos: [x, 0.0, 0.0], front: [1.0, 0.0, 0.0] };
        s.send(0, at(-1.0));
        s.send(0, post(1, 5));
        for k in 0..2 * THIN_AT {
            s.send(0, at(k as f64));
        }
        s.send(0, Cmd::StopAll { fade: 0.0, keep: None });
        assert!(s.waiting() < THIN_AT, "{} wait", s.waiting());
        let mut got = Vec::new();
        while s.waiting() > 0 || !rx.is_empty() {
            got.extend(std::iter::from_fn(|| rx.pop().ok()));
            s.flush();
        }
        let xs: Vec<f64> = got
            .iter()
            .filter_map(|c| match c.cmd {
                Cmd::SetEmitter { pos, .. } => Some(pos[0]),
                _ => None,
            })
            .collect();
        assert_eq!(xs.first(), Some(&-1.0));
        assert_eq!(xs.last(), Some(&((2 * THIN_AT - 1) as f64)));
        assert!(matches!(got[1].cmd, Cmd::Post(_)));
        assert!(matches!(got.last().map(|c| &c.cmd), Some(Cmd::StopAll { .. })));
        assert_eq!(s.dropped, 0);
    }
}
