//! The derived state walks (`#[derive(State)]`): `clone_from` copies field by field and leaves a skipped field, the
//! hash walks the same fields; the arena keeps its list order; the sim crates share one clippy config.

use indexmap::IndexSet;
use mvs_sim::State;
use mvs_sim::ids::Arena;
use mvs_sim::state::hash_of;

#[derive(State, Debug, PartialEq)]
enum Shape {
    Dot,
    Line { len: f64, tags: Vec<u32> },
    Pair(i32, i32),
}

#[derive(State, Debug, PartialEq)]
struct Thing {
    x: f64,
    shape: Shape,
    seen: IndexSet<i32>,
    #[state(skip)]
    scratch: Vec<u8>,
}

fn thing() -> Thing {
    Thing {
        x: 1.5,
        shape: Shape::Line { len: 2.0, tags: vec![1, 2, 3] },
        seen: [4, 5].into_iter().collect(),
        scratch: vec![9],
    }
}

#[test]
fn clone_from_copies_all_but_the_skipped() {
    let a = thing();
    let mut b = Thing { x: 0.0, shape: Shape::Dot, seen: IndexSet::new(), scratch: vec![7, 7] };
    b.clone_from(&a);
    assert_eq!(b.x, 1.5);
    assert_eq!(b.shape, a.shape);
    assert_eq!(b.seen, a.seen);
    assert_eq!(b.scratch, vec![7, 7]);
    // `clone` clones the skipped field too.
    assert_eq!(a.clone().scratch, vec![9]);

    // The same variant copies into its own buffers.
    let mut c = thing();
    let Shape::Line { tags, .. } = &mut c.shape else { unreachable!() };
    tags.reserve(64);
    let cap = tags.capacity();
    c.clone_from(&a);
    let Shape::Line { tags, .. } = &c.shape else { unreachable!() };
    assert_eq!(tags.capacity(), cap);

    let mut d = Shape::Pair(1, 2);
    d.clone_from(&Shape::Pair(3, 4));
    assert_eq!(d, Shape::Pair(3, 4));
}

#[test]
fn the_hash_walks_the_state() {
    let a = thing();
    let mut b = a.clone();
    assert_eq!(hash_of(&a), hash_of(&b));
    b.scratch.push(1);
    assert_eq!(hash_of(&a), hash_of(&b), "a skipped field is not hashed");
    b.x = -1.5;
    assert_ne!(hash_of(&a), hash_of(&b));
    // -0 and 0 differ (the bits are hashed); the variant index is hashed.
    assert_ne!(hash_of(&0.0f64), hash_of(&-0.0f64));
    assert_ne!(hash_of(&Shape::Dot), hash_of(&Shape::Pair(0, 0)));
    // An IndexSet hashes in insertion order.
    let s1: IndexSet<i32> = [1, 2].into_iter().collect();
    let s2: IndexSet<i32> = [2, 1].into_iter().collect();
    assert_ne!(hash_of(&s1), hash_of(&s2));
}

#[test]
fn the_arena_keeps_the_list_order() {
    let mut a: Arena<u32> = Arena::default();
    let ids: Vec<_> = (0..5).map(|v| a.insert(v)).collect();
    a.retain(|_, &v| v % 2 == 0);
    assert_eq!(a.iter().map(|(_, &v)| v).collect::<Vec<_>>(), [0, 2, 4]);
    assert!(a.get(ids[1]).is_none());
    // A freed slot is reused with a new generation: the old id stays dead.
    let n = a.insert(10);
    assert_eq!(n.index, ids[3].index);
    assert!(a.get(ids[3]).is_none());
    assert_eq!(a.get(n), Some(&10));
    assert_eq!(a.iter().map(|(_, &v)| v).collect::<Vec<_>>(), [0, 2, 4, 10]);
}

#[test]
fn the_deterministic_crates_share_one_clippy_config() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let own = std::fs::read_to_string(dir.join("clippy.toml")).unwrap();
    for c in ["mvs-anim", "mvs-collision", "mvs-vfx"] {
        let other = std::fs::read_to_string(dir.join("..").join(c).join("clippy.toml")).unwrap();
        assert_eq!(own, other, "{c}/clippy.toml differs from mvs-sim's");
    }
}
