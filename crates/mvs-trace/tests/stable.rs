//! The stage-B class (`mvs_trace::stable`): a change to a field the float32 sim must not move is found at its tick
//! and field, a change to a float is not; and on the oracle's traces every field the class reads as an integer holds
//! one. The oracle's traces are read from `MVS_TRACES`, else the tree's `.scratch/traces`; without them those tests
//! say so and pass.

use std::path::PathBuf;

use mvs_trace::stable::{self, Rule};
use mvs_trace::{
    EventRecord, Field, FighterRecord, Frame, Header, MontageRecord, Obj, Outcome, Trace, TraceWriter, Value,
    compare_kept, diff_frames,
};

fn header() -> Header {
    Header {
        format: 0,
        name: "t".into(),
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
            damage: 10.25,
            ground_state: "Grounded".into(),
            hitstun: 4.0,
            montage: Some(MontageRecord {
                name: "M".into(),
                section: "Default".into(),
                position: t as f64 / 60.0,
                ..Default::default()
            }),
            rest: Value::obj(vec![("fadeAt".into(), Value::Num(30.0)), ("blend".into(), Value::Num(0.5))]),
            ..Default::default()
        });
    }
    f.world.events.push(EventRecord {
        kind: "hitVfx".into(),
        fields: Obj::new(vec![
            ("victim".into(), Value::Num(1.0)),
            ("x".into(), Value::Num(t as f64 * 0.1)),
            ("decayStacks".into(), Value::Num(0.0)),
        ]),
    });
    f
}

fn written(frames: &[Frame]) -> Trace {
    let mut w = TraceWriter::new();
    for f in frames {
        w.push(f);
    }
    Trace::from_bytes(w.finish(header())).expect("reads back")
}

/// The first tick and field the class counts after `change` at tick 20, or `None` when it counts none.
fn found(change: impl Fn(&mut Frame)) -> Option<(usize, String)> {
    let frames: Vec<Frame> = (0..40).map(frame).collect();
    let mut other = frames.clone();
    change(&mut other[20]);
    match compare_kept(&written(&frames), &written(&other), stable::keeps).unwrap() {
        Outcome::Differs { tick, fields } => Some((tick, fields[0].path.clone())),
        Outcome::Same { .. } => None,
        Outcome::Length { .. } => panic!("lengths differ"),
    }
}

fn at20(path: &str) -> Option<(usize, String)> {
    Some((20, path.to_owned()))
}

#[test]
fn a_must_not_move_field_changed_is_found() {
    assert_eq!(found(|f| f.world.fighters[1].ground_state = "Falling".into()), at20("world.fighters[1].ground_state"));
    assert_eq!(found(|f| f.world.fighters[0].hitstun = 5.0), at20("world.fighters[0].hitstun"));
    assert_eq!(
        found(|f| f.world.fighters[0].montage.as_mut().unwrap().section = "Loop".into()),
        at20("world.fighters[0].montage.section")
    );
    assert_eq!(found(|f| f.world.fighters[0].montage = None), at20("world.fighters[0].montage"));
    // the HUD's percentage: 10 to 11
    assert_eq!(found(|f| f.world.fighters[0].damage = 11.0), at20("world.fighters[0].damage"));
    // an event's kind, its fighter, one more event, an integer in it
    assert_eq!(found(|f| f.world.events[0].kind = "koVfx".into()), at20("world.events[0].type"));
    assert_eq!(
        found(|f| f.world.events[0].fields.insert("victim".into(), Value::Num(0.0))),
        at20("world.events[0]<hitVfx>.victim")
    );
    assert_eq!(found(|f| f.world.events.push(f.world.events[0].clone())), at20("world.events.len"));
    assert_eq!(
        found(|f| f.world.events[0].fields.insert("decayStacks".into(), Value::Num(1.0))),
        at20("world.events[0]<hitVfx>.decayStacks")
    );
    // a tick stamp of a bag, by its name
    assert_eq!(
        found(|f| f.world.fighters[0].rest = Value::obj(vec![("fadeAt".into(), Value::Num(31.0))])),
        at20("world.fighters[0].rest.blend")
    );
    let r = Value::obj(vec![("fadeAt".into(), Value::Num(31.0)), ("blend".into(), Value::Num(0.5))]);
    assert_eq!(found(|f| f.world.fighters[0].rest = r.clone()), at20("world.fighters[0].rest.fadeAt"));
    // the inputs, a stick's float too
    assert_eq!(found(|f| f.inputs.push(Default::default())), at20("inputs.len"));
    let frames: Vec<Frame> = (0..40)
        .map(|t| {
            let mut f = frame(t);
            f.inputs.push(Default::default());
            f
        })
        .collect();
    let mut other = frames.clone();
    other[20].inputs[0].x = 0.5;
    assert!(matches!(
        compare_kept(&written(&frames), &written(&other), stable::keeps).unwrap(),
        Outcome::Differs { tick: 20, .. }
    ));
}

#[test]
fn a_float_changed_is_not_counted() {
    assert_eq!(found(|f| f.world.fighters[1].x += 1e-3), None);
    assert_eq!(found(|f| f.world.fighters[1].x = f64::from_bits(f.world.fighters[1].x.to_bits() ^ 1)), None);
    assert_eq!(found(|f| f.world.fighters[0].montage.as_mut().unwrap().position += 1e-6), None);
    // the HUD's percentage stays 10
    assert_eq!(found(|f| f.world.fighters[0].damage = 10.999), None);
    assert_eq!(found(|f| f.world.events[0].fields.insert("x".into(), Value::Num(7.25))), None);
    assert_eq!(
        found(|f| f.world.fighters[0].rest =
            Value::obj(vec![("fadeAt".into(), Value::Num(30.0)), ("blend".into(), Value::Num(0.25))])),
        None
    );
}

#[test]
fn the_rules_of_some_paths() {
    let r = stable::rule_of;
    assert_eq!(r("world.fighters[3].damage"), Rule::Trunc);
    assert_eq!(r("world.fighters[0].x"), Rule::Floats);
    assert_eq!(r("world.fighters[0].dash_montage.age"), Rule::Int);
    assert_eq!(r("world.events[2]<soundStart>.fighter"), Rule::Int);
    assert_eq!(r("world.actors[1].rest.blastAt"), Rule::Floats);
    assert_eq!(r("world.actors[1].rest.fadeAt"), Rule::Int);
    assert_eq!(r("inputs[0].x"), Rule::Keep);
}

fn oracle_traces() -> Vec<PathBuf> {
    let dir = std::env::var_os("MVS_TRACES")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/traces"), PathBuf::from);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "trace")).collect())
        .unwrap_or_default();
    out.sort();
    if out.is_empty() {
        eprintln!("no oracle trace in {} (the oracle's trace run writes them): skipped", dir.display());
    }
    out
}

/// Each field the class reads as an integer holds one (or an infinity: a tick that never comes) in the oracle's
/// traces, every 97th frame of each: a name pattern that catches a float is found here.
#[test]
fn the_integer_fields_hold_integers_in_the_oracle_traces() {
    let mut bad: Vec<String> = Vec::new();
    for p in oracle_traces() {
        let t = Trace::read(&p).unwrap();
        let mut text = String::new();
        for i in (0..t.len()).step_by(97) {
            text.clear();
            t.frame(i).unwrap().show_tree(&mut String::new(), &mut text);
            for line in text.lines() {
                let Some((path, value)) = line.split_once(" = ") else { continue };
                let Some(v) = mvs_trace::field::parse_shown_f64(value) else { continue };
                if stable::rule_of(path) == Rule::Int && v.is_finite() && v.fract() != 0.0 {
                    let g = mvs_trace::field::general_path(path);
                    if !bad.iter().any(|b| b.starts_with(&g)) {
                        bad.push(format!("{g} = {v} ({} frame {i})", p.display()));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "integer fields holding fractions:\n{}", bad.join("\n"));
}

/// A frame of an oracle trace: a bit of a fighter's position flipped is not counted, its movement state changed is.
#[test]
fn an_oracle_frame_changed() {
    let Some(p) = oracle_traces().into_iter().next() else { return };
    let t = Trace::read(&p).unwrap();
    let f = t.frame(t.len() / 2).unwrap();
    let mut g = f.clone();
    g.world.fighters[0].x = f64::from_bits(g.world.fighters[0].x.to_bits() ^ 1);
    g.world.fighters[0].vy += 1.0;
    let d = diff_frames(&f, &g);
    assert_eq!(d.len(), 2);
    assert!(!d.iter().any(stable::keeps));
    g.world.fighters[0].ground_state += "x";
    let d: Vec<_> = diff_frames(&f, &g).into_iter().filter(stable::keeps).collect();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].path, "world.fighters[0].ground_state");
}
