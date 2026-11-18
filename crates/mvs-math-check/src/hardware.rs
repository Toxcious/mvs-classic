//! The x86 instructions the float32 module reproduces, run on this CPU: the reciprocal estimates and the
//! float-to-integer conversions.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(target_arch = "x86_64")]
pub fn rsqrtss(x: f32) -> f32 {
    // SAFETY: SSE is part of the x86_64 baseline.
    unsafe { _mm_cvtss_f32(_mm_rsqrt_ss(_mm_set_ss(x))) }
}

#[cfg(target_arch = "x86_64")]
pub fn rcpss(x: f32) -> f32 {
    // SAFETY: SSE is part of the x86_64 baseline.
    unsafe { _mm_cvtss_f32(_mm_rcp_ss(_mm_set_ss(x))) }
}

/// rsqrtps on four lanes.
#[cfg(target_arch = "x86_64")]
pub fn rsqrtps(x: [f32; 4]) -> [f32; 4] {
    let mut out = [0f32; 4];
    // SAFETY: SSE is part of the x86_64 baseline; the loads and stores are unaligned forms on arrays of four floats.
    unsafe { _mm_storeu_ps(out.as_mut_ptr(), _mm_rsqrt_ps(_mm_loadu_ps(x.as_ptr()))) };
    out
}

/// rcpps on four lanes.
#[cfg(target_arch = "x86_64")]
pub fn rcpps(x: [f32; 4]) -> [f32; 4] {
    let mut out = [0f32; 4];
    // SAFETY: SSE is part of the x86_64 baseline; the loads and stores are unaligned forms on arrays of four floats.
    unsafe { _mm_storeu_ps(out.as_mut_ptr(), _mm_rcp_ps(_mm_loadu_ps(x.as_ptr()))) };
    out
}

/// cvtss2si under the thread's MXCSR (the default: to nearest, ties to even).
#[cfg(target_arch = "x86_64")]
pub fn cvtss2si(x: f32) -> i32 {
    // SAFETY: SSE is part of the x86_64 baseline.
    unsafe { _mm_cvtss_si32(_mm_set_ss(x)) }
}

#[cfg(target_arch = "x86_64")]
pub fn cvttss2si(x: f32) -> i32 {
    // SAFETY: SSE is part of the x86_64 baseline.
    unsafe { _mm_cvttss_si32(_mm_set_ss(x)) }
}

/// The CPU's brand string (CPUID 0x80000002-4).
#[cfg(target_arch = "x86_64")]
pub fn cpu_name() -> String {
    let mut bytes = Vec::new();
    for leaf in 0x8000_0002u32..=0x8000_0004 {
        let r = __cpuid(leaf);
        for w in [r.eax, r.ebx, r.ecx, r.edx] {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
    }
    String::from_utf8_lossy(&bytes).trim_matches(char::from(0)).trim().to_string()
}
