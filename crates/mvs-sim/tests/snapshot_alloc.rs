//! A rollback save allocates nothing once the ring's slots hold a world of the same shape: a World of two fighters
//! with state in its lists and maps (a projectile, an item, a buff with its own values, Shaggy's state) is saved over
//! a ring again and again, loaded back and stepped, under a global allocator that counts. A field whose type cloned
//! by a std `Clone` (a fresh copy) instead of `clone_from` would allocate here.
//!
//! This file is the one test of its binary, so no other test's allocations are counted.

use mvs_data::GameData;
use mvs_data::value::Value;
use mvs_sim::actors::{ActorRef, Item, ItemClass, Projectile, ProjectileClass};
use mvs_sim::chars::CharState;
use mvs_sim::chars::shaggy::ShaggyState;
use mvs_sim::chars::shaggy::state::KickMix;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::combat::buffs::{BuffExt, BuffOwn, FighterBuff};
use mvs_sim::fighter::match_side::VoLine;
use mvs_sim::names::Name;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::snapshot::SnapshotRing;
use mvs_sim::state::hash_of;
use mvs_sim::world::{Timer, TimerCmd};
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

fn counted(f: impl FnOnce()) -> usize {
    ALLOCS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    f();
    COUNTING.store(false, Ordering::Relaxed);
    ALLOCS.load(Ordering::Relaxed)
}

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
    let mut w = World::new(data, 7, &setup).expect("binds");
    // State in the lists and maps a save copies.
    for (n, &id) in w.players.clone().iter().enumerate() {
        let f = w.fighter_mut(id);
        f.decay.attack_decay.extend([Name(1), Name(2), Name(3)]);
        f.decay.landed_attack_ids.extend(0..8);
        f.react.hr_times.insert(Name(4), 0.25 * n as f32);
        f.vo.vo_lines.push(VoLine { key: 5, until: 30.0 });
        f.cs = CharState::Shaggy(ShaggyState {
            kick_mix: Some(KickMix { anim: Name(6), time: 0.5, w: 1.0 }),
            ..ShaggyState::default()
        });
        let values = [("SpeedMod", 1.5), ("JumpMod", 2.0)].into_iter().map(|(k, v)| (k.to_owned(), Value::Num(v)));
        let own = BuffOwn { values: values.collect() };
        f.buffs.buffs.push(buff(n as u32, own));
    }
    let (a, b) = (w.players[0], w.players[1]);
    let mut p = Projectile::new(Name(9), ProjectileClass::FireWallHazard, Some(a), 0, 1);
    p.hit_set.extend([0, 1]);
    p.hit_actors.extend([ActorRef::Fighter(b), ActorRef::Stage(2)]);
    p.healed_allies.insert(0);
    let pid = w.actors.projectiles.insert(p);
    let mut it = Item::new(Name(10), ItemClass::ItemSandwich, None, 0.0, 1.0, 0.0, 0.0, false);
    it.reach.extend([a, b]);
    it.overlapping.insert(ActorRef::Projectile(pid));
    w.actors.items.insert(it);
    w.flow.timers.push(Timer { at: 90, cmd: TimerCmd::StartBlastShrink, key: None });
    w.frame_hits.extend([1, 2, 3]);
    Some(w)
}

fn buff(serial: u32, own: BuffOwn) -> FighterBuff {
    FighterBuff {
        serial,
        cls: Name(11),
        class: BuffClass::BuffInvulnerable,
        frames: 60.0,
        stacks: 1.0,
        stack_frames: 0.0,
        heal: None,
        heal_amt: None,
        born: Some(0.0),
        rescue: None,
        by: None,
        pulled: None,
        notify: None,
        fire: None,
        fire_dmg: None,
        src: None,
        ext: BuffExt::None,
        stun: false,
        stun_by: None,
        mods_off: None,
        own: Some(own),
        gate: None,
        add_age: None,
        ally_hit: false,
        swept: false,
    }
}

#[test]
fn a_save_allocates_nothing_after_warm_up() {
    let Some(mut w) = world() else { return };
    let mut ring = SnapshotRing::new(8, &w);
    let inputs = [InputState::default(); 2];
    // Warm-up: the step's scratch grown and the world's lists and maps at the shape the saves below copy (the first
    // steps add to them), then every slot saved once.
    for _ in 0..8 {
        w.step(&inputs);
    }
    for t in 0..8 {
        ring.save(t, &w);
    }
    let h = hash_of(&w);
    // The counter sees a fresh clone's allocations.
    assert!(counted(|| drop(w.clone())) > 0);
    let n = counted(|| {
        for t in 8..200 {
            w.flow.tick = t as i32;
            ring.save(t, &w);
        }
        assert!(ring.load(199, &mut w));
        for _ in 0..16 {
            w.step(&inputs);
        }
    });
    assert_eq!(n, 0, "allocations after warm-up");
    assert!(ring.load(199, &mut w));
    assert_eq!(w.flow.tick, 199);
    assert_ne!(h, hash_of(&w));
    // A loaded state hashes as the saved one.
    assert_eq!(hash_of(ring.get(199).unwrap()), hash_of(&w));
}
