//! (the port's own) Rising Lava's picture: the approved look of `tools/looks/rising-lava` (`look/lava.hlsl`) drawn
//! at the sim's lava surface (`mvs_sim::world::own_rules::lava_level`). The lava sea, two sheets of embers over it,
//! the heat's shimmer and glow over the scene, and four orange lights just over the surface that light the fighters.

use std::path::Path;

use crate::Result;
use crate::gpu::Gpu;
use crate::look::{Blend, Look, LookFile, LookLight, Shape};

const SHADER: &str = include_str!("look/lava.hlsl");

/// The glow's colour and reach over the surface (cm): Params[1].
const GLOW: [f32; 4] = [1.0, 0.36, 0.07, 300.0];
/// The lava's half extents (cm): across the stage (UE Y) and in depth (UE X).
const HALF_ACROSS: f64 = 9000.0;
const HALF_DEEP: f64 = 8000.0;
/// The ember sheets: their depth (UE X), their middle over the surface and their half extents.
const EMBER_DEPTHS: [f64; 2] = [-700.0, 260.0];
const EMBER_OVER: f64 = 750.0;
const EMBER_HALF: [f64; 2] = [6000.0, 800.0];
/// The lights: their depth (UE X), their places across the stage from its middle, their height over the surface,
/// colour and radius.
const LIGHT_DEPTH: f64 = 600.0;
const LIGHT_ACROSS: [f64; 4] = [-2400.0, -800.0, 800.0, 2400.0];
const LIGHT_OVER: f64 = 150.0;
const LIGHT_COLOR: [f32; 3] = [12.0, 3.6, 0.6];
const LIGHT_RADIUS: f32 = 1500.0;

/// Where the lava is this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LavaAt {
    /// The surface's height (UE Z, cm).
    pub level: f64,
    /// The stage's middle across (UE Y, cm).
    pub middle: f64,
    /// The look's clock (seconds): its flow, bubbles and embers.
    pub time: f32,
}

/// Rising Lava's compiled look.
#[derive(Debug)]
pub struct Lava {
    pub(crate) look: Look,
}

impl Lava {
    pub fn load(gpu: &Gpu, assets: &crate::assets::Assets) -> Result<Lava> {
        let file = file(LavaAt { level: 0.0, middle: 0.0, time: 0.0 });
        Ok(Lava { look: Look::build(gpu, assets, file, Some(("lava.hlsl", SHADER)), Path::new("rising lava"))? })
    }

    /// Its shapes and parameters moved to `at`.
    pub(crate) fn place(&mut self, at: LavaAt) {
        let f = &mut self.look.file;
        for s in &mut f.shapes {
            let over = if s.entry == "Lava" { 0.0 } else { EMBER_OVER };
            s.center[1] = at.middle;
            s.center[2] = at.level + over;
        }
        f.params[0][0] = at.level as f32;
        self.look.time = at.time;
    }

    /// Its lights at `at`.
    pub(crate) fn lights(at: LavaAt) -> impl Iterator<Item = crate::scene::Light> {
        LIGHT_ACROSS.into_iter().enumerate().map(move |(k, y)| {
            LookLight {
                at: [LIGHT_DEPTH, at.middle + y, at.level + LIGHT_OVER],
                color: LIGHT_COLOR,
                radius: LIGHT_RADIUS,
                velocity: [0.0; 3],
            }
            .light(k, 0.0)
        })
    }
}

/// The look's file for the lava at `at` (its shapes still: `place` moves them with the surface frame by frame).
fn file(at: LavaAt) -> LookFile {
    let shape = |entry: &str, center: [f64; 3], u: [f64; 3], v: [f64; 3], blend| Shape {
        entry: entry.to_owned(),
        center,
        u,
        v,
        blend,
        velocity: [0.0; 3],
    };
    let mut shapes =
        vec![shape("Lava", [0.0, at.middle, at.level], [0.0, HALF_ACROSS, 0.0], [-HALF_DEEP, 0.0, 0.0], Blend::Opaque)];
    for x in EMBER_DEPTHS {
        shapes.push(shape(
            "Embers",
            [x, at.middle, at.level + EMBER_OVER],
            [0.0, EMBER_HALF[0], 0.0],
            [0.0, 0.0, EMBER_HALF[1]],
            Blend::Add,
        ));
    }
    LookFile {
        shader: None,
        // (the surface's height, still: Params[0].y is the preview's rise a second)
        params: vec![[at.level as f32, 0.0, 0.0, 0.0], GLOW],
        shapes,
        screen: vec!["Heat".to_owned()],
        ..LookFile::default()
    }
}
