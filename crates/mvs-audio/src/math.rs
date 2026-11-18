//! The math the interpreter and the effects call, with V8's bits where the tapes compare them: never
//! `f64::powf` / `exp` and the like directly (the platform's libm differs by machine).

mod v8;

/// x to the power y.
// Unverified: the recorded Math.pow is V8's, and Node 22's is neither fdlibm's e_pow.c nor the platform's (the C
// runtime's gives different last bits again): mvs_math's fdlibm pow differs from it in the last bit on about 4 inputs
// in 100. Two f64 gains a bit apart round to the same float on all but about one sample in a billion.
pub fn pow(x: f64, y: f64) -> f64 {
    mvs_math::libm::pow(x, y)
}

/// e to the power x: V8's (fdlibm's e_exp.c, the same bits as Node 22's on 400,000 random arguments).
pub fn exp(x: f64) -> f64 {
    v8::exp(x)
}
pub fn sin(x: f64) -> f64 {
    mvs_math::libm::sin(x)
}

pub fn cos(x: f64) -> f64 {
    mvs_math::libm::cos(x)
}

pub fn log10(x: f64) -> f64 {
    mvs_math::libm::log10(x)
}

pub fn tan(x: f64) -> f64 {
    mvs_math::libm::tan(x)
}

/// dB to a linear gain.
pub fn db_to_gain(db: f64) -> f64 {
    pow(10.0, db / 20.0)
}
