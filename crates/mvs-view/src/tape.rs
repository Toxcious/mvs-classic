//! The pose tape: per tick, the camera and every view's meshes, bones and materials as the oracle drew them, recorded
//! by the oracle; the check of the posing and the input of a frame no Rust sim can step to.
//!
//! The file (the oracle writes it, little-endian): `MVPOSE01`, a u32 length and the header JSON (`run`,
//! `map`, `size` and the run's own keys); then per frame a u32 tick, a u32 length and the frame JSON (`tick`, `alpha`,
//! `time`, `camera`, `fighters` / `actors` with their meshes: `mesh`, `lod`, `bones` (a count), `shown`,
//! `materials`, `stencil`), a u32 count and that many f32: per mesh in the JSON's order, its `to_world` (12) and
//! its bones (12 each), as `MeshPose` holds them.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;

use crate::frame::{ActorPose, Camera, FighterPose, FrameInput, FrameTime, M4, MaterialRef, MeshPose};
use crate::pose::PRIORITY_CAPTURES;

/// The file's first bytes.
pub const MAGIC: &[u8; 8] = b"MVPOSE01";

/// The tape's header.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TapeHeader {
    /// The run the oracle recorded: the trace match or the shots script.
    pub run: String,
    /// The map's MapData name.
    pub map: String,
    /// The oracle's buffer size, pixels.
    pub size: [u32; 2],
    /// The stage key when the run played another stage than its map's.
    pub stage: Option<String>,
}

impl TapeHeader {
    /// The stage key the tape is drawn on: its `stage`, else its map without the `map_` prefix.
    pub fn stage_key(&self) -> &str {
        self.stage.as_deref().unwrap_or_else(|| self.map.strip_prefix("map_").unwrap_or(&self.map))
    }
}

/// A pose tape opened for reading.
#[derive(Debug)]
pub struct PoseTape {
    pub header: TapeHeader,
    path: String,
    bytes: Vec<u8>,
    /// Per tick: the frame JSON's place and the floats' place and count.
    frames: BTreeMap<u32, FramePlace>,
}

#[derive(Clone, Copy, Debug)]
struct FramePlace {
    json: (usize, usize),
    floats: (usize, usize),
}

#[derive(Deserialize)]
struct HeaderJson {
    run: String,
    map: String,
    size: [u32; 2],
    #[serde(default)]
    stage: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameJson {
    tick: u32,
    alpha: f32,
    time: TimeJson,
    camera: CameraJson,
    fighters: Vec<FighterJson>,
    actors: Vec<ActorJson>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimeJson {
    game: f32,
    real: f32,
    delta: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CameraJson {
    origin: [f64; 3],
    translated_world_to_view: M4,
    view_to_clip: M4,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FighterJson {
    fighter: u32,
    meshes: Vec<MeshJson>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorJson {
    meshes: Vec<MeshJson>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MeshJson {
    mesh: String,
    lod: u8,
    bones: usize,
    shown: Vec<bool>,
    materials: Vec<MaterialJson>,
    stencil: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialJson {
    path: String,
    scalars: Vec<(String, f32)>,
    vectors: Vec<(String, [f32; 4])>,
    textures: Vec<(String, String)>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

impl PoseTape {
    pub fn open(path: &Path) -> crate::Result<PoseTape> {
        Self::read(std::fs::read(path)?, &path.display().to_string())
    }

    /// A tape from its bytes; `what` names it in errors.
    pub fn read(bytes: Vec<u8>, what: &str) -> crate::Result<PoseTape> {
        let bad = |why: String| crate::Error::Format { what: what.to_string(), why };
        if bytes.get(..8) != Some(MAGIC.as_slice()) {
            return Err(bad("not a pose tape (no MVPOSE01)".into()));
        }
        let hlen = u32_at(&bytes, 8).ok_or_else(|| bad("cut in the header".into()))? as usize;
        let hjson = bytes.get(12..12 + hlen).ok_or_else(|| bad("cut in the header".into()))?;
        let h: HeaderJson = serde_json::from_slice(hjson).map_err(|e| bad(format!("header: {e}")))?;
        let header = TapeHeader { run: h.run, map: h.map, size: h.size, stage: h.stage };
        let mut frames = BTreeMap::new();
        let mut at = 12 + hlen;
        while at < bytes.len() {
            let start = at;
            let cut = || bad(format!("a frame cut at byte {start}"));
            let tick = u32_at(&bytes, at).ok_or_else(cut)?;
            let jlen = u32_at(&bytes, at + 4).ok_or_else(cut)? as usize;
            let json = (at + 8, jlen);
            let fat = at + 8 + jlen;
            let n = u32_at(&bytes, fat).ok_or_else(cut)? as usize;
            let floats = (fat + 4, n);
            at = fat + 4 + n * 4;
            if at > bytes.len() {
                return Err(cut());
            }
            if frames.insert(tick, FramePlace { json, floats }).is_some() {
                return Err(bad(format!("two frames of tick {tick}")));
            }
        }
        if frames.is_empty() {
            return Err(bad("no frame".into()));
        }
        Ok(PoseTape { header, path: what.to_string(), bytes, frames })
    }

    /// The ticks the tape holds, first to last.
    pub fn ticks(&self) -> std::ops::RangeInclusive<u32> {
        // (a tape holds a frame at least: read refuses one with none)
        let first = self.frames.first_key_value().map_or(0, |(&t, _)| t);
        let last = self.frames.last_key_value().map_or(0, |(&t, _)| t);
        first..=last
    }

    /// The frame recorded at `tick`; None when the tape has no frame there.
    pub fn frame(&self, tick: u32) -> crate::Result<Option<TapeFrame>> {
        let Some(p) = self.frames.get(&tick) else { return Ok(None) };
        let bad = |why: String| crate::Error::Format { what: format!("{} tick {tick}", self.path), why };
        let f: FrameJson =
            serde_json::from_slice(&self.bytes[p.json.0..p.json.0 + p.json.1]).map_err(|e| bad(e.to_string()))?;
        if f.tick != tick {
            return Err(bad(format!("the frame says tick {}", f.tick)));
        }
        let fb = &self.bytes[p.floats.0..p.floats.0 + p.floats.1 * 4];
        let mut floats = fb.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c));
        let mut names: HashMap<String, Arc<str>> = HashMap::new();
        let mut name = |s: String| -> Arc<str> { names.entry(s).or_insert_with_key(|k| Arc::from(k.as_str())).clone() };
        let mut take = |n: usize| -> Option<Vec<[f32; 12]>> {
            (0..n)
                .map(|_| {
                    let mut m = [0f32; 12];
                    for v in &mut m {
                        *v = floats.next()?;
                    }
                    Some(m)
                })
                .collect()
        };
        let mut mesh = |m: MeshJson| -> Option<MeshPose> {
            let to_world = take(1)?[0];
            let bones = take(m.bones)?;
            let materials = m
                .materials
                .into_iter()
                .map(|t| MaterialRef {
                    path: name(t.path),
                    scalars: t.scalars.into_iter().map(|(k, v)| (name(k), v)).collect(),
                    vectors: t.vectors.into_iter().map(|(k, v)| (name(k), v)).collect(),
                    textures: t.textures.into_iter().map(|(k, v)| (name(k), name(v))).collect(),
                })
                .collect();
            Some(MeshPose {
                mesh: name(m.mesh),
                lod: m.lod,
                to_world,
                bones,
                shown: m.shown,
                materials,
                custom_stencil: m.stencil,
                capture_skip: false,
                no_decals: false,
                morphs: Vec::new(),
            })
        };
        let short = || bad(format!("{} floats are fewer than the meshes need", p.floats.1));
        let mut fighters = Vec::with_capacity(f.fighters.len());
        for v in f.fighters {
            let meshes = v.meshes.into_iter().map(&mut mesh).collect::<Option<Vec<_>>>().ok_or_else(short)?;
            // (the tape records no stack: the index read back from the capture a material reads, 0 when none does)
            let priority = meshes
                .iter()
                .flat_map(|m| &m.materials)
                .flat_map(|m| &m.textures)
                .find_map(|(_, t)| PRIORITY_CAPTURES.iter().position(|p| **p == **t))
                .map_or(0, |k| k as u8 + 1);
            fighters.push(FighterPose {
                fighter: v.fighter,
                meshes,
                priority: Some(priority),
                ..FighterPose::default()
            });
        }
        let mut actors = Vec::with_capacity(f.actors.len());
        for v in f.actors {
            let meshes = v.meshes.into_iter().map(&mut mesh).collect::<Option<Vec<_>>>().ok_or_else(short)?;
            actors.push(ActorPose { meshes, ..ActorPose::default() });
        }
        drop(mesh);
        if floats_left(fb, &fighters, &actors) {
            return Err(bad(format!("{} floats are more than the meshes need", p.floats.1)));
        }
        Ok(Some(TapeFrame {
            tick,
            alpha: f.alpha,
            time: FrameTime { game: f.time.game, real: f.time.real, delta: f.time.delta },
            camera: Camera {
                origin: f.camera.origin,
                translated_world_to_view: f.camera.translated_world_to_view,
                view_to_clip: f.camera.view_to_clip,
            },
            fighters,
            actors,
        }))
    }
}

/// Whether the frame's float block is longer than its meshes need.
fn floats_left(block: &[u8], fighters: &[FighterPose], actors: &[ActorPose]) -> bool {
    let meshes = fighters.iter().flat_map(|f| &f.meshes).chain(actors.iter().flat_map(|a| &a.meshes));
    let need: usize = meshes.map(|m| 12 * (1 + m.bones.len())).sum();
    block.len() / 4 != need
}

/// One recorded frame.
#[derive(Clone, Debug, PartialEq)]
pub struct TapeFrame {
    pub tick: u32,
    pub alpha: f32,
    pub time: FrameTime,
    pub camera: Camera,
    pub fighters: Vec<FighterPose>,
    pub actors: Vec<ActorPose>,
}

impl TapeFrame {
    /// The frame as the renderer takes it. The tape holds no sim events.
    pub fn input(&self) -> FrameInput<'_> {
        FrameInput {
            tick: self.tick,
            alpha: self.alpha,
            time: self.time,
            camera: &self.camera,
            fighters: &self.fighters,
            actors: &self.actors,
            events: &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mesh_json(m: &MeshPose) -> serde_json::Value {
        json!({
            "mesh": &*m.mesh, "lod": m.lod, "bones": m.bones.len(), "shown": m.shown, "stencil": m.custom_stencil,
            "materials": m.materials.iter().map(|t| json!({
                "path": &*t.path,
                "scalars": t.scalars.iter().map(|(k, v)| json!([&**k, v])).collect::<Vec<_>>(),
                "vectors": t.vectors.iter().map(|(k, v)| json!([&**k, v])).collect::<Vec<_>>(),
                "textures": t.textures.iter().map(|(k, v)| json!([&**k, &**v])).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    /// The tape's bytes as the oracle writes them.
    fn write(header: &TapeHeader, frames: &[TapeFrame]) -> Vec<u8> {
        let mut b = MAGIC.to_vec();
        let h =
            json!({ "run": header.run, "map": header.map, "size": header.size, "stage": header.stage, "worldSeed": 1 })
                .to_string();
        b.extend((h.len() as u32).to_le_bytes());
        b.extend(h.as_bytes());
        for f in frames {
            let meshes = f.fighters.iter().flat_map(|v| &v.meshes).chain(f.actors.iter().flat_map(|a| &a.meshes));
            let floats: Vec<f32> =
                meshes.flat_map(|m| m.to_world.iter().chain(m.bones.iter().flatten()).copied()).collect();
            let j = json!({
                "tick": f.tick, "alpha": f.alpha,
                "time": { "game": f.time.game, "real": f.time.real, "delta": f.time.delta },
                "camera": { "origin": f.camera.origin, "translatedWorldToView": f.camera.translated_world_to_view,
                    "viewToClip": f.camera.view_to_clip },
                "fighters": f.fighters.iter().map(|v| json!({ "fighter": v.fighter,
                    "meshes": v.meshes.iter().map(mesh_json).collect::<Vec<_>>() })).collect::<Vec<_>>(),
                "actors": f.actors.iter().map(|a| json!({ "meshes": a.meshes.iter().map(mesh_json).collect::<Vec<_>>() }))
                    .collect::<Vec<_>>(),
            })
            .to_string();
            b.extend(f.tick.to_le_bytes());
            b.extend((j.len() as u32).to_le_bytes());
            b.extend(j.as_bytes());
            b.extend((floats.len() as u32).to_le_bytes());
            for v in floats {
                b.extend(v.to_le_bytes());
            }
        }
        b
    }

    fn sample(tick: u32) -> TapeFrame {
        let m = |k: f32| {
            let mut t = [0f32; 12];
            t.iter_mut().enumerate().for_each(|(i, v)| *v = k + i as f32 * 0.25);
            t
        };
        let body = MeshPose {
            mesh: "/Game/A/SK_A.SK_A".into(),
            lod: 0,
            to_world: m(1.0),
            bones: vec![m(2.0), m(3.5)],
            shown: vec![true, false],
            materials: vec![MaterialRef {
                path: "/Game/A/MI_A.MI_A".into(),
                scalars: vec![("Flash".into(), 0.5)],
                vectors: vec![("Tint".into(), [1.0, 0.5, 0.25, 1.0])],
                textures: vec![("Depth".into(), "<runtime>".into())],
            }],
            custom_stencil: 2,
            capture_skip: false,
            no_decals: false,
            morphs: Vec::new(),
        };
        let prop =
            MeshPose { mesh: "/Game/A/SM_B.SM_B".into(), to_world: m(-7.0), shown: vec![true], ..Default::default() };
        TapeFrame {
            tick,
            alpha: 0.0,
            time: FrameTime { game: tick as f32 / 60.0, real: 1.0, delta: 1.0 / 60.0 },
            camera: Camera {
                origin: [1.0, -2.0, 300.5],
                translated_world_to_view: [[0.0, 0.0, 1.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0; 4]],
                view_to_clip: [[4.3, 0.0, 0.0, 0.0], [0.0, 7.7, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 10.0, 0.0]],
            },
            fighters: vec![FighterPose {
                fighter: 1,
                meshes: vec![body, prop.clone()],
                priority: Some(0),
                ..FighterPose::default()
            }],
            actors: vec![ActorPose { meshes: vec![prop], ..ActorPose::default() }],
        }
    }

    #[test]
    fn round_trip() {
        let header = TapeHeader {
            run: "lab-picture".into(),
            map: "map_training".into(),
            size: [1920, 1080],
            stage: Some("training".into()),
        };
        let frames = [sample(4), sample(5), sample(7)];
        let tape = PoseTape::read(write(&header, &frames), "test").unwrap();
        assert_eq!(tape.header, header);
        assert_eq!(tape.ticks(), 4..=7);
        for f in &frames {
            assert_eq!(tape.frame(f.tick).unwrap().as_ref(), Some(f));
        }
        assert_eq!(tape.frame(6).unwrap(), None);
        let mut cut = write(&header, &frames);
        cut.truncate(cut.len() - 3);
        assert!(PoseTape::read(cut, "test").is_err());
    }

    /// A tape the oracle recorded (POSE_TAPE names it): every frame reads, its floats are finite and each skinned
    /// mesh's bone count is the same on every frame.
    #[test]
    fn reads_a_recorded_tape() {
        let Some(path) = std::env::var_os("POSE_TAPE") else {
            eprintln!("POSE_TAPE names no tape: nothing to read");
            return;
        };
        let tape = PoseTape::open(Path::new(&path)).unwrap();
        let mut bones: HashMap<Arc<str>, usize> = HashMap::new();
        let (mut frames, mut meshes) = (0, 0);
        for tick in tape.ticks() {
            let Some(f) = tape.frame(tick).unwrap() else { continue };
            frames += 1;
            for m in f.fighters.iter().flat_map(|v| &v.meshes).chain(f.actors.iter().flat_map(|a| &a.meshes)) {
                meshes += 1;
                assert!(m.to_world.iter().chain(m.bones.iter().flatten()).all(|v| v.is_finite()), "{}", m.mesh);
                assert_eq!(*bones.entry(m.mesh.clone()).or_insert(m.bones.len()), m.bones.len(), "{}", m.mesh);
            }
        }
        eprintln!("{}: {} ({}), {frames} frames, {meshes} meshes", path.display(), tape.header.run, tape.header.map);
        assert!(frames > 0);
    }
}
