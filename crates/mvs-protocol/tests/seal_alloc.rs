//! Sealing and opening allocate nothing on buffers that have room (relay_seal: the relay seals and opens every
//! packet of every match on one thread): a counting global allocator around frame_into, datagram_into and a
//! datagram's open, after a first round that sizes the buffers.
//!
//! Only the test's own thread is counted: the test harness's threads allocate meanwhile (its output, its timer).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

use mvs_protocol::match_wire::MAX_PACKET;
use mvs_protocol::relay_seal::{MAX_SEALED_DATAGRAM, derive, split};
use mvs_protocol::relay_wire::{FT_MATCH, Side, Transcript, UDP_MATCH};

struct Counting;

thread_local! {
    /// Whether this thread's allocations count (a const one: reading it allocates nothing).
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

fn count() {
    if COUNTING.with(Cell::get) {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: forwards every call to the system allocator unchanged; the counter is an atomic.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: `p` came from `alloc` above, which is the system allocator's.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count();
        // SAFETY: as `alloc`.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn sealing_allocates_nothing() {
    let t = Transcript {
        client_key: [1; 32],
        relay_key: [2; 32],
        client_nonce: [3; 32],
        relay_nonce: [4; 32],
        client_kx: [5; 32],
        relay_kx: [6; 32],
    };
    let k = derive(&[7; 32], &t);
    let (mut tx, _, _) = split(&k, Side::Relay);
    let (_, _, mut rx) = split(&k, Side::Client);
    let packet = vec![9u8; MAX_PACKET];
    let mut d = Vec::with_capacity(MAX_SEALED_DATAGRAM);
    let mut stream = Vec::with_capacity(64 * 1024);
    COUNTING.with(|c| c.set(true));
    let mut opened = 0;
    for i in 0..1000u32 {
        tx.datagram_into(&mut d, &[8; 8], UDP_MATCH, &[&i.to_be_bytes(), &packet[..(i as usize) % MAX_PACKET]]);
        if rx.open(&mut d).is_some() {
            opened += 1;
        }
        stream.clear();
        tx.frame_into(&mut stream, FT_MATCH, &[&i.to_be_bytes(), &packet]).expect("a frame");
    }
    COUNTING.with(|c| c.set(false));
    assert_eq!(opened, 1000);
    assert_eq!(ALLOCS.load(Ordering::Relaxed), 0, "allocations while sealing and opening");
}
