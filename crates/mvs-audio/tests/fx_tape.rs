//! The effects tape: each case of the oracle's worklet processors replayed through
//! `Fx`, sample for sample. The tape is not in the repo: `MVS_AUDIO_TAPES` names the folder holding `fx.jsonl`
//! (default `.scratch/audio` at the workspace root); a test that finds none says so and passes.

use std::path::PathBuf;

use mvs_audio::QUANTUM;
use mvs_audio::fx::Fx;
use mvs_audio::mix::Block;
use mvs_audio::tape::{self, fx::Case};

fn tape_path() -> PathBuf {
    match std::env::var_os("MVS_AUDIO_TAPES") {
        Some(d) => PathBuf::from(d).join("fx.jsonl"),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/audio/fx.jsonl"),
    }
}

struct Outcome {
    samples: usize,
    bad: usize,
    max: f64,
    meter_ok: bool,
}

fn replay(c: &Case) -> Outcome {
    let ch = usize::from(c.channels);
    let frames = c.frames as usize;
    let (input, expect) = (c.input().unwrap(), c.output().unwrap());
    let mut fx = Fx::new(&c.fx, c.er.as_ref(), c.rate, ch).unwrap_or_else(|| panic!("{}: no effect", c.case));
    let mut got = vec![0.0f32; ch * frames];
    let mut block = Block::new(ch);
    let mut meter: Vec<[f64; 2]> = Vec::new();
    let mut last = fx.meter().map(|m| m.1);
    for at in (0..frames).step_by(QUANTUM) {
        for a in c.auto.iter().filter(|a| a.at as usize == at) {
            fx.set_param(a.id, a.value);
        }
        for k in 0..ch {
            block.ch[k].copy_from_slice(&input[k * frames + at..k * frames + at + QUANTUM]);
        }
        fx.process(&mut block);
        for k in 0..ch {
            got[k * frames + at..k * frames + at + QUANTUM].copy_from_slice(&block.ch[k]);
        }
        if let Some((_, v)) = fx.meter()
            && Some(v) != last
        {
            meter.push([(at + QUANTUM) as f64, v]);
            last = Some(v);
        }
    }
    let (mut bad, mut max) = (0, 0.0f64);
    for (g, e) in got.iter().zip(&expect) {
        if g.to_bits() != e.to_bits() {
            bad += 1;
            max = max.max((f64::from(*g) - f64::from(*e)).abs());
        }
    }
    Outcome { samples: got.len(), bad, max, meter_ok: meter == c.meter }
}

#[test]
fn effects_match_the_oracle() {
    let path = tape_path();
    if !path.exists() {
        eprintln!("no effects tape at {}: nothing compared", path.display());
        return;
    }
    let cases: Vec<Case> = tape::read(&path).unwrap();
    let mut failed = Vec::new();
    for c in &cases {
        let o = replay(c);
        eprintln!(
            "{:28} samples {:7} differing {:6} max diff {:e} meter {}",
            c.case, o.samples, o.bad, o.max, o.meter_ok
        );
        if o.bad > 0 || !o.meter_ok {
            failed.push(c.case.clone());
        }
    }
    eprintln!("{} cases, {} failed", cases.len(), failed.len());
    assert!(failed.is_empty(), "{failed:?}");
}
