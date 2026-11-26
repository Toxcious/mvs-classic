//! (the port's own) Wind's picture: the approved look of `tools/looks/wind` (`look/wind.hlsl`), a pass over the
//! scene drawn while the sim's gust (`mvs_sim::world::own_rules::gust_at`) warns or blows.

use std::path::Path;

use crate::Result;
use crate::gpu::Gpu;
use crate::look::{Look, LookFile};

const SHADER: &str = include_str!("look/wind.hlsl");

/// The store's textures the look reads: the M010 wind line mask and the noise streaks.
const TEXTURES: [&str; 2] = [
    "/Game/Panda_Main/Maps/M010/Textures/M010_WindLine_A_01_Mask.M010_WindLine_A_01_Mask",
    "/Game/Panda_Main/VFX/Textures/T_NoiseStreaks.T_NoiseStreaks",
];

/// The gust this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindAt {
    /// Seconds since its warning started.
    pub age: f32,
    /// Its push (0..1): 0 through the warning.
    pub blow: f32,
    /// Its way (sim x): -1 or 1.
    pub dir: f32,
    /// The warning's length (seconds).
    pub warn: f32,
    /// The look's clock (seconds): its streaks and dust.
    pub time: f32,
}

/// Wind's compiled look.
#[derive(Debug)]
pub struct Wind {
    pub(crate) look: Look,
}

impl Wind {
    pub fn load(gpu: &Gpu, assets: &crate::assets::Assets) -> Result<Wind> {
        let file = LookFile {
            textures: TEXTURES.map(str::to_owned).to_vec(),
            params: vec![[0.0; 4]],
            screen: vec!["Wind".to_owned()],
            ..LookFile::default()
        };
        Ok(Wind { look: Look::build(gpu, assets, file, Some(("wind.hlsl", SHADER)), Path::new("wind"))? })
    }

    /// Its parameters set to `at`.
    pub(crate) fn place(&mut self, at: WindAt) {
        self.look.file.params[0] = [at.age, at.blow, at.dir, at.warn];
        self.look.time = at.time;
    }
}
