//! The trace format and the compare: a written trace reads back the same, the Rust writer gives the oracle writer's
//! bytes, and one value changed is found at its tick and field. The oracle's traces are read from the tree's
//! `.scratch/traces` (written by the oracle's trace run); without them those tests say so and pass.

use std::path::PathBuf;

use mvs_trace::{
    CharRecord, EventRecord, Field, FighterRecord, Frame, Header, MontageRecord, Obj, Outcome, ShaggyRecord, Trace,
    TraceError, TraceWriter, Value, compare, frame_schema,
};

fn header(name: &str) -> Header {
    Header {
        format: 0,
        name: name.into(),
        spec: serde_json::json!({}),
        setup: serde_json::json!({}),
        world_seed: 3.0,
        store: serde_json::json!({}),
        source: serde_json::json!({}),
        ticks: 0,
        stop: "ticks".into(),
        hashes: vec![],
        final_hash: None,
        schema: vec![],
        stage: None,
        calls: vec![],
    }
}

fn frame(t: usize) -> Frame {
    let mut f = Frame::default();
    f.world.tick = (t + 1) as f64;
    for id in 0..2 {
        f.world.fighters.push(FighterRecord {
            id: id as f64,
            character: "shaggy".into(),
            x: t as f64 * 1.5 - id as f64 * 100.0,
            y: -0.0,
            vx: f64::NAN,
            montage: t.is_multiple_of(3).then(|| MontageRecord {
                name: "M".into(),
                position: t as f64 / 60.0,
                ..Default::default()
            }),
            state: CharRecord::Shaggy(ShaggyRecord { last_snack_spawn: Some(-1.0), ..Default::default() }),
            ..Default::default()
        });
    }
    f.world.events.push(EventRecord {
        kind: "hit".into(),
        fields: Obj::new(vec![("damage".into(), Value::Num(t as f64)), ("attacker".into(), Value::Num(0.0))]),
    });
    f
}

fn written(frames: &[Frame]) -> Trace {
    let mut w = TraceWriter::new();
    for f in frames {
        w.push(f);
    }
    Trace::from_bytes(w.finish(header("t"))).expect("reads back")
}

#[test]
fn reads_back_what_it_wrote() {
    let frames: Vec<Frame> = (0..50).map(frame).collect();
    let t = written(&frames);
    assert_eq!(t.len(), 50);
    assert_eq!(t.header.schema, frame_schema());
    for (i, f) in frames.iter().enumerate() {
        let back = t.frame(i).unwrap();
        // NaN != NaN for PartialEq: compare by the record's own rule
        assert!(mvs_trace::diff_frames(f, &back).is_empty(), "frame {i}");
        assert!(back.world.fighters[0].y.is_sign_negative());
    }
}

#[test]
fn finds_one_changed_value() {
    let frames: Vec<Frame> = (0..50).map(frame).collect();
    let mut other = frames.clone();
    // -0 against 0: the bits differ
    other[37].world.fighters[1].y = 0.0;
    match compare(&written(&frames), &written(&other)).unwrap() {
        Outcome::Differs { tick, fields } => {
            assert_eq!(tick, 37);
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].path, "world.fighters[1].y");
        }
        _ => panic!("not found"),
    }
    let mut other = frames.clone();
    other[12].world.events[0].fields.insert("damage".into(), Value::Num(12.000000000000002));
    match compare(&written(&frames), &written(&other)).unwrap() {
        Outcome::Differs { tick, fields } => {
            assert_eq!(tick, 12);
            assert_eq!(fields[0].path, "world.events[0]<hit>.damage");
        }
        _ => panic!("not found"),
    }
    assert!(matches!(compare(&written(&frames), &written(&frames)).unwrap(), Outcome::Same { ticks: 50 }));
    assert!(matches!(compare(&written(&frames), &written(&frames[..40])).unwrap(), Outcome::Length { .. }));
}

/// An object's keys are sorted however it is made, and a file whose keys are not is refused.
#[test]
fn object_keys_are_sorted() {
    let o = Obj::new(vec![("b".into(), Value::Null), ("a".into(), Value::Num(1.0)), ("b".into(), Value::Bool(true))]);
    let keys: Vec<&str> = o.entries().iter().map(|e| e.0.as_str()).collect();
    assert_eq!(keys, ["a", "b"]);
    assert_eq!(o.get("b"), Some(&Value::Bool(true)));
    // {b: null, a: null} as bytes: tag 6, two entries, "b", null, "a", null
    let bytes = [6u8, 2, 1, b'b', 0, 1, b'a', 0];
    let mut c = mvs_trace::field::Cursor::new(&bytes);
    assert!(matches!(Value::get(&mut c), Err(TraceError::Bad(..))));
}

/// A difference's path finds its value in a frame: a leaf, a list's `len`, an option with nothing in it, an item
/// past the end, an event's field, an event of another kind, the character state's variant.
#[test]
fn finds_a_value_by_its_path() {
    let f = frame(3);
    assert_eq!(f.find("world.fighters[0].x").as_deref(), Some(f.world.fighters[0].x.show().as_str()));
    assert_eq!(f.find("world.fighters.len").as_deref(), Some("2"));
    assert_eq!(f.find("world.fighters[0].montage.position").map(|s| s.starts_with("0.05")), Some(true));
    assert_eq!(frame(4).find("world.fighters[0].montage.position").as_deref(), Some("none"));
    assert_eq!(f.find("world.fighters[5].x").as_deref(), Some("absent"));
    assert_eq!(f.find("world.events[0]<hit>.damage").as_deref(), Some(Value::Num(3.0).show().as_str()));
    assert_eq!(f.find("world.events[0]<hit>.nothing").as_deref(), Some("absent"));
    assert!(f.find("world.events[0]<ko>.fighter").is_some_and(|s| s.starts_with("hit ")));
    assert!(f.find("world.fighters[0].state<shaggy>.last_snack_spawn").is_some_and(|s| s.starts_with("-1")));
    assert_eq!(f.find("world.no_such_field"), None);
}

fn oracle_traces() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/traces");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "trace")).collect())
        .unwrap_or_default();
    out.sort();
    if out.is_empty() {
        eprintln!("no oracle trace in {} (the oracle's trace run writes them): skipped", dir.display());
    }
    out
}

/// Every frame of the oracle's traces read and written again by the Rust side gives the oracle writer's bytes.
#[test]
fn rust_writer_gives_the_oracle_bytes() {
    for p in oracle_traces() {
        let t = Trace::read(&p).unwrap();
        let mut buf = Vec::new();
        for i in 0..t.len() {
            buf.clear();
            t.frame(i).unwrap().put(&mut buf);
            assert!(buf == t.frame_bytes(i), "{}: frame {i} written again differs", p.display());
        }
        eprintln!("{}: {} frames the same bytes", p.display(), t.len());
    }
}

/// One value of an oracle trace changed is found at its tick and field.
#[test]
fn finds_a_change_in_an_oracle_trace() {
    for p in oracle_traces() {
        let t = Trace::read(&p).unwrap();
        let at = t.len() * 2 / 3;
        // (the second fighter, or the first where the frame has one: a tutorial's player alone)
        let k = t.frame(at).unwrap().world.fighters.len().min(2) - 1;
        let mut w = TraceWriter::new();
        for i in 0..t.len() {
            let mut f = t.frame(i).unwrap();
            if i == at {
                let x = &mut f.world.fighters[k].x;
                *x = f64::from_bits(x.to_bits() ^ 1);
            }
            w.push(&f);
        }
        let changed = Trace::from_bytes(w.finish(t.header.clone())).unwrap();
        match compare(&t, &changed).unwrap() {
            Outcome::Differs { tick, fields } => {
                assert_eq!((tick, fields.len()), (at, 1), "{}", p.display());
                assert_eq!(fields[0].path, format!("world.fighters[{k}].x"));
            }
            _ => panic!("{}: the change at frame {at} was not found", p.display()),
        }
    }
}
