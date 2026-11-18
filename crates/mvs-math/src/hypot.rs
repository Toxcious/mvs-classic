//! Math.hypot of two, three and four numbers as V8 computes it (builtins/math.tq MathHypot), step for step.

#[inline]
fn any_inf2(a: f64, b: f64) -> bool {
    a.abs() == f64::INFINITY || b.abs() == f64::INFINITY
}

pub fn hypot2(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return if any_inf2(a, b) { f64::INFINITY } else { f64::NAN };
    }
    let (a, b) = (a.abs(), b.abs());
    let mut max = 0.0;
    if a > max {
        max = a;
    }
    if b > max {
        max = b;
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut n = a / max;
    let sum = n * n;
    n = b / max;
    (sum + n * n).sqrt() * max
}

pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    if a.is_nan() || b.is_nan() || c.is_nan() {
        return if any_inf2(a, b) || c.abs() == f64::INFINITY { f64::INFINITY } else { f64::NAN };
    }
    let (a, b, c) = (a.abs(), b.abs(), c.abs());
    let mut max = 0.0;
    if a > max {
        max = a;
    }
    if b > max {
        max = b;
    }
    if c > max {
        max = c;
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut n = a / max;
    let mut s = n * n;
    let mut sum = s;
    let mut comp = 0.0;
    n = b / max;
    s = n * n - comp;
    let pre = sum + s;
    comp = pre - sum - s;
    sum = pre;
    n = c / max;
    (sum + (n * n - comp)).sqrt() * max
}

pub fn hypot4(a: f64, b: f64, c: f64, d: f64) -> f64 {
    if a.is_nan() || b.is_nan() || c.is_nan() || d.is_nan() {
        return if any_inf2(a, b) || any_inf2(c, d) { f64::INFINITY } else { f64::NAN };
    }
    let (a, b, c, d) = (a.abs(), b.abs(), c.abs(), d.abs());
    let mut max = 0.0;
    for v in [a, b, c, d] {
        if v > max {
            max = v;
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut n = a / max;
    let mut s = n * n;
    let mut sum = s;
    let mut comp = 0.0;
    for v in [b, c] {
        n = v / max;
        s = n * n - comp;
        let pre = sum + s;
        comp = pre - sum - s;
        sum = pre;
    }
    n = d / max;
    (sum + (n * n - comp)).sqrt() * max
}
