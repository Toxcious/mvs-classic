//! Cromulons (Map_M009, Map_M009_small: the keys m009 and m009_small): the block its scenery entry has of its own
//! Blueprint actors (the importer's scenery part cromulons.rs): `cromulons`.
//! This draws them as BeginPlay leaves them:
//! - the heads (Cromulon_Head_C): each its own dynamic instance of Cromulun_MAT with the parameters its construction
//!   script set (Color, Contrast, UseColorAlpha), their idle from the file (Head2's paused at 0: `start`);
//! - the helicopters (SchwiftyHelicopter_Blueprint_C) at their places, their animation from the file;
//! - the audio visualizer's 20 planes (`visualizer` on the instances), with the material's own values;
//! - the speakers (M009_DestructiblePlatformBP_C / _Child_C): their two thrusters (PS_SchwiftyThruster on the bones
//!   Engine / Engine1) from the level's start; their damage drawing from their sim state (cromulons/speakers.rs:
//!   mesh swaps, AnimBP, montages, materials, sparks and effects).
//!
//! What moves (Comp_Parallax of the heads, the helicopters and Sky_BGCard, MoveHelicopter, Head2's start, the
//! visualizer's Scroll) is cromulons/motion.rs's.

mod motion;
mod speakers;

use mvs_vfx::SystemId;
use serde::Deserialize;
use serde_json::Value;

use super::host::{SceneryHost, SkinRef};
use super::sky_arena::attachment_of;
use crate::Result;
use crate::stage::{bad, transform};
use crate::view::mul;
use motion::{Helicopter, Motion, MotionHead};
use mvs_view::M4;
use speakers::{SpeakerArt, SpeakerData, Speakers};

pub(super) use speakers::SPARK_SOUNDS;

#[derive(Clone, Debug, Deserialize)]
struct Head {
    skeletal: String,
    #[serde(default)]
    start: bool,
    material: String,
    scalars: indexmap::IndexMap<String, f64>,
    vectors: indexmap::IndexMap<String, Vec<f64>>,
}

#[derive(Clone, Debug, Deserialize)]
struct Thruster {
    system: String,
    bone: String,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

#[derive(Clone, Debug, Deserialize)]
struct Speaker {
    #[serde(flatten)]
    data: SpeakerData,
    thrusters: Vec<Thruster>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Block {
    heads: Vec<Head>,
    helicopters: Vec<Helicopter>,
    speakers: Vec<Speaker>,
    planes: usize,
    #[serde(default)]
    speaker_art: Option<SpeakerArt>,
}

/// A thruster: its speaker, the skeletal mesh and bone it is on, its relative transform there, its system.
#[derive(Debug)]
struct ThrusterFx {
    speaker: usize,
    skeletal: String,
    bone: String,
    relative: M4,
    system: String,
    fx: Option<SystemId>,
}

/// The `cromulons` block.
#[derive(Debug)]
pub struct Cromulons {
    motion: Box<Motion>,
    speakers: Option<Box<Speakers>>,
    thrusters: Vec<ThrusterFx>,
}

impl Cromulons {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<Cromulons> {
        let what = format!("scenery: {key}");
        let block: Block = serde_json::from_value(data.clone()).map_err(|e| bad(&what, e.to_string()))?;
        let heads: Vec<MotionHead> =
            block.heads.iter().map(|h| MotionHead { skeletal: h.skeletal.clone(), start: h.start }).collect();
        let motion = Box::new(Motion::make(&heads, &block.helicopters, block.planes, host)?);
        // the heads: the construction script's dynamic instance (CreateDynamicMaterialInstance of slot 0)
        for h in &block.heads {
            let k = host.skeletal_index(&h.skeletal).ok_or_else(|| bad(&what, format!("no head {}", h.skeletal)))?;
            if host.skeletals()[k].materials.as_slice() != [Some(h.material.clone())] {
                return Err(bad(&what, format!("{} head material", h.skeletal)));
            }
            let mesh = SkinRef::File(&h.skeletal);
            host.skeletal_material(mesh, 0, Some(&h.material));
            let scalars: Vec<(&str, f32)> = h.scalars.iter().map(|(n, v)| (n.as_str(), *v as f32)).collect();
            host.skeletal_scalars(mesh, 0, &scalars);
            let mut vectors = Vec::new();
            for (n, v) in &h.vectors {
                let &[r, g, b, a] = v.as_slice() else { return Err(bad(&what, format!("{} vector {n}", h.skeletal))) };
                vectors.push((n.as_str(), [r as f32, g as f32, b as f32, a as f32]));
            }
            host.skeletal_vectors(mesh, 0, &vectors);
        }
        // the thrusters' place: the bone's world transform (UE) x the component's relative one
        let mut thrusters = Vec::new();
        for (i, sp) in block.speakers.iter().enumerate() {
            for t in &sp.thrusters {
                if host.bone_world(&sp.data.skeletal, &t.bone).is_none() {
                    return Err(bad(&what, format!("{} has no bone {}", sp.data.skeletal, t.bone)));
                }
                thrusters.push(ThrusterFx {
                    speaker: i,
                    skeletal: sp.data.skeletal.clone(),
                    bone: t.bone.clone(),
                    relative: transform(t.loc, t.rot, t.scale),
                    system: t.system.clone(),
                    fx: None,
                });
            }
        }
        let list: Vec<SpeakerData> = block.speakers.iter().map(|s| s.data.clone()).collect();
        let speakers = match block.speaker_art {
            Some(art) => Some(Box::new(Speakers::make(art, &list, host)?)),
            None if !list.is_empty() => return Err(bad(&what, "speakers without speakerArt")),
            None => None,
        };
        Ok(Cromulons { motion, speakers, thrusters })
    }

    /// A thruster's world transform now.
    fn thruster_at(host: &SceneryHost, t: &ThrusterFx) -> mvs_vfx::Attachment {
        let bone = host.bone_world(&t.skeletal, &t.bone).expect("a thruster's bone");
        attachment_of(&mul(&t.relative, &bone))
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        // (the level's systems end with it: the thrusters are spawned again at each start)
        for t in &mut self.thrusters {
            let at = Self::thruster_at(host, t);
            t.fx = host.vfx().and_then(|v| v.spawn_level(&t.system, &at, false, &[], 0, false));
        }
        if let Some(s) = &mut self.speakers {
            for (i, fx) in s.thruster_fx.iter_mut().enumerate() {
                fx.clear();
                fx.extend(self.thrusters.iter().filter(|t| t.speaker == i).map(|t| t.fx));
            }
        }
        self.motion.begin(host);
        if let Some(s) = &mut self.speakers {
            s.begin(host);
        }
    }

    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        self.motion.tick(host, dt);
        if let Some(s) = &mut self.speakers {
            s.tick(host, dt);
        }
        // (the thrusters follow their bones: VfxPlayer's follow, before its tick)
        for t in &self.thrusters {
            let Some(id) = t.fx else { continue };
            let at = Self::thruster_at(host, t);
            if let Some(v) = host.vfx() {
                v.attach(id, &at);
            }
        }
    }

    pub fn frame(&mut self, host: &mut SceneryHost, _alpha: f64) {
        self.motion.frame(host);
    }

    pub fn end(&mut self, _host: &mut SceneryHost) {}
}
