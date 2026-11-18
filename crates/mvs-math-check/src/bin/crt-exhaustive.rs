//! `mvs_math::f32::crt` against the correctly rounded reference: the one-argument functions on every float input,
//! atan2f / powf / fmodf on edge-case grids and random pairs of several shapes.
//!
//! `crt-exhaustive [name...] [--slow] [--pairs <n>]`: the named functions (all when none is named); `--slow` also runs
//! each function's double-double path on a sample of 2^24 inputs (the fast path hands it a few in a million);
//! `--pairs` sets the random pairs per shape of the two-argument functions (default 2^26). Exits 1 on any mismatch.

use mvs_math::f32::crt::{self, slow};
use mvs_math_check::par::{Tally, run};
use mvs_math_check::reference::{self, F1};
use mvs_math_check::{any_float, float_in, next, same};
use std::time::Instant;

fn port(f: F1) -> fn(f32) -> f32 {
    match f {
        F1::Sin => crt::sinf,
        F1::Cos => crt::cosf,
        F1::Tan => crt::tanf,
        F1::Asin => crt::asinf,
        F1::Acos => crt::acosf,
        F1::Atan => crt::atanf,
        F1::Exp => crt::expf,
        F1::Log => crt::logf,
    }
}

type SlowPath = (fn(f32) -> f32, fn(f32) -> bool);

/// The double-double path and the inputs it accepts.
fn slow_path(f: F1) -> SlowPath {
    match f {
        F1::Sin => (slow::sinf, |x| x.is_finite()),
        F1::Cos => (slow::cosf, |x| x.is_finite()),
        F1::Tan => (slow::tanf, |x| x.is_finite()),
        F1::Asin => (slow::asinf, |x| x.abs() < 1.0),
        F1::Acos => (slow::acosf, |x| x > -1.0 && x <= 1.0),
        F1::Atan => (slow::atanf, |x| x.is_finite()),
        F1::Exp => (slow::expf, |x| (-110.0..=89.0).contains(&x)),
        F1::Log => (slow::logf, |x| x.is_finite() && x > 0.0),
    }
}

fn report(what: &str, inputs: u64, t: &Tally, started: Instant) -> bool {
    println!("{what}: {inputs} inputs, {} mismatches ({:.1} s)", t.count, started.elapsed().as_secs_f64());
    for e in &t.examples {
        println!("  {e}");
    }
    t.count == 0
}

fn every_input(f: F1) -> bool {
    let started = Instant::now();
    let p = port(f);
    let parts = run(0, 1 << 32, 1 << 20, |lo, hi| {
        let mut t = Tally::default();
        for b in lo..hi {
            let x = f32::from_bits(b as u32);
            let (got, want) = (p(x), reference::f1(f, x));
            if !same(got, want) {
                t.add(|| format!("{}({:#010x} = {x:e}) = {got:e}, reference {want:e}", f.name(), b));
            }
        }
        t
    });
    report(f.name(), 1 << 32, &Tally::merge(parts), started)
}

fn slow_sample(f: F1) -> bool {
    let started = Instant::now();
    let (s, accepts) = slow_path(f);
    let parts = run(0, 1 << 24, 1 << 16, |lo, hi| {
        let mut t = Tally::default();
        let mut state = 0x9e37_79b9_7f4a_7c15 ^ lo.wrapping_mul(0x2545_f491_4f6c_dd1d);
        let mut n = lo;
        while n < hi {
            // half of all bit patterns, half the ordinary range of the function
            let x = if n & 1 == 0 { any_float(&mut state) } else { float_in(&mut state, -100.0, 100.0) };
            if !accepts(x) {
                continue;
            }
            n += 1;
            let (got, want) = (s(x), reference::f1(f, x));
            if !same(got, want) {
                t.add(|| format!("slow {}({:#010x} = {x:e}) = {got:e}, reference {want:e}", f.name(), x.to_bits()));
            }
        }
        t
    });
    report(&format!("{} double-double path", f.name()), 1 << 24, &Tally::merge(parts), started)
}

/// Floats a two-argument function's special cases turn on.
const EDGES: [f32; 30] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    -0.5,
    2.0,
    -2.0,
    3.0,
    -3.0,
    0.25,
    1.5,
    -1.5,
    360.0,
    -360.0,
    16_777_216.0,
    -16_777_216.0,
    16_777_218.0,
    16_777_215.0,
    -16_777_215.0,
    f32::MAX,
    -f32::MAX,
    f32::MIN_POSITIVE,
    -f32::MIN_POSITIVE,
    1e-45,
    -1e-45,
    f32::INFINITY,
    f32::NEG_INFINITY,
    f32::NAN,
    core::f32::consts::PI,
];

type Shape = fn(&mut u64) -> (f32, f32);

fn pairs(
    name: &str,
    f: fn(f32, f32) -> f32,
    r: fn(f32, f32) -> f32,
    shapes: &[(&str, Shape)],
    n: u64,
    edges: bool,
) -> bool {
    let mut ok = true;
    let started = Instant::now();
    let mut t = Tally::default();
    for &a in if edges { &EDGES[..] } else { &[] } {
        for &b in &EDGES {
            let (got, want) = (f(a, b), r(a, b));
            if !same(got, want) {
                t.add(|| format!("{name}({a:e}, {b:e}) = {got:e}, reference {want:e}"));
            }
        }
    }
    if edges {
        ok &= report(&format!("{name} edge grid"), (EDGES.len() * EDGES.len()) as u64, &t, started);
    }
    for (shape, make) in shapes {
        let started = Instant::now();
        let parts = run(0, n, 1 << 18, |lo, hi| {
            let mut t = Tally::default();
            let mut state = 0x9e37_79b9_7f4a_7c15 ^ (lo + 1).wrapping_mul(0x2545_f491_4f6c_dd1d);
            for _ in lo..hi {
                let (a, b) = make(&mut state);
                let (got, want) = (f(a, b), r(a, b));
                if !same(got, want) {
                    t.add(|| {
                        format!(
                            "{name}({a:e} = {:#010x}, {b:e} = {:#010x}) = {got:e}, reference {want:e}",
                            a.to_bits(),
                            b.to_bits()
                        )
                    });
                }
            }
            t
        });
        ok &= report(&format!("{name} {shape}"), n, &Tally::merge(parts), started);
    }
    ok
}

fn bits(s: &mut u64) -> (f32, f32) {
    (any_float(s), any_float(s))
}

fn atan2_shapes() -> Vec<(&'static str, Shape)> {
    vec![
        ("random bits", bits),
        ("in [-1000, 1000]", |s| (float_in(s, -1000.0, 1000.0), float_in(s, -1000.0, 1000.0))),
        ("near the diagonals", |s| {
            let a = float_in(s, -10.0, 10.0);
            let d = f32::from_bits(a.to_bits().wrapping_add((next(s) % 64) as u32).wrapping_sub(32));
            if next(s) & 1 == 0 { (a, d) } else { (a, -d) }
        }),
        ("ratios far from 1", |s| {
            let a = float_in(s, 0.5, 1.0) * f32::from_bits(((next(s) % 200 + 27) as u32) << 23);
            let b = float_in(s, -1.0, 1.0);
            if next(s) & 1 == 0 { (a, b) } else { (b, a) }
        }),
    ]
}

fn pow_shapes() -> Vec<(&'static str, Shape)> {
    vec![
        ("random bits", bits),
        ("x in (0, 4), y in [-4, 4]", |s| (float_in(s, 0.0, 4.0), float_in(s, -4.0, 4.0))),
        ("x in [-1000, 1000], integer y", |s| (float_in(s, -1000.0, 1000.0), ((next(s) % 41) as f32) - 20.0)),
        ("x near 1, large y", |s| {
            let x = f32::from_bits(1.0f32.to_bits().wrapping_add((next(s) % 4096) as u32).wrapping_sub(2048));
            (x, float_in(s, -2e6, 2e6))
        }),
        ("exact powers", |s| {
            // small odd mantissas to small powers, and squares under half powers: the cases at or near midpoints
            let m = (next(s) % 8192) as f32 * f32::from_bits(((next(s) % 40 + 107) as u32) << 23);
            match next(s) % 3 {
                0 => (m, (next(s) % 9 + 2) as f32),
                1 => (m * m, 0.5),
                _ => (m, -((next(s) % 4 + 1) as f32)),
            }
        }),
        ("results near the float range's ends", |s| {
            let x = float_in(s, 1.5, 100.0);
            let edge = if next(s) & 1 == 0 { 88.72 } else { -103.97 };
            (x, edge / x.ln() + float_in(s, -0.01, 0.01))
        }),
    ]
}

fn fmod_shapes() -> Vec<(&'static str, Shape)> {
    vec![
        ("random bits", bits),
        ("angles by 360", |s| (float_in(s, -1e5, 1e5), if next(s) & 1 == 0 { 360.0 } else { -360.0 })),
        ("in [-1000, 1000]", |s| (float_in(s, -1000.0, 1000.0), float_in(s, -1000.0, 1000.0))),
        ("large over small", |s| (any_float(s).abs().min(f32::MAX), float_in(s, -1.0, 1.0))),
    ]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let slow_too = args.iter().any(|a| a == "--slow");
    let n = args.iter().position(|a| a == "--pairs").map_or(1 << 26, |i| args[i + 1].parse().expect("--pairs <n>"));
    let names: Vec<&str> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || args[i - 1] != "--pairs"))
        .map(|(_, a)| a.as_str())
        .collect();
    let wanted = |name: &str| names.is_empty() || names.contains(&name);
    let started = Instant::now();
    let mut ok = true;
    for f in F1::ALL {
        if wanted(f.name()) {
            ok &= every_input(f);
            if slow_too {
                ok &= slow_sample(f);
            }
        }
    }
    if wanted("atan2f") {
        ok &= pairs("atan2f", crt::atan2f, reference::atan2f, &atan2_shapes(), n, true);
        if slow_too {
            let shapes: [(&str, Shape); 1] = [("finite non-zero", |s| {
                let ok = |v: f32| v.is_finite() && v != 0.0;
                loop {
                    let (a, b) =
                        if next(s) & 1 == 0 { bits(s) } else { (float_in(s, -9.0, 9.0), float_in(s, -9.0, 9.0)) };
                    if ok(a) && ok(b) {
                        return (a, b);
                    }
                }
            })];
            ok &= pairs("atan2f double-double path", slow::atan2f, reference::atan2f, &shapes, 1 << 24, false);
        }
    }
    if wanted("powf") {
        ok &= pairs("powf", crt::powf, reference::powf, &pow_shapes(), n, true);
        if slow_too {
            let shapes: [(&str, Shape); 1] = [("x in (0, 4), y in [-30, 30]", |s| {
                loop {
                    let (x, y) = (float_in(s, 0.0, 4.0), float_in(s, -30.0, 30.0));
                    if x > 0.0 && x != 1.0 && y != 0.0 {
                        return (x, y);
                    }
                }
            })];
            ok &= pairs("powf double-double path", slow::pow_abs, reference::powf, &shapes, 1 << 24, false);
        }
    }
    if wanted("fmodf") {
        ok &= pairs("fmodf", crt::fmodf, reference::fmodf, &fmod_shapes(), n, true);
    }
    println!("all: {:.1} s", started.elapsed().as_secs_f64());
    if !ok {
        std::process::exit(1);
    }
}
