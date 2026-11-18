//! The output: the mixer driven by the default device (WASAPI) or pumped by the caller (offline).

pub mod wasapi;

use std::io::Write;

use crate::mix::Mixer;

/// What runs the mixer.
pub(crate) enum Run {
    /// Held for its Drop, which stops the thread.
    Wasapi(#[expect(dead_code, reason = "held for its Drop, which stops the render thread")] wasapi::Wasapi),
    /// The mixer and what it rendered so far, interleaved.
    Offline { mixer: Box<Mixer>, out: Vec<f32> },
}

/// Writes interleaved f32 samples as a 32-bit float WAV.
pub fn write_wav(path: &std::path::Path, rate: u32, channels: u16, samples: &[f32]) -> std::io::Result<()> {
    let data = u32::try_from(samples.len() * 4).map_err(|_| std::io::Error::other("a WAV under 4 GB"))?;
    let mut o = Vec::with_capacity(58 + samples.len() * 4);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&(50 + data).to_le_bytes());
    o.extend_from_slice(b"WAVE");
    // fmt: WAVE_FORMAT_IEEE_FLOAT with the cbSize field, then a fact chunk (as the format asks of non-PCM)
    o.extend_from_slice(b"fmt ");
    o.extend_from_slice(&18u32.to_le_bytes());
    o.extend_from_slice(&3u16.to_le_bytes());
    o.extend_from_slice(&channels.to_le_bytes());
    o.extend_from_slice(&rate.to_le_bytes());
    o.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
    o.extend_from_slice(&(channels * 4).to_le_bytes());
    o.extend_from_slice(&32u16.to_le_bytes());
    o.extend_from_slice(&0u16.to_le_bytes());
    o.extend_from_slice(b"fact");
    o.extend_from_slice(&4u32.to_le_bytes());
    let frames = data / 4 / u32::from(channels.max(1));
    o.extend_from_slice(&frames.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        o.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::File::create(path)?.write_all(&o)
}
