//! `mvs_math::f32::fmath` and `estimate` against the beta exe's own routines, called by address in a mapped copy of
//! the exe (`mvs_math_check::exe`): one argument over every float with the others fixed, plus random tuples.
//!
//! `exe-fmath [name...] [--exe <path>] [--samples <n>]`: the named checks (all when none is named); `--samples` sets the
//! random tuples per check (default 2^26). Exits 1 on any mismatch. Windows only.

#[cfg(not(windows))]
fn main() {
    eprintln!("exe-fmath maps a Windows exe: Windows only");
}

#[cfg(windows)]
fn main() {
    imp::main();
}

#[cfg(windows)]
mod imp {
    use mvs_math::f32::estimate::inv_sqrt;
    use mvs_math::f32::fmath::{self, RandomStream};
    use mvs_math::f32::vector;
    use mvs_math_check::exe::{EXE, Image};
    use mvs_math_check::hardware::{cvtss2si, cvttss2si};
    use mvs_math_check::par::{Tally, run};
    use mvs_math_check::{any_float, float_in, next, same};
    use std::time::Instant;

    type V3 = [f32; 3];

    /// The exe routines by address and signature.
    struct Exe {
        image: Image,
    }

    macro_rules! call {
        ($self:ident, $va:expr, $ty:ty, $($arg:expr),*) => {{
            // SAFETY: `$ty` is the routine's signature, read off its code (the Win64 convention: the first four
            // arguments in rcx / xmm0 .. r9 / xmm3 by position, the rest on the stack, a struct result by a pointer
            // in the first slot, `this` before it).
            unsafe { $self.image.func::<$ty>($va)($($arg),*) }
        }};
    }

    impl Exe {
        fn atan2(&self, y: f32, x: f32) -> f32 {
            call!(self, 0x1_412c_8e50, unsafe extern "C" fn(f32, f32) -> f32, y, x)
        }
        fn fmod(&self, x: f32, y: f32) -> f32 {
            call!(self, 0x1_412c_e930, unsafe extern "C" fn(f32, f32) -> f32, x, y)
        }
        fn normalize_axis(&self, a: f32) -> f32 {
            call!(self, 0x1_42b2_a8a0, unsafe extern "C" fn(f32) -> f32, a)
        }
        fn clamp_axis(&self, a: f32) -> f32 {
            call!(self, 0x1_42b1_2900, unsafe extern "C" fn(f32) -> f32, a)
        }
        fn rotator_vector(&self, r: V3) -> V3 {
            let mut out = [0f32; 3];
            call!(self, 0x1_4137_4280, unsafe extern "C" fn(*const V3, *mut V3) -> *mut V3, &r, &mut out);
            out
        }
        fn vector_rotation(&self, v: V3) -> V3 {
            let mut out = [0f32; 3];
            call!(self, 0x1_4136_e340, unsafe extern "C" fn(*const V3, *mut V3) -> *mut V3, &v, &mut out);
            out
        }
        fn rotate_angle_axis(&self, v: V3, angle: f32, axis: V3) -> V3 {
            let mut out = [0f32; 3];
            call!(
                self,
                0x1_4136_dfb0,
                unsafe extern "C" fn(*const V3, *mut V3, f32, *const V3) -> *mut V3,
                &v,
                &mut out,
                angle,
                &axis
            );
            out
        }
        fn quat_rotator(&self, q: [f32; 4]) -> V3 {
            let mut out = [0f32; 3];
            call!(self, 0x1_4136_e550, unsafe extern "C" fn(*const [f32; 4], *mut V3) -> *mut V3, &q, &mut out);
            out
        }
        fn rotator_quaternion(&self, r: V3) -> [f32; 4] {
            let mut out = [0f32; 4];
            call!(self, 0x1_4136_a6d0, unsafe extern "C" fn(*const V3, *mut [f32; 4]) -> *mut [f32; 4], &r, &mut out);
            out
        }
        fn finterp_to(&self, c: f32, t: f32, dt: f32, s: f32) -> f32 {
            call!(self, 0x1_4135_cc10, unsafe extern "C" fn(f32, f32, f32, f32) -> f32, c, t, dt, s)
        }
        fn finterp_constant_to(&self, c: f32, t: f32, dt: f32, s: f32) -> f32 {
            call!(self, 0x1_4135_cbb0, unsafe extern "C" fn(f32, f32, f32, f32) -> f32, c, t, dt, s)
        }
        fn interp3(&self, va: u64, c: V3, t: V3, dt: f32, s: f32) -> V3 {
            let mut out = [0f32; 3];
            call!(
                self,
                va,
                unsafe extern "C" fn(*mut V3, *const V3, *const V3, f32, f32) -> *mut V3,
                &mut out,
                &c,
                &t,
                dt,
                s
            );
            out
        }
        fn map_range_clamped(&self, v: f32, ia: f32, ib: f32, oa: f32, ob: f32) -> f32 {
            call!(self, 0x1_42b2_9590, unsafe extern "C" fn(f32, f32, f32, f32, f32) -> f32, v, ia, ib, oa, ob)
        }
        /// RandomFloatInRangeFromStream(min, max, stream): advances the stream.
        fn frand_range(&self, min: f32, max: f32, stream: &mut [i32; 2]) -> f32 {
            call!(self, 0x1_42b2_be20, unsafe extern "C" fn(f32, f32, *mut [i32; 2]) -> f32, min, max, stream)
        }
        /// The routine at 142b1bbe0: (p - o) . n n + o, n the safe normal of `v` (InvSqrt inline).
        fn plane(&self, p: V3, o: V3, v: V3) -> V3 {
            let mut out = [0f32; 3];
            call!(
                self,
                0x1_42b1_bbe0,
                unsafe extern "C" fn(*mut V3, *const V3, *const V3, *const V3) -> *mut V3,
                &mut out,
                &p,
                &o,
                &v
            );
            out
        }
        /// FAnimationRuntime::AdvanceTime (142816220): (&looping, &move, &time, &end) -> the result; time is in and out.
        fn advance_time(&self, looping: bool, mv: f32, time: f32, end: f32) -> (u32, f32) {
            let (flag, mut t) = (u8::from(looping), time);
            let r = call!(
                self,
                0x1_4281_6220,
                unsafe extern "C" fn(*const u8, *const f32, *mut f32, *const f32) -> u32,
                &flag,
                &mv,
                &mut t,
                &end
            );
            (r, t)
        }
        /// CharacterMovement IsExceedingMaxSpeed (142970eb0): this (Velocity at +0xc4), MaxSpeed.
        fn is_exceeding_max_speed(&self, v: V3, m: f32) -> bool {
            let mut this = [0u8; 0x100];
            for (k, c) in v.iter().enumerate() {
                this[0xc4 + 4 * k..0xc8 + 4 * k].copy_from_slice(&c.to_le_bytes());
            }
            let r = call!(self, 0x1_4297_0eb0, unsafe extern "C" fn(*const u8, f32) -> u8, this.as_ptr(), m);
            r != 0
        }
        /// FMatrix::MakeFromX (141363360): (ret, &X) -> the rows X, Y, Z of the matrix (16 floats).
        fn make_from_x(&self, x: V3) -> [V3; 3] {
            let mut m = [0f32; 16];
            call!(self, 0x1_4136_3360, unsafe extern "C" fn(*mut [f32; 16], *const V3) -> *mut [f32; 16], &mut m, &x);
            rows(&m)
        }
        /// FMatrix::MakeFromXZ (141363b50): (ret, &X, &Z).
        fn make_from_xz(&self, x: V3, z: V3) -> [V3; 3] {
            let mut m = [0f32; 16];
            call!(
                self,
                0x1_4136_3b50,
                unsafe extern "C" fn(*mut [f32; 16], *const V3, *const V3) -> *mut [f32; 16],
                &mut m,
                &x,
                &z
            );
            rows(&m)
        }
        /// FMatrix::Rotator (14136e3c0): (&matrix, ret).
        fn matrix_rotator(&self, m: [V3; 3]) -> V3 {
            let mut full = [0f32; 16];
            for (r, row) in m.iter().enumerate() {
                full[4 * r..4 * r + 3].copy_from_slice(row);
            }
            let mut out = [0f32; 3];
            call!(self, 0x1_4136_e3c0, unsafe extern "C" fn(*const [f32; 16], *mut V3) -> *mut V3, &full, &mut out);
            out
        }
        /// RotatorFromAxisAndAngle's routine (142b2df90): (ret, &axis, angle degrees), the rotator of FQuat(Axis, Angle).
        fn rotator_from_axis_angle(&self, axis: V3, angle: f32) -> V3 {
            let mut out = [0f32; 3];
            call!(
                self,
                0x1_42b2_df90,
                unsafe extern "C" fn(*mut V3, *const V3, f32) -> *mut V3,
                &mut out,
                &axis,
                angle
            );
            out
        }
        /// The RandomFloat node (an exec thunk: this, the frame, the result), on a frame with no code.
        fn random_float_node(&self) -> f32 {
            let mut frame = [0u64; 32];
            let mut out = 0f32;
            call!(
                self,
                0x1_42f7_fcb0,
                unsafe extern "C" fn(*mut u8, *mut [u64; 32], *mut f32),
                std::ptr::null_mut(),
                &mut frame,
                &mut out
            );
            out
        }
    }

    /// The rows X, Y, Z of a matrix's 16 floats.
    fn rows(m: &[f32; 16]) -> [V3; 3] {
        [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]]
    }

    fn flat(m: &[V3; 3]) -> Vec<f32> {
        m.iter().flatten().copied().collect()
    }

    /// A vector of mixed floats, some of them special: zero, unit, an axis, the up-switch boundary, NaN.
    fn special_vec(s: &mut u64) -> V3 {
        match next(s) % 12 {
            0 => [0.0; 3],
            1 => [1.0, 0.0, 0.0],
            2 => [0.0, 0.0, if next(s) & 1 == 0 { 1.0 } else { -1.0 }],
            3 => {
                let z = f32::from_bits(0x3f7f_f972 - 2 + (next(s) % 5) as u32);
                [mixed(s, 1.0), mixed(s, 1.0), if next(s) & 1 == 0 { z } else { -z }]
            }
            4 => [0.0, 0.0, mixed(s, 5.0)],
            5 => [f32::NAN, mixed(s, 1.0), mixed(s, 1.0)],
            6 => [float_in(s, -1e-4, 1e-4), float_in(s, -1e-4, 1e-4), float_in(s, -1e-4, 1e-4)],
            7 => {
                let v = [float_in(s, -1.0, 1.0), float_in(s, -1.0, 1.0), float_in(s, -1.0, 1.0)];
                let n = vector::size(v);
                [v[0] / n, v[1] / n, v[2] / n]
            }
            _ => [mixed(s, 1000.0), mixed(s, 1000.0), mixed(s, 1000.0)],
        }
    }

    fn same_all(a: &[f32], b: &[f32]) -> bool {
        a.iter().zip(b).all(|(x, y)| same(*x, *y))
    }

    fn show(v: &[f32]) -> String {
        let parts: Vec<String> = v.iter().map(|x| format!("{x:e} ({:#010x})", x.to_bits())).collect();
        format!("[{}]", parts.join(", "))
    }

    struct Checks {
        exe: Exe,
        samples: u64,
        ok: bool,
    }

    impl Checks {
        fn report(&mut self, what: &str, inputs: u64, t: Tally, started: Instant) {
            println!("{what}: {inputs} inputs, {} mismatches ({:.1} s)", t.count, started.elapsed().as_secs_f64());
            for e in &t.examples {
                println!("  {e}");
            }
            self.ok &= t.count == 0;
        }

        /// `test(bits)` over every 32-bit pattern; a mismatch is Some(description).
        fn every(&mut self, what: &str, test: impl Fn(&Exe, u32) -> Option<String> + Sync) {
            let started = Instant::now();
            let exe = &self.exe;
            let parts = run(0, 1 << 32, 1 << 20, |lo, hi| {
                let mut t = Tally::default();
                for b in lo..hi {
                    if let Some(e) = test(exe, b as u32) {
                        t.add(|| e);
                    }
                }
                t
            });
            self.report(what, 1 << 32, Tally::merge(parts), started);
        }

        /// `test(state)` on `samples` random draws.
        fn sample(&mut self, what: &str, test: impl Fn(&Exe, &mut u64) -> Option<String> + Sync) {
            let started = Instant::now();
            let exe = &self.exe;
            let parts = run(0, self.samples, 1 << 16, |lo, hi| {
                let mut t = Tally::default();
                let mut state = 0x9e37_79b9_7f4a_7c15 ^ (lo + 1).wrapping_mul(0x2545_f491_4f6c_dd1d);
                for _ in lo..hi {
                    if let Some(e) = test(exe, &mut state) {
                        t.add(|| e);
                    }
                }
                t
            });
            self.report(what, self.samples, Tally::merge(parts), started);
        }
    }

    fn f1(name: &str, x: f32, got: f32, want: f32) -> Option<String> {
        (!same(got, want)).then(|| format!("{name}({x:e} = {:#010x}): exe {got:e}, port {want:e}", x.to_bits()))
    }

    fn f2(name: &str, a: f32, b: f32, got: f32, want: f32) -> Option<String> {
        (!same(got, want)).then(|| {
            format!(
                "{name}({}): exe {got:e} ({:#010x}), port {want:e} ({:#010x})",
                show(&[a, b]),
                got.to_bits(),
                want.to_bits()
            )
        })
    }

    fn vn(name: &str, input: &[f32], got: &[f32], want: &[f32]) -> Option<String> {
        (!same_all(got, want)).then(|| format!("{name}({}): exe {}, port {}", show(input), show(got), show(want)))
    }

    /// A float of random bits half the time, else one in [-r, r].
    fn mixed(s: &mut u64, r: f32) -> f32 {
        if next(s) & 1 == 0 { any_float(s) } else { float_in(s, -r, r) }
    }

    /// The routine at 142b1bbe0 written from its code, with `inv_sqrt` for the inline InvSqrt.
    fn plane_port(p: V3, o: V3, v: V3, zero: V3) -> V3 {
        let sq = (v[0] * v[0] + v[1] * v[1]) + v[2] * v[2];
        // ucomiss / jne: equal or unordered keeps v
        let n = if sq == 1.0 || sq.is_nan() {
            v
        } else if sq < f32::from_bits(0x322b_cc77) {
            zero
        } else {
            let s = inv_sqrt(sq);
            [v[0] * s, v[1] * s, v[2] * s]
        };
        let d = ((p[1] - o[1]) * n[1] + (p[0] - o[0]) * n[0]) + (p[2] - o[2]) * n[2];
        [n[0] * d + o[0], n[1] * d + o[1], n[2] * d + o[2]]
    }

    /// The exe's results on a few inputs per routine, as the bit patterns `mvs-math`'s quick tests hold.
    fn golden(e: &Exe) {
        let bits = |v: &[f32]| v.iter().map(|x| format!("{:#010x}", x.to_bits())).collect::<Vec<_>>().join(", ");
        let xs: [f32; 8] = [0.3, -1.7, 123.456, -359.9, 720.25, 1e-3, -45.0, 3.0e6];
        for (k, &x) in xs.iter().enumerate() {
            let y = xs[(k + 3) % 8];
            println!("atan2 [{}] -> {}", bits(&[y, x]), bits(&[e.atan2(y, x)]));
            println!("fmod [{}] -> {}", bits(&[x, 360.0]), bits(&[e.fmod(x, 360.0)]));
            println!("normalize_axis [{}] -> {}", bits(&[x]), bits(&[e.normalize_axis(x)]));
            println!("clamp_axis [{}] -> {}", bits(&[x]), bits(&[e.clamp_axis(x)]));
            let r = [x, y, xs[(k + 5) % 8]];
            println!("rotator_vector [{}] -> {}", bits(&r), bits(&e.rotator_vector(r)));
            println!("rotator_quaternion [{}] -> {}", bits(&r), bits(&e.rotator_quaternion(r)));
            println!("vector_rotation [{}] -> {}", bits(&r), bits(&e.vector_rotation(r)));
            let q = [x * 0.001, y * 0.001, 0.5, -0.25];
            println!("quat_rotator [{}] -> {}", bits(&q), bits(&e.quat_rotator(q)));
            println!(
                "finterp_to [{}] -> {}",
                bits(&[x, y, 1.0 / 60.0, 8.0]),
                bits(&[e.finterp_to(x, y, 1.0 / 60.0, 8.0)])
            );
            let fc = e.finterp_constant_to(x, y, 1.0 / 60.0, 300.0);
            println!("finterp_constant_to [{}] -> {}", bits(&[x, y, 1.0 / 60.0, 300.0]), bits(&[fc]));
            let t = [y, x, xs[(k + 1) % 8]];
            println!(
                "rinterp_to [{}] -> {}",
                bits(&[r[0], r[1], r[2], t[0], t[1], t[2]]),
                bits(&e.interp3(0x1_4136_ab80, r, t, 1.0 / 60.0, 5.0))
            );
            println!(
                "vinterp_to [{}] -> {}",
                bits(&[r[0], r[1], r[2], t[0], t[1], t[2]]),
                bits(&e.interp3(0x1_4137_3640, r, t, 1.0 / 60.0, 5.0))
            );
            println!(
                "map_range_clamped [{}] -> {}",
                bits(&[x, -3.0, 250.0, 1.0, 0.25]),
                bits(&[e.map_range_clamped(x, -3.0, 250.0, 1.0, 0.25)])
            );
            let axis = [0.6, 0.0, 0.8];
            println!(
                "rotate_angle_axis [{}] -> {}",
                bits(&[r[0], r[1], r[2], x]),
                bits(&e.rotate_angle_axis(r, x, axis))
            );
            let v = [x, y, 0.5];
            let m = e.make_from_x(v);
            println!("make_from_x [{}] -> {}", bits(&v), bits(&flat(&m)));
            let z = [y, -0.25, x];
            println!("make_from_xz [{} {}] -> {}", bits(&v), bits(&z), bits(&flat(&e.make_from_xz(v, z))));
            println!("matrix_rotator [{}] -> {}", bits(&flat(&m)), bits(&e.matrix_rotator(m)));
            println!(
                "rotator_from_axis_angle [{} {}] -> {}",
                bits(&v),
                bits(&[x]),
                bits(&e.rotator_from_axis_angle(v, x))
            );
            let (r, t) = e.advance_time(k % 2 == 0, x, y, 1.5);
            println!("advance_time [{} {}] -> {r} {}", k % 2 == 0, bits(&[x, y, 1.5]), bits(&[t]));
            let max = xs[(k + 2) % 8];
            println!("is_exceeding_max_speed [{} {}] -> {}", bits(&v), bits(&[max]), e.is_exceeding_max_speed(v, max));
            println!("plane [{}] -> {}", bits(&v), bits(&e.plane([1.0, 0.0, 0.0], [0.0; 3], v)));
        }
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].clone());
        let path = value("--exe").unwrap_or_else(|| EXE.to_string());
        let samples = value("--samples").map_or(1 << 26, |n| n.parse().expect("--samples <n>"));
        let names: Vec<&str> = args
            .iter()
            .enumerate()
            .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--")))
            .map(|(_, a)| a.as_str())
            .collect();
        let wanted = |n: &str| names.is_empty() || names.contains(&n);

        let image = Image::load(&path).unwrap_or_else(|e| panic!("{e}"));
        println!("mapped {path}; {} vector-constant initializers copied", image.initializers);
        // The vector constants the packed routines read (VectorMod360, VectorSinCos, the DEG_TO_RAD_HALF factor),
        // and FVector::ZeroVector.
        image
            .check_globals(&[
                (0x1_44ed_32a0, 0x43b4_0000),
                (0x1_44ed_3290, 0x4b00_0000),
                (0x1_44ed_2f30, 0x7fff_ffff),
                (0x1_44ed_32b0, 0x3c0e_fa35),
                (0x1_44ed_2f70, 0x3e22_f983),
                (0x1_44ed_2f60, 0x40c9_0fdb),
                (0x1_44ed_2f50, 0x8000_0000),
                (0x1_44ed_2f40, 0x4049_0fdb),
                (0x1_44ed_2f20, 0x3fc9_0fdb),
                (0x1_44ed_2f00, 0xbf80_0000),
                (0x1_44ed_2f10, 0x3f80_0000),
                (0x1_44ed_32d0, 0x4334_0000),
                (0x1_44ed_32f0, 0),
                (0x1_44ed_32f4, 0),
                (0x1_44ed_32f8, 0),
                // FRotationMatrix (140da5540): pi/180, 1/2pi, pi/2, sign, abs, 2pi, pi, -1, 1
                (0x1_44e7_0160, 0x3c8e_fa35),
                (0x1_44e7_0150, 0x3e22_f983),
                (0x1_44e7_0100, 0x3fc9_0fdb),
                (0x1_44e7_0130, 0x8000_0000),
                (0x1_44e7_0110, 0x7fff_ffff),
                (0x1_44e7_0140, 0x40c9_0fdb),
                (0x1_44e7_0120, 0x4049_0fdb),
                (0x1_44e7_00f0, 0xbf80_0000),
                (0x1_44e6_ffb0, 0x3f80_0000),
            ])
            .unwrap_or_else(|e| panic!("{e}"));
        let mut c = Checks { exe: Exe { image }, samples, ok: true };
        let started = Instant::now();

        if wanted("atan2") {
            c.every("Atan2(y, 1)", |e, b| {
                let y = f32::from_bits(b);
                f1("Atan2(y, 1)", y, e.atan2(y, 1.0), fmath::atan2(y, 1.0))
            });
            c.every("Atan2(1, x)", |e, b| {
                let x = f32::from_bits(b);
                f1("Atan2(1, x)", x, e.atan2(1.0, x), fmath::atan2(1.0, x))
            });
            c.sample("Atan2 pairs", |e, s| {
                let (y, x) = (mixed(s, 1000.0), mixed(s, 1000.0));
                f2("Atan2", y, x, e.atan2(y, x), fmath::atan2(y, x))
            });
        }
        if wanted("fmod") {
            c.every("Fmod(x, 360)", |e, b| {
                let x = f32::from_bits(b);
                f1("Fmod(x, 360)", x, e.fmod(x, 360.0), fmath::fmod(x, 360.0))
            });
            c.every("Fmod(1000, y)", |e, b| {
                let y = f32::from_bits(b);
                f1("Fmod(1000, y)", y, e.fmod(1000.0, y), fmath::fmod(1000.0, y))
            });
            c.sample("Fmod pairs", |e, s| {
                let (x, y) = (mixed(s, 1e5), mixed(s, 1000.0));
                f2("Fmod", x, y, e.fmod(x, y), fmath::fmod(x, y))
            });
        }
        if wanted("axis") {
            c.every("NormalizeAxis", |e, b| {
                let x = f32::from_bits(b);
                f1("NormalizeAxis", x, e.normalize_axis(x), fmath::normalize_axis(x))
            });
            c.every("ClampAxis", |e, b| {
                let x = f32::from_bits(b);
                f1("ClampAxis", x, e.clamp_axis(x), fmath::clamp_axis(x))
            });
        }
        if wanted("vector") {
            for lane in 0..2 {
                c.every(&format!("FRotator::Vector, {} over every float", ["pitch", "yaw"][lane]), |e, b| {
                    let mut r = [37.5, -121.25, 3.0];
                    r[lane] = f32::from_bits(b);
                    vn("Vector", &r, &e.rotator_vector(r), &fmath::rotator_vector(r))
                });
            }
            c.sample("FRotator::Vector", |e, s| {
                let r = [mixed(s, 720.0), mixed(s, 720.0), mixed(s, 720.0)];
                vn("Vector", &r, &e.rotator_vector(r), &fmath::rotator_vector(r))
            });
        }
        if wanted("rotation") {
            c.every("FVector::Rotation, X over every float", |e, b| {
                let v = [f32::from_bits(b), -3.5, 0.75];
                vn("Rotation", &v, &e.vector_rotation(v), &fmath::vector_rotation(v))
            });
            c.sample("FVector::Rotation", |e, s| {
                let v = [mixed(s, 1000.0), mixed(s, 1000.0), mixed(s, 1000.0)];
                vn("Rotation", &v, &e.vector_rotation(v), &fmath::vector_rotation(v))
            });
        }
        if wanted("rotate") {
            c.every("RotateAngleAxis, the angle over every float", |e, b| {
                let (v, axis) = ([3.0, -1.5, 7.25], [0.6, 0.0, 0.8]);
                let a = f32::from_bits(b);
                vn("RotateAngleAxis", &[a], &e.rotate_angle_axis(v, a, axis), &fmath::rotate_angle_axis(v, a, axis))
            });
            c.sample("RotateAngleAxis", |e, s| {
                let v = [mixed(s, 1000.0), mixed(s, 1000.0), mixed(s, 1000.0)];
                let a = mixed(s, 720.0);
                let axis = [float_in(s, -1.0, 1.0), float_in(s, -1.0, 1.0), float_in(s, -1.0, 1.0)];
                let input = [v[0], v[1], v[2], a, axis[0], axis[1], axis[2]];
                vn("RotateAngleAxis", &input, &e.rotate_angle_axis(v, a, axis), &fmath::rotate_angle_axis(v, a, axis))
            });
        }
        if wanted("rotator") {
            for lane in 0..4 {
                c.every(&format!("FQuat::Rotator, {} over every float", ["x", "y", "z", "w"][lane]), |e, b| {
                    let mut q = [0.25, -0.5, 0.125, 0.75];
                    q[lane] = f32::from_bits(b);
                    vn("Rotator", &q, &e.quat_rotator(q), &fmath::quat_rotator(q))
                });
            }
            c.sample("FQuat::Rotator, unit quaternions", |e, s| {
                let mut q =
                    [float_in(s, -1.0, 1.0), float_in(s, -1.0, 1.0), float_in(s, -1.0, 1.0), float_in(s, -1.0, 1.0)];
                let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
                q = q.map(|v| v / n);
                vn("Rotator", &q, &e.quat_rotator(q), &fmath::quat_rotator(q))
            });
            c.sample("FQuat::Rotator, near the poles", |e, s| {
                // x z - y w near +-0.5: x = z, y = -w, both pairs near sqrt(1/2) / sqrt(2) = 0.5
                let a = 0.5 + float_in(s, -1e-3, 1e-3);
                let b = float_in(s, -1e-3, 1e-3);
                let sign = if next(s) & 1 == 0 { 1.0 } else { -1.0 };
                let q = [a, b, a * sign, float_in(s, -1.0, 1.0)];
                vn("Rotator", &q, &e.quat_rotator(q), &fmath::quat_rotator(q))
            });
            c.sample("FQuat::Rotator, random", |e, s| {
                let q = [mixed(s, 2.0), mixed(s, 2.0), mixed(s, 2.0), mixed(s, 2.0)];
                vn("Rotator", &q, &e.quat_rotator(q), &fmath::quat_rotator(q))
            });
        }
        if wanted("quaternion") {
            for lane in 0..3 {
                c.every(
                    &format!("FRotator::Quaternion, {} over every float", ["pitch", "yaw", "roll"][lane]),
                    |e, b| {
                        let mut r = [37.5, -121.25, 3.0];
                        r[lane] = f32::from_bits(b);
                        vn("Quaternion", &r, &e.rotator_quaternion(r), &fmath::rotator_quaternion(r))
                    },
                );
            }
            c.sample("FRotator::Quaternion", |e, s| {
                let r = [mixed(s, 720.0), mixed(s, 720.0), mixed(s, 720.0)];
                vn("Quaternion", &r, &e.rotator_quaternion(r), &fmath::rotator_quaternion(r))
            });
        }
        if wanted("interp") {
            c.every("FInterpTo, current over every float", |e, b| {
                let x = f32::from_bits(b);
                f1("FInterpTo", x, e.finterp_to(x, 10.0, 1.0 / 60.0, 8.0), fmath::finterp_to(x, 10.0, 1.0 / 60.0, 8.0))
            });
            c.every("FInterpConstantTo, current over every float", |e, b| {
                let x = f32::from_bits(b);
                let (got, want) = (
                    e.finterp_constant_to(x, 10.0, 1.0 / 60.0, 300.0),
                    fmath::finterp_constant_to(x, 10.0, 1.0 / 60.0, 300.0),
                );
                f1("FInterpConstantTo", x, got, want)
            });
            c.sample("FInterpTo", |e, s| {
                let a = [mixed(s, 1000.0), mixed(s, 1000.0), mixed(s, 0.1), mixed(s, 50.0)];
                vn(
                    "FInterpTo",
                    &a,
                    &[e.finterp_to(a[0], a[1], a[2], a[3])],
                    &[fmath::finterp_to(a[0], a[1], a[2], a[3])],
                )
            });
            c.sample("FInterpConstantTo", |e, s| {
                let a = [mixed(s, 1000.0), mixed(s, 1000.0), mixed(s, 0.1), mixed(s, 1000.0)];
                let got = e.finterp_constant_to(a[0], a[1], a[2], a[3]);
                vn("FInterpConstantTo", &a, &[got], &[fmath::finterp_constant_to(a[0], a[1], a[2], a[3])])
            });
            c.sample("VInterpTo", |e, s| {
                let a: [f32; 8] = std::array::from_fn(|k| if k == 6 { mixed(s, 0.1) } else { mixed(s, 100.0) });
                let (cur, tgt) = ([a[0], a[1], a[2]], [a[3], a[4], a[5]]);
                let got = e.interp3(0x1_4137_3640, cur, tgt, a[6], a[7]);
                vn("VInterpTo", &a, &got, &fmath::vinterp_to(cur, tgt, a[6], a[7]))
            });
            c.sample("RInterpTo", |e, s| {
                let a: [f32; 8] = std::array::from_fn(|k| if k == 6 { mixed(s, 0.1) } else { mixed(s, 720.0) });
                let (cur, tgt) = ([a[0], a[1], a[2]], [a[3], a[4], a[5]]);
                let got = e.interp3(0x1_4136_ab80, cur, tgt, a[6], a[7]);
                vn("RInterpTo", &a, &got, &fmath::rinterp_to(cur, tgt, a[6], a[7]))
            });
            c.sample("RInterpTo, small differences", |e, s| {
                let a: [f32; 8] = std::array::from_fn(|k| match k {
                    0..=2 => float_in(s, -360.0, 360.0),
                    3..=5 => float_in(s, -1e-3, 1e-3),
                    6 => float_in(s, 0.0, 0.05),
                    _ => float_in(s, 0.0, 30.0),
                });
                let (cur, tgt) = ([a[0], a[1], a[2]], [a[0] + a[3], a[1] + a[4], a[2] + a[5]]);
                let got = e.interp3(0x1_4136_ab80, cur, tgt, a[6], a[7]);
                vn("RInterpTo", &a, &got, &fmath::rinterp_to(cur, tgt, a[6], a[7]))
            });
        }
        if wanted("maprange") {
            c.every("MapRangeClamped, value over every float", |e, b| {
                let v = f32::from_bits(b);
                f1(
                    "MapRangeClamped",
                    v,
                    e.map_range_clamped(v, -3.0, 250.0, 1.0, 0.25),
                    fmath::map_range_clamped(v, -3.0, 250.0, 1.0, 0.25),
                )
            });
            c.sample("MapRangeClamped", |e, s| {
                let a = [mixed(s, 1000.0), mixed(s, 1000.0), mixed(s, 1000.0), mixed(s, 1000.0), mixed(s, 1000.0)];
                let got = e.map_range_clamped(a[0], a[1], a[2], a[3], a[4]);
                vn("MapRangeClamped", &a, &[got], &[fmath::map_range_clamped(a[0], a[1], a[2], a[3], a[4])])
            });
        }
        if wanted("random") {
            c.every("RandomFloatInRangeFromStream, every seed", |e, b| {
                let (min, max) = (-3.0, 7.5);
                let mut stream = [17, b as i32];
                let got = e.frand_range(min, max, &mut stream);
                let mut port = RandomStream { initial_seed: 17, seed: b as i32 };
                let want = port.frand_range(min, max);
                (!same(got, want) || stream[1] != port.seed).then(|| {
                    format!(
                        "seed {b:#010x}: exe {got:e} next seed {:#010x}, port {want:e} {:#010x}",
                        stream[1], port.seed
                    )
                })
            });
            // The RandomFloat node calls the C runtime's rand(), whose state is per thread: the same seed, then
            // rand() itself, on this thread.
            let started = Instant::now();
            let mut t = Tally::default();
            unsafe extern "C" {
                safe fn srand(seed: u32);
                safe fn rand() -> i32;
            }
            for seed in 0..64u32 {
                srand(seed);
                let node: Vec<f32> = (0..1 << 16).map(|_| c.exe.random_float_node()).collect();
                srand(seed);
                for (k, got) in node.iter().enumerate() {
                    let want = fmath::rand_to_float(rand());
                    if !same(*got, want) {
                        t.add(|| format!("seed {seed} draw {k}: exe {got:e}, port {want:e}"));
                    }
                }
            }
            c.report("RandomFloat node", 64 << 16, t, started);
        }
        if wanted("rounding") {
            // The Floor, Round and FTrunc nodes are exec thunks that read their argument through the VM; their
            // arithmetic is the instruction itself, checked on this CPU.
            c.every("FloorToInt, RoundToInt, TruncToInt against cvtss2si / cvttss2si", |_, b| {
                let x = f32::from_bits(b);
                let want = [cvtss2si(x * 2.0 - 0.5) >> 1, cvtss2si(x * 2.0 + 0.5) >> 1, cvttss2si(x)];
                let got = [fmath::floor_to_int(x), fmath::round_to_int(x), fmath::trunc_to_int(x)];
                (got != want).then(|| format!("{x:e} ({b:#010x}): instructions {want:?}, port {got:?}"))
            });
        }
        if wanted("invsqrt") {
            let zero = [0.0; 3];
            c.every("InvSqrt in the routine at 142b1bbe0, v = (x, 0.5, 0)", |e, b| {
                let (p, o, v) = ([1.0, 0.0, 0.0], [0.0; 3], [f32::from_bits(b), 0.5, 0.0]);
                vn("142b1bbe0", &v, &e.plane(p, o, v), &plane_port(p, o, v, zero))
            });
            c.sample("InvSqrt in the routine at 142b1bbe0", |e, s| {
                let a: [f32; 9] = std::array::from_fn(|_| mixed(s, 100.0));
                let (p, o, v) = ([a[0], a[1], a[2]], [a[3], a[4], a[5]], [a[6], a[7], a[8]]);
                vn("142b1bbe0", &a, &e.plane(p, o, v), &plane_port(p, o, v, zero))
            });
        }
        if wanted("matrix") {
            // MakeFromX and MakeFromXZ carry the inline GetSafeNormal (X, Y, Z), so they check `safe_normal` too.
            for lane in 0..3 {
                c.every(&format!("MakeFromX, X.{} over every float", ["x", "y", "z"][lane]), |e, b| {
                    let mut x = [0.3, -1.5, 0.75];
                    x[lane] = f32::from_bits(b);
                    vn("MakeFromX", &x, &flat(&e.make_from_x(x)), &flat(&vector::make_from_x(x)))
                });
            }
            c.sample("MakeFromX", |e, s| {
                let x = special_vec(s);
                vn("MakeFromX", &x, &flat(&e.make_from_x(x)), &flat(&vector::make_from_x(x)))
            });
            c.every("MakeFromXZ, Z.x over every float", |e, b| {
                let (x, z) = ([0.3, -1.5, 0.75], [f32::from_bits(b), 0.25, 0.5]);
                let input = [x[0], x[1], x[2], z[0], z[1], z[2]];
                vn("MakeFromXZ", &input, &flat(&e.make_from_xz(x, z)), &flat(&vector::make_from_xz(x, z)))
            });
            c.sample("MakeFromXZ", |e, s| {
                let (x, z) = (special_vec(s), special_vec(s));
                let input = [x[0], x[1], x[2], z[0], z[1], z[2]];
                vn("MakeFromXZ", &input, &flat(&e.make_from_xz(x, z)), &flat(&vector::make_from_xz(x, z)))
            });
            c.sample("MakeFromXZ, Z near X", |e, s| {
                let x = special_vec(s);
                let z = [x[0] * 1.000_000_1 + float_in(s, -1e-7, 1e-7), x[1], x[2] * 0.999_999_9];
                let input = [x[0], x[1], x[2], z[0], z[1], z[2]];
                vn("MakeFromXZ", &input, &flat(&e.make_from_xz(x, z)), &flat(&vector::make_from_xz(x, z)))
            });
            for lane in 0..3 {
                c.every(&format!("FMatrix::Rotator, X.{} over every float", ["x", "y", "z"][lane]), |e, b| {
                    let mut m = [[0.3, -0.5, 0.8], [0.6, 0.7, -0.2], [-0.4, 0.1, 0.9]];
                    m[0][lane] = f32::from_bits(b);
                    vn("FMatrix::Rotator", &flat(&m), &e.matrix_rotator(m), &vector::matrix_rotator(m))
                });
            }
            c.sample("FMatrix::Rotator", |e, s| {
                let m = [special_vec(s), special_vec(s), special_vec(s)];
                vn("FMatrix::Rotator", &flat(&m), &e.matrix_rotator(m), &vector::matrix_rotator(m))
            });
            c.sample("MakeRotFromX (MakeFromX then Rotator)", |e, s| {
                let x = special_vec(s);
                vn("MakeRotFromX", &x, &e.matrix_rotator(e.make_from_x(x)), &vector::rot_from_x(x))
            });
            c.sample("MakeRotFromXZ (MakeFromXZ then Rotator)", |e, s| {
                let (x, z) = (special_vec(s), special_vec(s));
                let input = [x[0], x[1], x[2], z[0], z[1], z[2]];
                vn("MakeRotFromXZ", &input, &e.matrix_rotator(e.make_from_xz(x, z)), &vector::rot_from_xz(x, z))
            });
        }
        if wanted("axisangle") {
            c.every("RotatorFromAxisAndAngle, the angle over every float", |e, b| {
                let axis = [0.3, -1.5, 0.75];
                let a = f32::from_bits(b);
                vn("142b2df90", &[a], &e.rotator_from_axis_angle(axis, a), &vector::rotator_from_axis_angle(axis, a))
            });
            for lane in 0..3 {
                c.every(
                    &format!("RotatorFromAxisAndAngle, axis.{} over every float", ["x", "y", "z"][lane]),
                    |e, b| {
                        let mut axis = [0.3, -0.5, 0.75];
                        axis[lane] = f32::from_bits(b);
                        let a = 37.5;
                        vn(
                            "142b2df90",
                            &axis,
                            &e.rotator_from_axis_angle(axis, a),
                            &vector::rotator_from_axis_angle(axis, a),
                        )
                    },
                );
            }
            c.sample("RotatorFromAxisAndAngle", |e, s| {
                let (axis, a) = (special_vec(s), mixed(s, 720.0));
                let input = [axis[0], axis[1], axis[2], a];
                vn("142b2df90", &input, &e.rotator_from_axis_angle(axis, a), &vector::rotator_from_axis_angle(axis, a))
            });
        }
        if wanted("advance") {
            let check = |e: &Exe, looping: bool, mv: f32, t: f32, end: f32| {
                let (r, got) = e.advance_time(looping, mv, t, end);
                let mut port = t;
                let want = vector::advance_time(looping, mv, &mut port, end);
                (r != want as u32 || !same(got, port)).then(|| {
                    format!("AdvanceTime({looping}, {mv:e}, {t:e}, {end:e}): exe {r} {got:e}, port {want:?} {port:e}")
                })
            };
            for looping in [false, true] {
                c.every(&format!("AdvanceTime, looping {looping}, time over every float"), |e, b| {
                    check(e, looping, 0.25, f32::from_bits(b), 1.5)
                });
                c.every(&format!("AdvanceTime, looping {looping}, end over every float"), |e, b| {
                    check(e, looping, 0.25, 3.7, f32::from_bits(b))
                });
                c.sample(&format!("AdvanceTime, looping {looping}"), |e, s| {
                    let (mv, t) = (mixed(s, 5.0), mixed(s, 5.0));
                    let end = match next(s) % 6 {
                        0 => 0.0,
                        1 => f32::NAN,
                        2 => -mixed(s, 5.0).abs(),
                        _ => float_in(s, 0.0, 5.0),
                    };
                    check(e, looping, mv, t, end)
                });
            }
        }
        if wanted("exceeding") {
            let check = |e: &Exe, v: V3, m: f32| {
                let (got, want) = (e.is_exceeding_max_speed(v, m), vector::is_exceeding_max_speed(v, m));
                (got != want).then(|| format!("IsExceedingMaxSpeed({v:?}, {m:e}): exe {got}, port {want}"))
            };
            c.every("IsExceedingMaxSpeed, MaxSpeed over every float", |e, b| {
                check(e, [30.0, -40.0, 12.0], f32::from_bits(b))
            });
            c.every("IsExceedingMaxSpeed, Velocity.x over every float", |e, b| {
                check(e, [f32::from_bits(b), -4.0, 1.5], 40.0)
            });
            c.sample("IsExceedingMaxSpeed", |e, s| {
                let v = [mixed(s, 100.0), mixed(s, 100.0), mixed(s, 100.0)];
                // half the draws sit near the 1.01 boundary
                let m = if next(s) & 1 == 0 {
                    mixed(s, 100.0)
                } else {
                    vector::size(v) / 1.005_f32.sqrt() * float_in(s, 0.999_999, 1.000_001)
                };
                check(e, v, m)
            });
        }
        if names.contains(&"golden") {
            golden(&c.exe);
        }
        println!("all: {:.1} s", started.elapsed().as_secs_f64());
        if !c.ok {
            std::process::exit(1);
        }
    }
}
