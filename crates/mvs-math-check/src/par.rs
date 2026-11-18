//! A range of inputs spread over every hardware thread in chunks, each chunk's result kept in input order.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(8, |n| n.get())
}

/// `f(lo, hi)` over [start, end) in chunks of `chunk`; the results in chunk order.
pub fn run<T: Send>(start: u64, end: u64, chunk: u64, f: impl Fn(u64, u64) -> T + Sync) -> Vec<T> {
    let next = AtomicU64::new(start);
    let out = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..threads() {
            s.spawn(|| {
                loop {
                    let lo = next.fetch_add(chunk, Ordering::Relaxed);
                    if lo >= end {
                        break;
                    }
                    let r = f(lo, (lo + chunk).min(end));
                    out.lock().unwrap().push((lo, r));
                }
            });
        }
    });
    let mut v = out.into_inner().unwrap();
    v.sort_by_key(|(lo, _)| *lo);
    v.into_iter().map(|(_, r)| r).collect()
}

/// Mismatches of a check: how many, and the first few lines describing them.
#[derive(Default)]
pub struct Tally {
    pub count: u64,
    pub examples: Vec<String>,
}

impl Tally {
    pub fn add(&mut self, line: impl FnOnce() -> String) {
        self.count += 1;
        if self.examples.len() < 8 {
            self.examples.push(line());
        }
    }

    pub fn merge(parts: Vec<Tally>) -> Tally {
        let mut t = Tally::default();
        for p in parts {
            t.count += p.count;
            for e in p.examples {
                if t.examples.len() < 8 {
                    t.examples.push(e);
                }
            }
        }
        t
    }
}
