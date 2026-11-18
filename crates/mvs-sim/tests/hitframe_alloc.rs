//! A fighter in an active Hitframe allocates nothing per tick once the pools are warm: Shaggy attacks Shaggy (a
//! neutral attack pressed again and again, the victim kept within reach), the first rounds warm the World's scratch
//! and the rounds after are counted under a global allocator that counts, the ticks with a Hitframe active apart.
//!
//! This file is the one test of its binary, so no other test's allocations are counted.

use mvs_data::GameData;
use mvs_sim::combat::phases;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::{InputState, World};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counting;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator unchanged; the counter is an atomic.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: `p` came from `alloc` above, which is the system allocator's.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as `alloc`.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Shaggy against Shaggy on the Lab, the data from the dev store; none (the test says so and passes) without one.
fn world() -> Option<World> {
    let dir = mvs_store::dev_store_dir();
    let Ok(store) = mvs_store::Store::open(&dir) else {
        eprintln!("no store at {}: skipped", dir.display());
        return None;
    };
    let data = Arc::new(GameData::load(&store, &["shaggy"], "trainingroomsmall").expect("the gamedata kind loads"));
    let player = MatchPlayer { character: Some("shaggy".into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player.clone(), player], ..MatchSetup::default() };
    Some(World::new(data, 7, &setup).expect("binds"))
}

/// One step with the attacker pressing attack on the first ticks of every 60, the victim `gap` in front of the attacker
/// first. Then, in a tick with a Hitframe active, both fighters' overlap updates run again as the step's own do: the
/// allocations of those calls, whether a Hitframe was active and whether they dealt a hit.
fn tick(w: &mut World, t: i32, gap: f32) -> (usize, bool, bool) {
    let (a, b) = (w.players[0], w.players[1]);
    let x = w.fighter(a).body.x;
    w.fighter_mut(b).body.x = x + gap * w.fighter(a).body.facing_dir;
    let press = InputState { attack: true, ..InputState::default() };
    let inputs = [if t % 60 < 2 { press } else { InputState::default() }, InputState::default()];
    w.step(&inputs);
    if w.fighter(a).montages.active_hits.is_empty() {
        return (0, false, false);
    }
    if gap < 100.0 {
        // The step's own hit is recorded: forget it, so the update's test deals it again.
        w.frame_hits.clear();
        for h in w.fighter_mut(a).montages.active_hits.values_mut() {
            h.hit_set.clear();
        }
    }
    ALLOCS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    phases::overlaps(w, a, true);
    phases::overlaps(w, b, false);
    COUNTING.store(false, Ordering::Relaxed);
    (ALLOCS.load(Ordering::Relaxed), true, !w.frame_hits.is_empty())
}

/// The overlap updates of a fighter in an active Hitframe over a run of attacks, after a run that grew the pools.
fn measure(gap: f32) -> Option<(usize, usize, usize)> {
    let mut w = world()?;
    for t in 0..360 {
        tick(&mut w, t, gap);
    }
    let mut active_ticks = 0;
    let mut dealt = 0;
    let mut total = 0;
    for t in 360..720 {
        let (n, active, hit) = tick(&mut w, t, gap);
        if active {
            active_ticks += 1;
            dealt += usize::from(hit);
            total += n;
        }
    }
    Some((total, active_ticks, dealt))
}

#[test]
fn an_active_hitframe_allocates_nothing_after_warm_up() {
    if out_of_reach().is_some() {
        in_reach();
    }
}

/// None when there is no store (the test is skipped).
fn out_of_reach() -> Option<()> {
    // Out of reach: the Hitframe's shapes are placed and tested every tick, no hit.
    let (total, ticks, dealt) = measure(600.0)?;
    assert!(ticks > 0, "no Hitframe was active: the test measures nothing");
    assert_eq!(dealt, 0, "a hit was dealt out of reach");
    assert_eq!(total, 0, "allocations in {ticks} ticks with a Hitframe active, out of reach");
    Some(())
}

fn in_reach() {
    // In reach: the step's own hit is forgotten before each counted update, which deals it again (the Hitframe's
    // test, the deferred call and the packet it carries). Left to allocate are the three of the hit's own
    // application: its hit-stun decay list and the two lists of the attack's voice tags.
    let Some((total, ticks, dealt)) = measure(70.0) else { return };
    assert!(ticks > 0, "no Hitframe was active: the test measures nothing");
    assert!(dealt > 0, "no hit was dealt: the test measures nothing");
    assert!(total <= 3 * ticks, "{total} allocations in {ticks} ticks with a Hitframe dealing");
}
