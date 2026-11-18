//! The match's warm-up (the port's own, a frame-cost rule): after the load, before the first frame, the shaders, state
//! objects, input layouts and assets of every mesh and material set the match may draw are made, so that no actor's
//! first spawn makes them in play. The views list what they may draw (`mvs_view::ViewPoser::warm_meshes`), the effects
//! their component meshes and particle systems (`Fx::warm`); `Renderer::census` is the check's reading of what has been
//! made.
//!
//! The GPU half runs on a loader thread ([`WarmJob`]): the device creates on any thread into the caches every
//! handle shares, and the job's own deferred context takes what a warm-up sets. The frame thread then takes in the
//! particle and decal materials it made ([`Renderer::install_warm`]), a few lookups.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use mvs_sim::World;
use mvs_vfx::draw::EmitterKind;
use mvs_view::{ViewPoser, WarmMesh};

use crate::Result;
use crate::assets::{Assets, Loaded};
use crate::decal::DecalsMade;
use crate::draw::{DrawItem, MeshSlice, PassMask, VfInput, local_layout, set_streams};
use crate::fx::{Fx, FxWarm};
use crate::gpu::{Gpu, Recorded};
use crate::material::{
    LightMapScene, LightMapSurface, MaterialId, MaterialInstance, Mobility, PassKind, VertexFactory, light_map_policy,
};
use crate::particles::{Particles, ParticlesMade};
use crate::renderer::Renderer;
use crate::view::SceneBuffers;

/// What the renderer has made so far (the warm-up's check: nothing of it grows while a warmed match plays).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Census {
    /// The device's shaders, state objects and input layouts.
    pub gpu: [usize; 3],
    /// The meshes, textures and materials loaded, and the shader codes.
    pub assets: Loaded,
    /// The particle materials and the mesh emitters' streams.
    pub particles: [usize; 2],
}

impl Census {
    /// What `later` holds that this one does not, a line per kind (empty: nothing grew).
    pub fn grown(&self, later: &Census) -> Vec<String> {
        let mut out = Vec::new();
        for (k, name) in ["shaders", "states", "layouts"].iter().enumerate() {
            if later.gpu[k] != self.gpu[k] {
                out.push(format!("{name} {} -> {}", self.gpu[k], later.gpu[k]));
            }
        }
        let new = |a: &[String], b: &[String]| -> Vec<String> {
            let had: BTreeSet<&String> = a.iter().collect();
            b.iter().filter(|p| !had.contains(p)).cloned().collect()
        };
        for (name, a, b) in [
            ("meshes", &self.assets.meshes, &later.assets.meshes),
            ("textures", &self.assets.textures, &later.assets.textures),
            ("materials", &self.assets.materials, &later.assets.materials),
        ] {
            let n = new(a, b);
            if !n.is_empty() {
                out.push(format!("{name} +{}: {}", n.len(), n.join(", ")));
            }
        }
        if later.assets.codes != self.assets.codes {
            out.push(format!("shader codes {} -> {}", self.assets.codes, later.assets.codes));
        }
        for (k, name) in ["particle materials", "particle mesh streams"].iter().enumerate() {
            if later.particles[k] != self.particles[k] {
                out.push(format!("{name} {} -> {}", self.particles[k], later.particles[k]));
            }
        }
        out
    }
}

/// What a match may draw: the views' meshes and material sets, the effects' component meshes, the particle systems
/// and floor decals its owners hold. Listed on any thread.
#[derive(Clone, Debug, Default)]
pub struct WarmLists {
    pub meshes: Vec<WarmMesh>,
    pub emitters: Vec<(Arc<str>, EmitterKind)>,
    pub decals: Vec<String>,
}

impl WarmLists {
    /// The lists of a match's World, views and effects (`ViewPoser::warm_meshes`, `Fx::warm`).
    pub fn of(w: &World, poser: &ViewPoser, fx: &mut Fx) -> WarmLists {
        let mut meshes = poser.warm_meshes(w);
        let FxWarm { meshes: own, emitters, decals } = fx.warm(w);
        meshes.extend(own);
        WarmLists { meshes, emitters, decals }
    }
}

/// The warm-up's GPU half for a loader thread (`Renderer::warm_job`, then [`WarmJob::run`] anywhere).
#[derive(Debug)]
pub struct WarmJob {
    gpu: Gpu,
    assets: Arc<Assets>,
    /// The light-map choice's scene (None: read from the stage's own entry, a stage not set yet).
    scene: Option<LightMapScene>,
    stage: String,
    /// The set stage's scenery blocks' own skeletal meshes (none for a stage not set yet: the install warms them).
    own: Vec<WarmMesh>,
    decal_names: Option<Arc<HashMap<String, String>>>,
}

/// What a [`WarmJob`] made that the renderer's own passes keep (`Renderer::install_warm`).
#[derive(Debug)]
pub struct WarmMade {
    /// The copies and writes its context recorded (a mesh emitter's streams), run at the install.
    recorded: Recorded,
    particles: ParticlesMade,
    decals: DecalsMade,
    /// The stage's scenery's own meshes were warmed (its stage was set when the job was made).
    own: bool,
    /// The job's time, ms.
    pub ms: f64,
}

impl WarmJob {
    /// The warm-up of `lists` on this thread with a deferred context of its own: the meshes, textures, materials,
    /// shaders, states and input layouts made into the shared caches, the particle and decal materials kept for the
    /// install. A mesh or material that fails is skipped, its warning left to the frame thread's look-ups.
    pub fn run(self, lists: &WarmLists) -> Result<WarmMade> {
        let t0 = std::time::Instant::now();
        let gpu = self.gpu.deferred()?;
        let assets = &*self.assets;
        let scene = match self.scene {
            Some(s) => s,
            None => crate::stage::stage_lightmap_scene(assets.store(), &self.stage)?,
        };
        let buffers = SceneBuffers::new(&gpu)?;
        let mut insts = HashMap::new();
        for wm in lists.meshes.iter().chain(&self.own) {
            let _ = warm_mesh(&gpu, assets, &buffers, &scene, wm, &mut insts);
        }
        drop(insts);
        let mut particles = Particles::new(&gpu, assets)?;
        let _ = particles.warm(&gpu, assets, &lists.emitters);
        let decals = crate::decal::warm_off_thread(&gpu, assets, self.decal_names, &lists.decals);
        let recorded = gpu.recorded()?;
        Ok(WarmMade {
            recorded,
            particles: particles.into_made(),
            decals,
            own: self.scene.is_some(),
            ms: t0.elapsed().as_secs_f64() * 1e3,
        })
    }
}

impl WarmJob {
    /// A view's warm-up on this thread (a menu's preview fighter, made on the loader): `meshes` as `run` warms
    /// them and `textures` loaded (its materials' overrides, which the draw reads). The commands it recorded are the
    /// frame thread's to run (`Gpu::run_recorded`) before the view draws.
    pub fn run_meshes(self, meshes: &[WarmMesh], textures: &[Arc<str>]) -> Result<Recorded> {
        let gpu = self.gpu.deferred()?;
        let assets = &*self.assets;
        let scene = match self.scene {
            Some(s) => s,
            None => crate::stage::stage_lightmap_scene(assets.store(), &self.stage)?,
        };
        let buffers = SceneBuffers::new(&gpu)?;
        let mut insts = HashMap::new();
        for wm in meshes {
            let _ = warm_mesh(&gpu, assets, &buffers, &scene, wm, &mut insts);
        }
        drop(insts);
        for t in textures {
            let _ = assets.texture(t);
        }
        gpu.recorded()
    }
}

impl Renderer {
    /// The match's warm-up, after its load (`set_stage`, `scenery_begin`): the views' meshes and material sets,
    /// the effects' component meshes, floor decals and the particle systems the match's owners hold (`Fx::warm`).
    /// Returns how many meshes, emitters and decal materials it took. On this thread: [`WarmJob`] is the same off it.
    pub fn prewarm_match(&mut self, w: &World, poser: &ViewPoser, fx: &mut Fx) -> Result<[usize; 3]> {
        let mut lists = WarmLists::of(w, poser, fx);
        // (the stage blocks' own skeletal meshes, hidden at the start)
        if let Some(s) = &self.scenery {
            lists.meshes.extend(s.own_warm());
        }
        self.prewarm(&lists.meshes)?;
        self.prewarm_particles(&lists.emitters);
        for line in self.warm_decals(&lists.decals) {
            eprintln!("warning: warm-up: {line}");
        }
        Ok([lists.meshes.len(), lists.emitters.len(), lists.decals.len()])
    }

    /// The warm-up's GPU half for a loader thread. `stage`: the stage key of a match whose stage is not set yet
    /// (its light-map scene is read from its entry; its scenery's own meshes are the install's); None: the stage set
    /// now goes on.
    pub fn warm_job(&self, stage: Option<&str>) -> WarmJob {
        let own = match (stage, &self.scenery) {
            (None, Some(s)) => s.own_warm(),
            _ => Vec::new(),
        };
        WarmJob {
            gpu: self.gpu.share(),
            assets: self.assets.clone(),
            scene: stage.is_none().then_some(self.lightmap_scene),
            stage: stage.unwrap_or_default().to_owned(),
            own,
            decal_names: self.decal_names(),
        }
    }

    /// A [`WarmJob`]'s work taken in, after `set_stage` and `scenery_begin`: the particle and decal materials it
    /// made; then the frame thread's warm-up of the particles, decals and (a job made before its stage was set) the
    /// scenery's own meshes, which finds what the job made and binds this thread's input layouts and states.
    pub fn install_warm(&mut self, lists: &WarmLists, made: WarmMade) -> Result<()> {
        self.gpu.run_recorded(&made.recorded);
        self.adopt_warm(made.particles, made.decals);
        if !made.own
            && let Some(s) = &self.scenery
        {
            let own = s.own_warm();
            self.prewarm(&own)?;
        }
        self.prewarm_particles(&lists.emitters);
        for line in self.warm_decals(&lists.decals) {
            eprintln!("warning: warm-up: {line}");
        }
        Ok(())
    }

    /// The particle pass's warm-up: each emitter's material (its passes and the vertex factory's input layout) and
    /// a mesh emitter's streams. One that fails is skipped with a warning.
    pub fn prewarm_particles(&mut self, emitters: &[(Arc<str>, EmitterKind)]) {
        for line in self.warm_particles(emitters).into_iter().collect::<BTreeSet<_>>() {
            eprintln!("warning: warm-up: {line}");
        }
    }

    /// What the renderer has made so far.
    pub fn census(&self) -> Census {
        Census { gpu: self.gpu().cache_counts(), assets: self.assets().loaded(), particles: self.particles_counts() }
    }

    /// The warm-up of `meshes` (after `set_stage`: the light-map policy reads the stage's scene): each mesh loaded,
    /// each material of its LOD 0 sections made with its textures, and the passes the views' draws take bound (the
    /// base or translucent pass, custom depth when it is outlined, the depth-only pass of the priority captures when
    /// it is captured) with their input layouts. The instances made here are dropped: what stays is the device's and
    /// the assets' caches, which the frames' own instances find. A mesh or material that fails is skipped with a
    /// warning (a set the match never draws may hold one), as is a material whose pass the renderer has no port of.
    pub fn prewarm(&mut self, meshes: &[WarmMesh]) -> Result<()> {
        let mut insts: HashMap<Arc<str>, MaterialInstance> = HashMap::new();
        let mut warned = BTreeSet::new();
        for wm in meshes {
            if let Err(e) = warm_mesh(&self.gpu, &self.assets, &self.scene, &self.lightmap_scene, wm, &mut insts)
                && warned.insert(e.to_string())
            {
                eprintln!("warning: warm-up: {e}");
            }
        }
        Ok(())
    }
}

/// One mesh's warm-up (`Renderer::prewarm`) on `gpu`, the frame thread's or a loader's deferred one.
fn warm_mesh(
    gpu: &Gpu,
    assets: &Assets,
    buffers: &SceneBuffers,
    lightmap_scene: &LightMapScene,
    wm: &WarmMesh,
    insts: &mut HashMap<Arc<str>, MaterialInstance>,
) -> Result<()> {
    let mesh = wm.mesh.as_ref().map(|p| assets.mesh(p)).transpose()?;
    let vf = match &mesh {
        // (the morph factory of a mesh with an active target, Velma's police car, is made at its first draw)
        Some(m) if m.bones > 0 => VertexFactory::GpuSkin { extra_influences: m.extra_influences, morph: false },
        _ => VertexFactory::Local,
    };
    // (a material slot per LOD 0 section, or the one slot of an own geometry)
    let sections: Vec<(Option<u16>, usize)> = match &mesh {
        Some(m) => m.lods.first().map_or_else(Vec::new, |l| {
            l.sections
                .iter()
                .enumerate()
                .filter(|(_, s)| s.triangles > 0)
                .map(|(k, s)| (Some(k as u16), usize::from(s.material)))
                .collect()
        }),
        None => vec![(None, 0)],
    };
    for (section, slot) in sections {
        let own = mesh.as_ref().and_then(|m| m.materials.get(slot).cloned());
        let set = wm.materials.get(if wm.every_slot { 0 } else { slot });
        let Some(path) = set.cloned().flatten().or(own).filter(|p| !p.is_empty()) else {
            continue;
        };
        if !insts.contains_key(&path) {
            let inst = MaterialInstance::new(gpu, assets, &path)?;
            insts.insert(path.clone(), inst);
        }
        let inst = insts.get_mut(&path).expect("made above");
        let st = &inst.material.state;
        // (a pass the renderer panics on: material.rs make_pass's unported ThinTranslucent states)
        if st.shading_model == "MSM_ThinTranslucent" {
            continue;
        }
        let translucent = st.blend != "BLEND_Opaque" && st.blend != "BLEND_Masked";
        // (as Renderer::add_mesh_pose chooses them)
        let surface = LightMapSurface {
            lit: st.shading_model != "MSM_Unlit",
            translucent,
            light_cache: !matches!(vf, VertexFactory::GpuSkin { .. }),
            cast_volumetric_translucent_shadow: false,
            texture_light_map: false,
            hq_interaction: true,
            texture_shadow_map: false,
            mobility: Mobility::Movable,
            preview_or_forced_volumetric: false,
            indirect_cache: None,
        };
        let policy = light_map_policy(&surface, lightmap_scene);
        let depth = inst.has_depth_pass(assets, vf);
        let mut passes = vec![if translucent { PassKind::Translucent } else { PassKind::Base }];
        if wm.outlined && depth && (!translucent || st.allow_translucent_custom_depth_writes) {
            passes.push(PassKind::CustomDepth);
        }
        if wm.captured && depth && !translucent {
            passes.push(PassKind::DepthOnly);
        }
        for pass in passes {
            let vs = inst.bind(gpu, assets, vf, pass, policy)?.vs;
            match (&mesh, section) {
                (Some(m), Some(k)) => {
                    let item = DrawItem {
                        mesh: Some(MeshSlice { mesh: m.clone(), lod: 0, section: k }),
                        vf,
                        vf_input: VfInput::None,
                        material: MaterialId(0),
                        policy,
                        primitive: 0,
                        lightmap: None,
                        passes: PassMask::of(&[pass]),
                        custom_stencil: 0,
                        lighting_channels: 1,
                        mirrored: false,
                        sort_key: 0.0,
                        after_meshes: false,
                        instances: 1,
                    };
                    set_streams(gpu, buffers, &item, item.mesh.as_ref().expect("set above"), vs)?;
                }
                _ => {
                    let (elems, key) = local_layout();
                    gpu.set_input_layout(vs, key, &elems)?;
                }
            }
        }
    }
    Ok(())
}
