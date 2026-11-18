//! The texture quality setting: sg.TextureQuality's levels 0..=4 and what each does here,
//! r.MaxAnisotropy of the level's `[TextureQuality@<n>]` section (BaseScalability; the last level is `@Cine`), the
//! cap of an anisotropic sampler.
//!
//! Every level keeps every mip: r.Streaming.MipBias is not added to a texture's bias (r.Streaming.UsePerTextureBias
//! is 1) and only caps the top mips the streamer may cut under pool pressure.
// Deviation: the streaming pool (r.Streaming.PoolSize) is not modelled: the port loads per scope, not by screen size,
// so no level cuts a mip.

use std::sync::atomic::{AtomicU8, Ordering};

/// r.MaxAnisotropy by level.
const MAX_ANISOTROPY: [u8; 5] = [0, 2, 4, 8, 8];

/// sg.TextureQuality's start value in the exe (140729e60).
const DEFAULT_LEVEL: u8 = 3;

static LEVEL: AtomicU8 = AtomicU8::new(DEFAULT_LEVEL);

/// The level in force: samplers made after the call take its anisotropy.
pub(crate) fn set_level(level: u8) {
    LEVEL.store(level.min(4), Ordering::Relaxed);
}

/// The anisotropy cap of the level in force (at least 1, D3D11's smallest).
pub(crate) fn max_anisotropy() -> u8 {
    anisotropy_of(LEVEL.load(Ordering::Relaxed))
}

/// The anisotropy cap of `level`.
fn anisotropy_of(level: u8) -> u8 {
    MAX_ANISOTROPY[usize::from(level.min(4))].max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_cap_anisotropy() {
        // (the level in force is the process's: a test that set it would race the renderer's tests)
        for (level, want) in [(0, 1), (1, 2), (2, 4), (3, 8), (4, 8), (9, 8)] {
            assert_eq!(anisotropy_of(level), want);
        }
        assert_eq!(anisotropy_of(DEFAULT_LEVEL), 8);
    }
}
