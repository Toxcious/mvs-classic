//! Every call of the math tape (the oracle's math vectors) gives V8's bits. The tape is at
//! `.scratch/rust-trial/math.tape` in this tree, or at `MVS_MATH_TAPE`; with none the test says so and passes.

use mvs_math::{det, hypot, js, libm};
use trial_tape::{Cursor, Tape, same_bits};

#[test]
fn math_tape() {
    let path = std::env::var("MVS_MATH_TAPE")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../.scratch/rust-trial/math.tape").into());
    let Ok(tape) = Tape::open(&path) else {
        eprintln!("skip: no math tape at {path} (the oracle's math recorder writes it)");
        return;
    };
    type F = fn(&[f64]) -> f64;
    let fns: Vec<(&str, F)> = vec![
        ("libm.sin", |a| libm::sin(a[0])),
        ("libm.cos", |a| libm::cos(a[0])),
        ("libm.tan", |a| libm::tan(a[0])),
        ("libm.atan", |a| libm::atan(a[0])),
        ("libm.atan2", |a| libm::atan2(a[0], a[1])),
        ("libm.asin", |a| libm::asin(a[0])),
        ("libm.acos", |a| libm::acos(a[0])),
        ("libm.log", |a| libm::log(a[0])),
        ("libm.log2", |a| libm::log2(a[0])),
        ("libm.log10", |a| libm::log10(a[0])),
        ("libm.pow", |a| libm::pow(a[0], a[1])),
        ("hypot.hypot2", |a| hypot::hypot2(a[0], a[1])),
        ("hypot.hypot3", |a| hypot::hypot3(a[0], a[1], a[2])),
        ("hypot.hypot4", |a| hypot::hypot4(a[0], a[1], a[2], a[3])),
        ("det.acos", |a| det::acos(a[0])),
        ("det.hypot", |a| det::hypot(a[0], a[1])),
        ("det.finterp_constant_to", |a| det::finterp_constant_to(a[0], a[1], a[2], a[3])),
        ("js.min", |a| js::min(a[0], a[1])),
        ("js.max", |a| js::max(a[0], a[1])),
        ("js.round", |a| js::round(a[0])),
        ("js.fround", |a| js::fround(a[0])),
        ("js.sign", |a| js::sign(a[0])),
        ("js.trunc", |a| a[0].trunc()),
        ("js.floor", |a| a[0].floor()),
        ("js.ceil", |a| a[0].ceil()),
        ("js.rem", |a| a[0] % a[1]),
        ("js.to_int32", |a| js::to_int32(a[0]) as f64),
        ("js.to_uint32", |a| js::to_uint32(a[0]) as f64),
    ];
    let mut by_id: Vec<Option<(&str, F)>> = vec![None; tape.header.calls.len()];
    for (name, f) in &fns {
        by_id[tape.header.id(name) as usize] = Some((name, *f));
    }
    let mut checked = vec![0usize; by_id.len()];
    let mut wrong: Vec<String> = Vec::new();
    let mut args = [0.0f64; 4];
    for rec in tape.records() {
        let (name, f) = by_id[rec.id as usize].unwrap_or_else(|| panic!("no port of tape call {}", rec.id));
        let mut c = Cursor::new(rec);
        let n = rec.payload.len() / 8 - 1;
        for a in args.iter_mut().take(n) {
            *a = c.f64();
        }
        let want = c.f64();
        let got = f(&args[..n]);
        checked[rec.id as usize] += 1;
        if !same_bits(got, want) && wrong.len() < 40 {
            wrong.push(format!(
                "{name}{:?}: rust {got:e} ({:#018x}) v8 {want:e} ({:#018x})",
                &args[..n],
                got.to_bits(),
                want.to_bits()
            ));
        }
    }
    for (id, n) in checked.iter().enumerate() {
        assert!(*n > 0, "tape call {} not checked", tape.header.calls[id].name);
    }
    assert!(wrong.is_empty(), "{} differing (first {}):\n{}", wrong.len(), wrong.len(), wrong.join("\n"));
    eprintln!("math tape: {} calls of {} functions, all the same bits", checked.iter().sum::<usize>(), checked.len());
}
