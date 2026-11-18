//! The core's deadlines (a clock, steady repeats, a manual clock's order): one-shot and
//! repeating timers on the caller's clock, run in time order, two due at once in the order they were set. A
//! repeating timer is steady: each run aims at the next beat (start + n * every), so a late wake-up shortens the next
//! wait instead of adding to it, and one past several beats runs the ones it missed, at most STEADY_CATCH_UP; past
//! them it counts again from now.

use crate::link::STEADY_CATCH_UP;

/// A timer's handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(u64);

#[derive(Clone, Debug)]
struct Entry<T> {
    id: TimerId,
    at: f64,
    // the order it was set in (a repeating timer takes a new one at each run)
    seq: u64,
    // 0: once
    every: f64,
    // the runs of this pump (the catch-up count)
    runs: u32,
    what: T,
}

/// Timers whose payload `T` says what each one does.
#[derive(Clone, Debug)]
pub struct Timers<T> {
    due: Vec<Entry<T>>,
    seq: u64,
    ids: u64,
}

impl<T: Clone> Default for Timers<T> {
    fn default() -> Self {
        Timers { due: Vec::new(), seq: 0, ids: 0 }
    }
}

impl<T: Clone> Timers<T> {
    /// A timer `ms` after `now`; `every` > 0: repeating, that far apart (at least 1 ms).
    pub fn add(&mut self, now: f64, ms: f64, every: f64, what: T) -> TimerId {
        self.ids += 1;
        let id = TimerId(self.ids);
        let seq = self.next_seq();
        let every = if every > 0.0 { every.max(1.0) } else { 0.0 };
        self.due.push(Entry { id, at: now + ms.max(0.0), seq, every, runs: 0, what });
        id
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// The timer stops (nothing when it ran or was cancelled).
    pub fn cancel(&mut self, id: TimerId) {
        self.due.retain(|e| e.id != id);
    }

    /// Every timer stops.
    pub fn clear(&mut self) {
        self.due.clear();
    }

    pub fn len(&self) -> usize {
        self.due.len()
    }

    pub fn is_empty(&self) -> bool {
        self.due.is_empty()
    }

    /// When the next one is due.
    pub fn next_due(&self) -> Option<f64> {
        self.due.iter().map(|e| e.at).reduce(f64::min)
    }

    /// A pump begins: the catch-up counts start again.
    pub fn begin(&mut self) {
        for e in &mut self.due {
            e.runs = 0;
        }
    }

    /// The first timer due by `now` (the earliest; of two at once the first set), taken out (a one-shot) or moved to
    /// its next beat (a repeating one).
    pub fn take_due(&mut self, now: f64) -> Option<(TimerId, T)> {
        loop {
            let at = (0..self.due.len()).filter(|&i| self.due[i].at <= now).min_by(|&a, &b| {
                let (x, y) = (&self.due[a], &self.due[b]);
                x.at.total_cmp(&y.at).then(x.seq.cmp(&y.seq))
            })?;
            if self.due[at].every == 0.0 {
                let e = self.due.remove(at);
                return Some((e.id, e.what));
            }
            let seq = self.next_seq();
            let e = &mut self.due[at];
            e.runs += 1;
            if e.runs > STEADY_CATCH_UP {
                // (past the catch-up: the beat counts again from now, this wake-up runs no more of it)
                e.at = now + e.every;
                e.seq = seq;
                continue;
            }
            e.at += e.every;
            e.seq = seq;
            return Some((e.id, e.what.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_order_then_set_order() {
        let mut t: Timers<u32> = Timers::default();
        t.add(0.0, 10.0, 0.0, 1);
        t.add(0.0, 5.0, 0.0, 2);
        t.add(0.0, 10.0, 0.0, 3);
        t.begin();
        assert_eq!(t.take_due(4.0), None);
        assert_eq!(t.take_due(10.0).map(|x| x.1), Some(2));
        assert_eq!(t.take_due(10.0).map(|x| x.1), Some(1));
        assert_eq!(t.take_due(10.0).map(|x| x.1), Some(3));
        assert!(t.is_empty());
    }

    // Node's timers on Windows fire on the system tick (about 15.6 ms): a plain 16 ms interval woken on those ticks
    // runs near 32 Hz; the steady beat keeps its rate.
    #[test]
    fn a_steady_beat_on_the_system_tick_keeps_its_rate() {
        let mut t: Timers<()> = Timers::default();
        t.add(0.0, 16.0, 16.0, ());
        let tick = 15.625;
        let mut runs = 0;
        let mut now = 0.0;
        while now < 10_000.0 {
            now += tick;
            // (the wake-up is the first system tick at or after the timer's due time)
            if t.next_due().is_some_and(|d| d <= now) {
                t.begin();
                while t.take_due(now).is_some() {
                    runs += 1;
                }
            }
        }
        assert!((620..=626).contains(&runs), "{runs} runs in 10 s of 16 ms beats");
    }

    #[test]
    fn a_long_stall_runs_at_most_the_catch_up() {
        let mut t: Timers<()> = Timers::default();
        t.add(0.0, 16.0, 16.0, ());
        t.begin();
        let mut runs = 0;
        while t.take_due(1000.0).is_some() {
            runs += 1;
        }
        assert_eq!(runs, STEADY_CATCH_UP);
        assert_eq!(t.next_due(), Some(1016.0));
    }
}
