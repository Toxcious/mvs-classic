//! The material preshader evaluator (FMaterialPreshader, the exe's evaluator FUN_142ba2510): a uniform expression's
//! value from its byte program, as FillUniformBuffer 142ba5110 runs it for each vector and scalar preshader. Ports
//! the preshader evaluator (its numbers: f64, the libm of mvs-math), so the rows match the
//! ones the import evaluated (`Uniforms::values`).
//!
//! Opcodes: 0x01 zero, 0x02 constant, 0x03 / 0x04 scalar / vector parameter, 0x05-0x0b Add Sub Mul Div Fmod Min Max,
//! 0x0c Clamp, 0x0d-0x12 Sin Cos Tan Asin Acos Atan, 0x13 Atan2, 0x14 Dot, 0x15 Cross, 0x16 Sqrt, 0x18 Saturate,
//! 0x19 Abs, 0x1a Floor, 0x1b Ceil, 0x1c Round, 0x1d Trunc, 0x1e Sign, 0x1f Frac, 0x20 x - Trunc(x), 0x21 Log2,
//! 0x22 Log10, 0x23 ComponentSwizzle, 0x24 AppendVector, 0x25 TextureSize (the unnamed form only: the referenced
//! texture itself). Div clamps |b| to at least 1e-5 keeping b's sign (0 -> +1e-5); Fmod is FMath::Fmod (0 when
//! |b| <= 1e-8); Asin / Acos clamp to [-1, 1]. 0x26-0x29 are not used by the build's materials.
// Unverified: the evaluator computes in f64 in double precision; the exe's runs in float32 (a later stage of the port's
// numerics, as the sim's).

use mvs_math::{js, libm};

use crate::material::{Material, Preshader};
use crate::{Error, Result};

type V4 = [f64; 4];

fn fdiv(x: f64, y: f64) -> f64 {
    x / if y.abs() < 1e-5 { if y < 0.0 { -1e-5 } else { 1e-5 } } else { y }
}

fn fmod(x: f64, y: f64) -> f64 {
    let ay = y.abs();
    if ay <= 1e-8 {
        return 0.0;
    }
    let q = x / y;
    let r = x - (if q.abs() < 8388608.0 { q.trunc() } else { q }) * y;
    js::min(js::max(if x.abs() < (x - r).abs() { 0.0 } else { r }, -ay), ay)
}

fn clamp1(x: f64) -> f64 {
    if x >= -1.0 { if x >= 1.0 { 1.0 } else { x } } else { -1.0 }
}

fn unary(op: u8) -> Option<fn(f64) -> f64> {
    Some(match op {
        0x0d => libm::sin,
        0x0e => libm::cos,
        0x0f => libm::tan,
        0x10 => |x| libm::asin(clamp1(x)),
        0x11 => |x| libm::acos(clamp1(x)),
        0x12 => libm::atan,
        0x16 => f64::sqrt,
        0x18 => |x| if x >= 0.0 { js::min(x, 1.0) } else { 0.0 },
        0x19 => f64::abs,
        0x1a => f64::floor,
        0x1b => f64::ceil,
        0x1c => |x| (x + 0.5).floor(),
        0x1d => f64::trunc,
        0x1e => |x| {
            if x > 0.0 {
                1.0
            } else if x >= 0.0 {
                0.0
            } else {
                -1.0
            }
        },
        0x1f => |x| x - x.floor(),
        0x20 => |x| x - x.trunc(),
        0x21 => libm::log2,
        0x22 => libm::log10,
        _ => return None,
    })
}

fn binary(op: u8) -> fn(f64, f64) -> f64 {
    match op {
        0x05 => |a, b| a + b,
        0x06 => |a, b| a - b,
        0x07 => |a, b| a * b,
        0x08 => fdiv,
        0x09 => fmod,
        0x0a => js::min,
        _ => js::max,
    }
}

/// What a preshader reads besides its program: the parameter values and the referenced textures' sizes.
pub trait PreshaderInputs {
    fn scalar(&self, k: usize) -> f64;
    fn vector(&self, k: usize) -> V4;
    /// [SizeX, SizeY, SizeZ, 0] of referenced texture k.
    fn texture_size(&self, k: usize) -> Option<V4>;
}

/// Runs one preshader program (`code`, its range already cut from the section) and returns its float4.
pub fn eval(code: &[u8], inputs: &dyn PreshaderInputs, stack: &mut Vec<V4>) -> Result<V4> {
    let bad = |why: String| Error::format("preshader", why);
    let base = stack.len();
    let mut i = 0;
    let byte = |i: usize| code.get(i).copied().ok_or_else(|| bad(format!("cut short at {i}")));
    let u16at = |i: usize| -> Result<u16> {
        code.get(i..i + 2).map(|s| u16::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| bad("cut short".into()))
    };
    let f32at = |i: usize| -> Result<f64> {
        code.get(i..i + 4)
            .map(|s| f32::from_le_bytes(s.try_into().unwrap()) as f64)
            .ok_or_else(|| bad("cut short".into()))
    };
    let pop =
        |stack: &mut Vec<V4>| stack.pop().filter(|_| stack.len() >= base).ok_or_else(|| bad("stack underflow".into()));
    while i < code.len() {
        let op = code[i];
        i += 1;
        match op {
            0x01 => stack.push([0.0; 4]),
            0x02 => {
                stack.push([f32at(i)?, f32at(i + 4)?, f32at(i + 8)?, f32at(i + 12)?]);
                i += 16;
            }
            0x03 => {
                let v = inputs.scalar(u16at(i)? as usize);
                i += 2;
                stack.push([v; 4]);
            }
            0x04 => {
                let v = inputs.vector(u16at(i)? as usize);
                i += 2;
                stack.push(v);
            }
            0x05..=0x0b => {
                let (b, a) = (pop(stack)?, pop(stack)?);
                let f = binary(op);
                stack.push([f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2]), f(a[3], b[3])]);
            }
            0x0c => {
                let (hi, lo, x) = (pop(stack)?, pop(stack)?, pop(stack)?);
                let mut out = [0.0; 4];
                for c in 0..4 {
                    let v = x[c];
                    out[c] = if lo[c] <= v { if hi[c] <= v { hi[c] } else { v } } else { lo[c] };
                }
                stack.push(out);
            }
            0x13 => {
                let (b, a) = (pop(stack)?, pop(stack)?);
                stack.push([
                    libm::atan2(a[0], b[0]),
                    libm::atan2(a[1], b[1]),
                    libm::atan2(a[2], b[2]),
                    libm::atan2(a[3], b[3]),
                ]);
            }
            0x14 => {
                // the byte is a component mask (1, 3, 7, 15)
                let n = byte(i)?;
                i += 1;
                let (b, a) = (pop(stack)?, pop(stack)?);
                let mut d = 0.0;
                for c in 0..4 {
                    if n >> c & 1 != 0 {
                        d += a[c] * b[c];
                    }
                }
                stack.push([d; 4]);
            }
            0x15 => {
                let m = byte(i)?;
                i += 1;
                let (b, a) = (pop(stack)?, pop(stack)?);
                let on = |v: &V4, c: usize| if m >> c & 1 != 0 { v[c] } else { 0.0 };
                let (a0, a1, a2, b0, b1, b2) = (on(&a, 0), on(&a, 1), on(&a, 2), on(&b, 0), on(&b, 1), on(&b, 2));
                stack.push([a1 * b2 - a2 * b1, a2 * b0 - a0 * b2, a0 * b1 - a1 * b0, 0.0]);
            }
            0x23 => {
                let n = byte(i)?;
                // the indices are int8 (read as signed bytes)
                let idx = [byte(i + 1)? as i8, byte(i + 2)? as i8, byte(i + 3)? as i8, byte(i + 4)? as i8];
                i += 5;
                let v = pop(stack)?;
                // Deviation: an index outside 0..4 is an error here; a JavaScript reader would take v[i] as undefined
                // and go on with NaN. No material of the build has one, and a wrong entry is better refused than drawn.
                let get = |k: usize| -> Result<f64> {
                    usize::try_from(idx[k])
                        .ok()
                        .and_then(|j| v.get(j).copied())
                        .ok_or_else(|| bad(format!("swizzle index {}", idx[k])))
                };
                // one element broadcasts to all four; two or three leave the rest 0
                let out = if n == 0 {
                    [0.0; 4]
                } else if n == 1 {
                    [get(0)?; 4]
                } else {
                    let x0 = get(0)?;
                    let x1 = get(1)?;
                    let x2 = if n > 2 { get(2)? } else { 0.0 };
                    let x3 = if n > 3 { get(3)? } else { 0.0 };
                    [x0, x1, x2, x3]
                };
                stack.push(out);
            }
            0x24 => {
                // AppendVector: a's first n components, then b's
                let n = byte(i)? as usize;
                i += 1;
                let (b, mut a) = (pop(stack)?, pop(stack)?);
                let m = n.min(4);
                a[m..].copy_from_slice(&b[..4 - m]);
                stack.push(a);
            }
            0x25 if u16at(i)? == 0 => {
                let k = code
                    .get(i + 7..i + 11)
                    .map(|s| i32::from_le_bytes(s.try_into().unwrap()))
                    .ok_or_else(|| bad("cut short".into()))?;
                let v = usize::try_from(k)
                    .ok()
                    .and_then(|k| inputs.texture_size(k))
                    .ok_or_else(|| bad(format!("no size for referenced texture {k}")))?;
                stack.push(v);
                i += 11;
            }
            _ => match unary(op) {
                Some(f) => {
                    if stack.len() <= base {
                        return Err(bad("stack underflow".into()));
                    }
                    let x = stack.last_mut().unwrap();
                    *x = [f(x[0]), f(x[1]), f(x[2]), f(x[3])];
                }
                None => return Err(bad(format!("opcode 0x{op:02x}"))),
            },
        }
    }
    if stack.len() != base + 1 {
        let n = stack.len() as isize - base as isize;
        stack.truncate(base);
        return Err(bad(format!("stack {n}")));
    }
    Ok(stack.pop().unwrap())
}

impl Material {
    fn run(&self, p: Preshader, inputs: &dyn PreshaderInputs, stack: &mut Vec<V4>) -> Result<V4> {
        let code = &self.preshader[p.0 as usize..(p.0 + p.1) as usize];
        eval(code, inputs, stack).map_err(|e| Error::format(&self.path, e.to_string()))
    }

    /// The material buffer's float4 rows (the vector preshaders' results, then the scalar ones packed by four) for
    /// a set of parameter values: FillUniformBuffer's order. The resources after them are the renderer's.
    pub fn uniform_rows(&self, inputs: &dyn PreshaderInputs) -> Result<Vec<[f64; 4]>> {
        let u = &self.uniforms;
        let mut stack = Vec::with_capacity(16);
        let mut out = Vec::with_capacity(u.vector_preshaders.len() + u.scalar_preshaders.len().div_ceil(4));
        for &p in &u.vector_preshaders {
            out.push(self.run(p, inputs, &mut stack)?);
        }
        for chunk in u.scalar_preshaders.chunks(4) {
            let mut row = [0.0; 4];
            for (c, &p) in chunk.iter().enumerate() {
                row[c] = self.run(p, inputs, &mut stack)?[0];
            }
            out.push(row);
        }
        Ok(out)
    }

    /// The parameter values the entry holds (the instance chain's over the map's defaults) as preshader inputs.
    pub fn default_inputs(&self) -> impl PreshaderInputs + '_ {
        Defaults(self)
    }
}

struct Defaults<'a>(&'a Material);

impl PreshaderInputs for Defaults<'_> {
    fn scalar(&self, k: usize) -> f64 {
        self.0.uniforms.scalars[k].value
    }
    fn vector(&self, k: usize) -> V4 {
        self.0.uniforms.vectors[k].value
    }
    fn texture_size(&self, k: usize) -> Option<V4> {
        self.0.uniforms.texture_sizes.as_ref()?.get(k).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoInputs;

    impl PreshaderInputs for NoInputs {
        fn scalar(&self, _: usize) -> f64 {
            0.0
        }
        fn vector(&self, _: usize) -> V4 {
            [0.0; 4]
        }
        fn texture_size(&self, _: usize) -> Option<V4> {
            None
        }
    }

    /// Opcode 0x02 with four float32 constants.
    fn c(v: [f32; 4]) -> Vec<u8> {
        let mut out = vec![0x02];
        v.iter().for_each(|x| out.extend(x.to_le_bytes()));
        out
    }

    /// The opcodes no material of the build uses, against what the oracle's evaluator
    /// returns for the same programs (a probe run of it, its f64 results' bits).
    #[test]
    fn matches_the_ts_evaluator() {
        let a = c([0.75, -2.5, 3.25, -0.4]);
        let b = c([-1.5, 0.3, 0.0, 7.1]);
        let cat = |parts: &[&[u8]]| parts.concat();
        let cases: Vec<(&str, Vec<u8>, [u64; 4])> = vec![
            ("zero", vec![0x01], [0, 0, 0, 0]),
            (
                "fmod",
                cat(&[&c([7.5, -7.5, 5.25, 1e-9]), &c([2.0, 2.0, -1.5, 3.0]), &[0x09]]),
                [0x3ff8000000000000, 0xbff8000000000000, 0x3fe8000000000000, 0x3e112e0be0000000],
            ),
            ("fmod by |b| <= 1e-8", cat(&[&a, &c([1e-9, -1e-9, 0.0, 2e-8]), &[0x09]]), [0, 0, 0, 0]),
            ("min", cat(&[&a, &b, &[0x0a]]), [0xbff8000000000000, 0xc004000000000000, 0, 0xbfd99999a0000000]),
            (
                "tan",
                cat(&[&a, &[0x0f]]),
                [0x3fedcfa36110eeec, 0x3fe7e79b4e00bb15, 0x3fbbdc8bf445936c, 0xbfdb0f0b51682226],
            ),
            (
                "asin",
                cat(&[&c([0.5, -2.0, 1.0, -0.25]), &[0x10]]),
                [0x3fe0c152382d7366, 0xbff921fb54442d18, 0x3ff921fb54442d18, 0xbfd02be9ce0b87cd],
            ),
            (
                "acos",
                cat(&[&c([0.5, -2.0, 1.0, -0.25]), &[0x11]]),
                [0x3ff0c152382d7366, 0x400921fb54442d18, 0, 0x3ffd2cf5c7c70f0c],
            ),
            (
                "atan2",
                cat(&[&a, &b, &[0x13]]),
                [0x40056c6e7397f5ae, 0xbff738cd04ca066c, 0x3ff921fb54442d18, 0xbfacd08a5f527832],
            ),
            ("cross xyz", cat(&[&a, &b, &[0x15, 7]]), [0xbfef333348000000, 0xc013800000000000, 0xc00c333332000000, 0]),
            ("cross xy", cat(&[&a, &b, &[0x15, 3]]), [0x8000000000000000, 0x8000000000000000, 0xc00c333332000000, 0]),
            (
                "round",
                cat(&[&c([2.5, -2.5, -0.5, 1.49]), &[0x1c]]),
                [0x4008000000000000, 0xc000000000000000, 0, 0x3ff0000000000000],
            ),
            (
                "trunc",
                cat(&[&c([2.7, -2.7, -0.5, 0.0]), &[0x1d]]),
                [0x4000000000000000, 0xc000000000000000, 0x8000000000000000, 0],
            ),
            ("sign", cat(&[&c([2.7, -2.7, -0.0, 0.0]), &[0x1e]]), [0x3ff0000000000000, 0xbff0000000000000, 0, 0]),
            (
                "x - trunc(x)",
                cat(&[&c([2.7, -2.7, -0.5, 3.0]), &[0x20]]),
                [0x3fe6666680000000, 0xbfe6666680000000, 0xbfe0000000000000, 0],
            ),
            (
                "log10",
                cat(&[&c([1000.0, 0.5, 2.0, 7.0]), &[0x22]]),
                [0x4008000000000000, 0xbfd34413509f79ff, 0x3fd34413509f79ff, 0x3feb0b0b0b78cc3f],
            ),
        ];
        let mut stack = Vec::new();
        for (name, code, want) in cases {
            let got = eval(&code, &NoInputs, &mut stack).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(got.map(f64::to_bits), want, "{name}: {got:?}");
            assert!(stack.is_empty());
        }
    }

    #[test]
    fn refuses_a_swizzle_index_out_of_range() {
        let code = [c([1.0, 2.0, 3.0, 4.0]), vec![0x23, 2, 1, 0xff, 0, 0]].concat();
        assert!(eval(&code, &NoInputs, &mut Vec::new()).is_err());
        let code = [c([1.0, 2.0, 3.0, 4.0]), vec![0x23, 2, 3, 1, 0, 0]].concat();
        assert_eq!(eval(&code, &NoInputs, &mut Vec::new()).unwrap(), [4.0, 2.0, 0.0, 0.0]);
    }
}
