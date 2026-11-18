//! The cost of a rollback save: the size of the World, the heap a copy of it holds and the time of a save and a load
//! on a heavy frame, to re-measure what the float32 sim changed. Opt in, on the trace profile:
//! `cargo-slot.ps1 test --profile trace -p mvs-sim-trace --test rollback_cost '--' --ignored --nocapture`.
//! `MVS_COST_MATCH` / `MVS_COST_FRAME` choose the frame (default ffa-items-a at 6000, a four-fighter match with items).
//!
//! This file is the one test of its binary, so no other test's allocations are counted.

use mvs_sim::World;
use mvs_sim::snapshot::SnapshotRing;
use mvs_sim_trace::harness;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::Instant;

struct Counting;

static COUNTING: AtomicBool = AtomicBool::new(false);
/// Bytes live since counting started (allocations less frees).
static LIVE: AtomicIsize = AtomicIsize::new(0);

fn add(n: isize) {
    if COUNTING.load(Ordering::Relaxed) {
        LIVE.fetch_add(n, Ordering::Relaxed);
    }
}

// SAFETY: forwards every call to the system allocator unchanged; the counter is an atomic.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        add(l.size() as isize);
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        add(-(l.size() as isize));
        // SAFETY: `p` came from `alloc` above, which is the system allocator's.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        add(n as isize - l.size() as isize);
        // SAFETY: as `alloc`.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn median(v: &mut [u128]) -> u128 {
    v.sort_unstable();
    v[v.len() / 2]
}

#[test]
#[ignore = "a measurement, not a check"]
fn rollback_save_cost() {
    let name = std::env::var("MVS_COST_MATCH").unwrap_or_else(|_| "ffa-items-a".to_owned());
    let frame = std::env::var("MVS_COST_FRAME").ok().and_then(|s| s.parse().ok()).unwrap_or(6000);
    let Some(h) = harness::at(&name, frame) else { return };
    let w = h.w;
    println!("size_of::<World>() = {} bytes", std::mem::size_of::<World>());

    // The heap a copy holds: what cloning the World allocates and keeps (shared data behind an Arc is not copied).
    LIVE.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let copy = w.clone();
    COUNTING.store(false, Ordering::Relaxed);
    println!("heap bytes of a copy = {}", LIVE.load(Ordering::Relaxed));

    let mut ring = SnapshotRing::new(2, &w);
    let mut back = copy;
    let (mut save, mut load) = (Vec::new(), Vec::new());
    for t in 0..1000u32 {
        let s = Instant::now();
        ring.save(t, &w);
        save.push(s.elapsed().as_nanos());
        let s = Instant::now();
        assert!(ring.load(t, &mut back));
        load.push(s.elapsed().as_nanos());
    }
    println!("{name} frame {frame}: save (clone_from into a slot) median of 1000 = {} ns", median(&mut save));
    println!("{name} frame {frame}: load (clone_from back) median of 1000 = {} ns", median(&mut load));
}
