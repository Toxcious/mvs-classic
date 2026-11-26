//! (the port's own) Black Hole's picture: the approved look of `tools/looks/black-hole` (`look/hole.hlsl`), a pass
//! over the scene at the sim's hole (`mvs_sim::world::own_rules::hole_at`): its core, swirling disc and wisps, the
//! picture bent around it.

use std::path::Path;

use crate::Result;
use crate::gpu::Gpu;
use crate::look::{Look, LookFile};

const SHADER: &str = include_str!("look/hole.hlsl");
/// The look's shader file: its name and text.
const SHADER_FILE: (&str, &str) = ("hole.hlsl", SHADER);

/// The look's file as `Hole::load` compiles it.
pub(crate) fn look_file() -> LookFile {
    LookFile {
        textures: TEXTURES.map(str::to_owned).to_vec(),
        params: vec![[0.0; 4], GLOW, [0.0; 4]],
        screen: vec!["Lens".to_owned()],
        ..LookFile::default()
    }
}

/// The HLSL `Hole::load` compiles (`shaders.rs` lists it for Linux).
pub(crate) fn look_source() -> String {
    crate::look::source(&look_file(), Some(SHADER_FILE))
}

/// The store's textures the look reads: the dashed ring and the stringy noise.
const TEXTURES: [&str; 2] = [
    "/Game/Panda_Main/VFX/Textures/T_RingGraphic_01.T_RingGraphic_01",
    "/Game/Panda_Main/VFX/Textures/T_NoiseStringy02.T_NoiseStringy02",
];
/// The glow's colour and the disc's spin (radians a second): Params[1].
const GLOW: [f32; 4] = [0.75, 0.18, 2.4, 1.6];

/// The hole this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleAt {
    /// Its middle (UE, cm).
    pub at: [f64; 3],
    /// Its core's radius (cm).
    pub core: f32,
    /// How far open (0..1).
    pub open: f32,
    /// The look's clock (seconds): its spin.
    pub time: f32,
}

/// Black Hole's compiled look.
#[derive(Debug)]
pub struct Hole {
    pub(crate) look: Look,
}

impl Hole {
    pub fn load(gpu: &Gpu, assets: &crate::assets::Assets) -> Result<Hole> {
        Ok(Hole { look: Look::build(gpu, assets, look_file(), Some(SHADER_FILE), Path::new("black hole"))? })
    }

    /// Its parameters set to `at`.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn place(&mut self, at: HoleAt) {
        let p = &mut self.look.file.params;
        p[0] = [at.at[0] as f32, at.at[1] as f32, at.at[2] as f32, at.core];
        p[2][0] = at.open;
        self.look.time = at.time;
    }
}
