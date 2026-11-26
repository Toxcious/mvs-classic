//! (the port's own) Flood's and Storm Flood's pictures: the approved looks of `tools/looks/flood-tropical`
//! (`look/flood_tropical.hlsl`) and `tools/looks/flood-storm` (`look/flood_storm.hlsl`), screen passes over the scene
//! drawn at the sim's water (`mvs_sim::world::own_rules::water_at`): the water itself, refracted and hazed below its
//! surface, its front face just before the fighters; the storm's sky, rain and lightning. Where a fighter crosses the
//! surface the water splashes.

use std::path::Path;

use crate::Result;
use crate::gpu::Gpu;
use crate::look::{Look, LookFile};

const TROPICAL: &str = include_str!("look/flood_tropical.hlsl");
const STORM: &str = include_str!("look/flood_storm.hlsl");

/// The tropical water's rows: Params[0].w its front face (UE X, cm), Params[1] its waves, clarity and haze.
const TROPICAL_FRONT: f32 = 120.0;
const TROPICAL_WATER: [f32; 4] = [16.0, 1.0, 4500.0, 520.0];
/// The storm's: Params[0].w its wave scale, Params[1] its front plane (UE X, cm), first two strikes (s into the
/// rise) and rain.
const STORM_WAVES: f32 = 2.0;
const STORM_SKY: [f32; 4] = [420.0, 3.6, 5.2, 1.0];
/// The storm's fade in from the rise's start (s).
const STORM_FADE: f32 = 1.5;
/// A splash's life (s) and the crossing speed (cm/s) of a full one.
const SPLASH_LIFE: f32 = 1.6;
const SPLASH_FULL: f32 = 1800.0;

/// Which water.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloodStyle {
    Tropical,
    Storm,
}

/// A fighter's middle crossing the surface in a tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossing {
    /// Which fighter (one splash a tick each).
    pub key: u32,
    /// The tick it crossed in.
    pub tick: i32,
    /// Where across (UE Y, cm).
    pub y: f32,
    /// Its speed up or down (cm/s).
    pub speed: f32,
}

/// The water this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloodAt {
    pub style: FloodStyle,
    /// The surface (UE Z, cm).
    pub level: f64,
    /// The stage's middle across (UE Y, cm).
    pub middle: f64,
    /// How far risen (0 at the blast box's bottom, 1 settled).
    pub risen: f32,
    /// Seconds since the rise began, and the rise's length (s).
    pub age: f32,
    pub rise: f32,
    /// The look's clock (seconds): its waves, bubbles, rain and lightning.
    pub time: f32,
    /// The fighters that crossed the surface this tick.
    pub crossings: [Option<Crossing>; 8],
}

/// A splash on the surface.
#[derive(Clone, Copy, Debug, Default)]
struct Splash {
    y: f32,
    start: f32,
    strength: f32,
    seed: f32,
    /// The crossing it is (`Crossing` key and tick).
    of: (u32, i32),
}

/// The floods' compiled looks (each loaded when the match's rules may turn it on) and their splashes.
#[derive(Debug, Default)]
pub struct Flood {
    tropical: Option<Look>,
    storm: Option<Look>,
    splashes: [Option<Splash>; 4],
}

impl Flood {
    /// Compiles `style`'s look when it is not yet.
    pub fn load(&mut self, gpu: &Gpu, assets: &crate::assets::Assets, style: FloodStyle) -> Result<()> {
        let (slot, file, shader, name) = match style {
            FloodStyle::Tropical => (&mut self.tropical, "flood_tropical.hlsl", TROPICAL, "flood"),
            FloodStyle::Storm => (&mut self.storm, "flood_storm.hlsl", STORM, "storm flood"),
        };
        if slot.is_none() {
            let look = LookFile { params: vec![[0.0; 4]; 8], screen: screens(style), ..LookFile::default() };
            *slot = Some(Look::build(gpu, assets, look, Some((file, shader)), Path::new(name))?);
        }
        Ok(())
    }

    /// The look of `at`'s style, its parameters set to `at` and its splashes brought up to date; None when that
    /// style was not loaded.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn place(&mut self, at: &FloodAt) -> Option<&mut Look> {
        self.splash(at);
        let mut rows = [[0.0f32; 4]; 8];
        let level = at.level as f32;
        // (a rise time under 0: the game's water, not the preview's rise)
        rows[2] = [at.risen, at.age, at.middle as f32, 1.0];
        rows[3] = [at.rise, 0.0, 0.0, 0.0];
        match at.style {
            FloodStyle::Tropical => {
                rows[0] = [level, level, -1.0, TROPICAL_FRONT];
                rows[1] = TROPICAL_WATER;
            }
            FloodStyle::Storm => {
                rows[0] = [level, level, -1.0, STORM_WAVES];
                rows[1] = STORM_SKY;
                // In with the rise, out with the drain.
                let drain = if at.age > at.rise { at.risen } else { 1.0 };
                rows[2][3] = (at.age / STORM_FADE).clamp(0.0, 1.0).min(drain);
            }
        }
        for (k, s) in self.splashes.iter().enumerate() {
            rows[4 + k] = match s {
                Some(s) if at.time - s.start < SPLASH_LIFE => [s.y, at.time - s.start, s.strength, s.seed],
                _ => [0.0, -1.0, 0.0, 0.0],
            };
        }
        let look = match at.style {
            FloodStyle::Tropical => self.tropical.as_mut(),
            FloodStyle::Storm => self.storm.as_mut(),
        }?;
        look.file.params = rows.to_vec();
        look.time = at.time;
        Some(look)
    }

    /// A new splash for each crossing not seen yet, over the oldest.
    fn splash(&mut self, at: &FloodAt) {
        for c in at.crossings.iter().flatten() {
            if self.splashes.iter().flatten().any(|s| s.of == (c.key, c.tick)) {
                continue;
            }
            let k = (0..self.splashes.len())
                .min_by(|&a, &b| {
                    let t = |k: usize| self.splashes[k].map_or(f32::MIN, |s| s.start);
                    t(a).total_cmp(&t(b))
                })
                .expect("four slots");
            #[allow(clippy::cast_precision_loss)]
            let seed = ((c.tick as f32) * 0.618_034 + c.key as f32 * 0.414_214).fract();
            self.splashes[k] = Some(Splash {
                y: c.y,
                start: at.time,
                strength: (c.speed / SPLASH_FULL).clamp(0.3, 1.2),
                seed,
                of: (c.key, c.tick),
            });
        }
    }
}

/// The style's screen passes, in order.
fn screens(style: FloodStyle) -> Vec<String> {
    let s: &[&str] = match style {
        FloodStyle::Tropical => &["Water"],
        FloodStyle::Storm => &["Storm", "Flood", "Rain"],
    };
    s.iter().map(|&x| x.to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(time: f32, crossings: [Option<Crossing>; 8]) -> FloodAt {
        FloodAt {
            style: FloodStyle::Tropical,
            level: 0.0,
            middle: 0.0,
            risen: 1.0,
            age: 10.0,
            rise: 3.5,
            time,
            crossings,
        }
    }

    #[test]
    fn a_crossing_splashes_once_and_the_oldest_gives_way() {
        let mut f = Flood::default();
        let c = |key, tick| Some(Crossing { key, tick, y: 0.0, speed: 900.0 });
        let mut cs = [None; 8];
        cs[0] = c(0, 10);
        f.splash(&at(1.0, cs));
        // (the same tick drawn again: no second splash)
        f.splash(&at(1.01, cs));
        assert_eq!(f.splashes.iter().flatten().count(), 1);
        for (k, t) in [(1, 11), (2, 12), (3, 13), (0, 14)] {
            let mut cs = [None; 8];
            cs[0] = c(k, t);
            f.splash(&at(1.0 + t as f32, cs));
        }
        assert_eq!(f.splashes.iter().flatten().count(), 4);
        assert!(f.splashes.iter().flatten().all(|s| s.of != (0, 10)), "the oldest went first");
    }
}
