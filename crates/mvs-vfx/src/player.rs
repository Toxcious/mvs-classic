//! The player: spawns systems, ticks them and hands out their
//! emitters' draws. Places are UE world (cm).

use indexmap::IndexMap;
use std::fmt;
use std::sync::Arc;

use mvs_store::{Pack, Store};

use crate::data::{ParticlesIndex, mesh_extent, parse_index, parse_system};
use crate::distribution::Params;
use crate::draw::{DrawParams, EmitterDraw, EmitterKind};
use crate::emitter::EmitterInstance;
use crate::fill::{
    Blend, MESH_STRIDE, RIBBON_STRIDE, SPRITE_STRIDE, SpriteSort, View, draw_params, fill_beam, fill_meshes,
    fill_ribbon, fill_sprites, view_z,
};
use crate::math::{Rows, V3};
use crate::random::{RandomSource, RandomStream};
use crate::system::{Box3, CollisionView, Surface, SystemInstance, SystemTemplate};
use crate::template::{EmitterTemplate, Kind, UNPORTED_MODULES};

/// A draw's material scalars (EmitterDraw::material_scalars).
type Scalars = Vec<(Arc<str>, f32)>;

/// A spawned system. A handle to a system that has ended answers as dead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SystemId {
    pub index: u32,
    pub generation: u32,
}

/// An attached component's world transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attachment {
    pub loc: [f64; 3],
    /// The rotation's rows.
    pub rot: [[f64; 3]; 3],
    /// World Scale3D; the spawn scale stays when None.
    pub scale: Option<[f64; 3]>,
}

fn rows(a: &Attachment) -> Rows {
    let r = &a.rot;
    [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2]]
}

/// The particles index entry's name below the kind's versioned folder.
const INDEX_NAME: &str = "/Index.Owners";

/// A loaded system's template and, per emitter, its mesh section.
/// A system's first unported module (`Player::unported`), as the store's cache keeps it.
struct Unported(Option<String>);

#[derive(Clone)]
struct Loaded {
    t: Arc<SystemTemplate>,
    sections: Vec<Option<u32>>,
}

struct Slot {
    generation: u32,
    sys: Option<SystemInstance>,
    sections: Vec<Option<u32>>,
}

/// Every live system of a match.
pub struct Player {
    /// The store whose cache holds the parsed templates every player shares.
    store: Store,
    particles: std::sync::Arc<Pack>,
    meshes: std::sync::Arc<Pack>,
    /// The particles kind's versioned folder (the index entry's).
    dir: String,
    index: ParticlesIndex,
    /// The kind's systems outside the index by their package's short name: the build object path (a block of the
    /// scenery names its systems by short name; VfxPlayer.addSystems registers them from the entry's `systems`).
    by_short: IndexMap<String, String>,
    /// The material kind: an emitter whose material it holds is kept.
    materials: std::sync::Arc<Pack>,
    templates: IndexMap<String, Option<Loaded>>,
    rng: Box<dyn RandomSource>,
    slots: Vec<Slot>,
    free: Vec<u32>,
    /// The live systems' slots in spawn order (the tick's and the draw's order).
    live: Vec<u32>,
    game_time: f64,
    /// The camera the last draws were made for (OrientZAxisTowardCamera reads it at the next tick).
    view: Option<View>,
    /// The camera's up axis (CameraToWorld's Y axis, View +0x390; UE world) the beams' up falls back to.
    cam_up: V3,
    params: Vec<DrawParams>,
    /// The buffers of the last draws (their vertices and material scalars) and the sprite sort's, kept for the next.
    spare: Vec<(Vec<u8>, Scalars)>,
    sprite_sort: SpriteSort,
    /// What every system's Collision modules meet (VfxPlayer.setSurfaces), whoever spawned the system.
    surfaces: Option<Arc<[Surface]>>,
    /// The Collision modules' view: the eye of the last draws (None before them) and the LOD distance factor.
    collision_loc: Option<V3>,
    lod: f64,
    /// (the port's own Ghosts) The systems spawned now start ghosted (`set_ghost`): their fighter is hidden from this
    /// screen.
    pub spawn_ghost: bool,
}

impl fmt::Debug for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Player").field("live", &self.live.len()).field("game_time", &self.game_time).finish()
    }
}

impl Player {
    /// A player reading its systems from `store`'s particles kind; `seed` starts its random stream.
    pub fn new(store: &Store, seed: u32) -> crate::Result<Player> {
        let particles = store.kind("particles")?;
        let meshes = store.kind("mesh")?;
        let materials = store.kind("material")?;
        let index_path = particles
            .paths()
            .find(|p| p.ends_with(INDEX_NAME))
            .ok_or_else(|| crate::Error::Format { what: "particles".into(), why: format!("no {INDEX_NAME} entry") })?
            .to_string();
        let index = parse_index(&particles.bytes(&index_path)?)?;
        let dir = index_path[..index_path.len() - INDEX_NAME.len()].to_string();
        let mut by_short = IndexMap::new();
        for p in particles.paths() {
            let Some(object) = p.strip_prefix(dir.as_str()).filter(|o| *o != INDEX_NAME && o.starts_with('/')) else {
                continue;
            };
            let package = object.split('.').next().unwrap_or(object);
            by_short
                .entry(package[package.rfind('/').map_or(0, |k| k + 1)..].to_string())
                .or_insert_with(|| object.to_string());
        }
        Ok(Player {
            store: store.clone(),
            particles,
            meshes,
            dir,
            index,
            by_short,
            materials,
            templates: IndexMap::new(),
            rng: Box::new(RandomStream::new(seed)),
            slots: Vec::new(),
            free: Vec::new(),
            live: Vec::new(),
            game_time: 0.0,
            view: None,
            cam_up: [0.0, 0.0, 1.0],
            params: Vec::new(),
            spare: Vec::new(),
            sprite_sort: SpriteSort::default(),
            surfaces: None,
            collision_loc: None,
            lod: 1.0,
            spawn_ghost: false,
        })
    }

    /// The index's systems by short name, with the parts that load each.
    pub fn systems(&self) -> impl Iterator<Item = (&str, &[String])> + '_ {
        self.index.systems.iter().map(|(k, r)| (k.as_str(), r.owners.as_slice()))
    }

    /// The warm-up (the port's own, a frame-cost rule): the templates of every
    /// indexed system one of `owners` owns (the index's owners, compared in any case) loaded, a system with an
    /// unported module left out, and each kept emitter's material (its DrawRef) and draw kind, once a pair, for the
    /// renderer's particle warm-up.
    pub fn warm(&mut self, owners: &[&str]) -> Vec<(Arc<str>, EmitterKind)> {
        let names: Vec<String> = self
            .index
            .systems
            .iter()
            .filter(|(_, r)| r.owners.iter().any(|o| owners.iter().any(|w| w.eq_ignore_ascii_case(o))))
            .map(|(n, _)| n.clone())
            .collect();
        let mut out: IndexMap<(Arc<str>, String), EmitterKind> = IndexMap::new();
        for n in names {
            if self.unported(&n).is_some() {
                continue;
            }
            let Some((t, _)) = self.template(&n) else { continue };
            for e in &t.emitters {
                let kind = match (e.kind, &e.mesh) {
                    (Kind::Sprite, _) => EmitterKind::Sprite,
                    (Kind::Mesh, Some(mesh)) => EmitterKind::Mesh { mesh: mesh.clone() },
                    (Kind::Mesh, None) => continue,
                    (Kind::Ribbon, _) => EmitterKind::Ribbon,
                    (Kind::Beam, _) => EmitterKind::Beam,
                };
                out.entry((e.material.clone(), format!("{kind:?}"))).or_insert(kind);
            }
        }
        out.into_iter().map(|((m, _), k)| (m, k)).collect()
    }

    /// The random source every draw of the particles takes in turn (a replay hands back recorded draws).
    pub fn set_random(&mut self, rng: Box<dyn RandomSource>) {
        self.rng = rng;
    }

    /// The system's template by its short name (the index's key, as the game spawns it) or its object path; None
    /// when the index has no such system. An emitter whose material the material kind lacks is left out.
    fn template(&mut self, system: &str) -> Option<(Arc<SystemTemplate>, Vec<Option<u32>>)> {
        if !self.templates.contains_key(system) {
            // (parsed once per store: a match's player and a loader's warm-up find the same templates; weighed by
            // the entry's size, so a trim lets go what no load asked for since)
            let size = self.entry_size(system);
            let shared = self.store.cached(system, || Ok::<_, std::convert::Infallible>(self.load(system)), |_| size);
            let loaded = match shared {
                Ok(l) => (*l).clone(),
                Err(never) => match never {},
            };
            self.templates.insert(system.to_string(), loaded);
        }
        self.templates.get(system).and_then(|l| l.as_ref()).map(|l| (l.t.clone(), l.sections.clone()))
    }

    /// A system by its short name or build object path from the index; else, by its build object path, a system the
    /// kind holds outside the index (a level's: VfxPlayer.addSystems of the scenery's `systems`), or by its short name, named by its
    /// package's short name.
    fn locate<'a>(&'a self, system: &'a str) -> Option<(&'a str, String)> {
        match self
            .index
            .systems
            .get_key_value(system)
            .or_else(|| self.index.systems.iter().find(|(_, r)| r.path == system))
        {
            Some((name, r)) => Some((name.as_str(), format!("{}{}", self.dir, r.path))),
            None => {
                let object = if system.starts_with('/') { system } else { self.by_short.get(system)? };
                let path = format!("{}{object}", self.dir);
                if !self.particles.has(&path) {
                    return None;
                }
                let system = object;
                let package = system.split('.').next().unwrap_or(system);
                Some((&package[package.rfind('/').map_or(0, |k| k + 1)..], path))
            }
        }
    }

    /// How many of the system's emitters the player keeps (those whose material the material kind holds); None
    /// when the index has no such system.
    pub fn emitter_count(&mut self, system: &str) -> Option<usize> {
        self.template(system).map(|(t, _)| t.emitters.len())
    }

    /// The first module type of `system` that has no port (template.rs UNPORTED_MODULES): building its template would
    /// panic. None when everything is ported or the system is not found.
    pub fn unported(&self, system: &str) -> Option<String> {
        // (read once per store)
        let size = self.entry_size(system);
        match self.store.cached(
            system,
            || Ok::<_, std::convert::Infallible>(Unported(self.read_unported(system))),
            |_| size,
        ) {
            Ok(u) => u.0.clone(),
            Err(never) => match never {},
        }
    }

    /// The size of the system's entry in bytes (1 when it is not found): what its cached values weigh.
    fn entry_size(&self, system: &str) -> usize {
        self.locate(system).and_then(|(_, path)| self.particles.record(&path)).map_or(1, |r| (r.size as usize).max(1))
    }

    /// `unported` read from the system's entry.
    fn read_unported(&self, system: &str) -> Option<String> {
        let (_, path) = self.locate(system)?;
        let bytes = self.particles.bytes(&path).unwrap_or_else(|e| panic!("particles {path}: {e}"));
        let entry = parse_system(&path, &bytes).unwrap_or_else(|e| panic!("{e}"));
        let type_of = |m: &serde_json::Value| m.get("type").and_then(|t| t.as_str()).map(str::to_string);
        for e in &entry.system.emitters {
            if let Some(t) = e.modules.iter().filter_map(type_of).find(|t| UNPORTED_MODULES.contains(&t.as_str())) {
                return Some(t);
            }
        }
        None
    }

    fn load(&self, system: &str) -> Option<Loaded> {
        let (name, path) = self.locate(system)?;
        let bytes = self.particles.bytes(&path).unwrap_or_else(|e| panic!("particles {path}: {e}"));
        let entry = parse_system(&path, &bytes).unwrap_or_else(|e| panic!("{e}"));
        let s = entry.system;
        let mut emitters = Vec::new();
        let mut sections = Vec::new();
        for e in &s.emitters {
            let req = e.modules.iter().find(|m| m.get("type").and_then(|t| t.as_str()) == Some("Required"));
            let material = req.and_then(|m| m.get("Material")).and_then(|m| m.as_str()).unwrap_or("");
            // (a DrawRef is the material's object path and `?ctx=<context>`; the material kind is keyed by the path)
            if !self.materials.has(material.split_once('?').map_or(material, |(p, _)| p)) {
                continue;
            }
            let mut section = None;
            let mesh = e.mesh.as_ref().map(|key| {
                // (a key the index lacks is "<mesh path>" or "<mesh path>#<section>")
                let (mesh_path, sec) = match self.index.meshes.get(key) {
                    Some(m) => (m.path.as_str(), m.section),
                    None => match key.split_once('#') {
                        Some((path, sec)) => {
                            (path, Some(sec.parse().unwrap_or_else(|_| panic!("particles {name}: mesh key {key}"))))
                        }
                        None => (key.as_str(), None),
                    },
                };
                section = sec;
                let bytes = self.meshes.bytes(mesh_path).unwrap_or_else(|err| panic!("mesh {mesh_path}: {err}"));
                let extent = mesh_extent(mesh_path, &bytes).unwrap_or_else(|err| panic!("{err}"));
                (Arc::<str>::from(mesh_path), extent)
            });
            emitters.push(Arc::new(EmitterTemplate::new(e, mesh)));
            sections.push(section);
        }
        let fixed_box = s.fixed_box.as_ref().map(|f| Box3 { min: [f[0], f[1], f[2]], max: [f[3], f[4], f[5]] });
        let t = SystemTemplate {
            name: Arc::from(name),
            emitters,
            fixed_box,
            warmup: s.warmup_time.unwrap_or(0.0),
            warmup_rate: s.warmup_tick_rate.unwrap_or(0.0),
            orient_z: s.orient_z.unwrap_or(false),
        };
        Some(Loaded { t: Arc::new(t), sections })
    }

    fn insert(&mut self, mut s: SystemInstance, sections: Vec<Option<u32>>) -> SystemId {
        s.ghost = self.spawn_ghost;
        let index = match self.free.pop() {
            Some(i) => {
                let slot = &mut self.slots[i as usize];
                slot.sys = Some(s);
                slot.sections = sections;
                i
            }
            None => {
                self.slots.push(Slot { generation: 0, sys: Some(s), sections });
                (self.slots.len() - 1) as u32
            }
        };
        self.live.push(index);
        SystemId { index, generation: self.slots[index as usize].generation }
    }

    fn get(&mut self, id: SystemId) -> Option<&mut SystemInstance> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.sys.as_mut()
    }

    fn params_of(params: &[(&str, f64)]) -> Params {
        params.iter().map(|&(k, v)| (k.to_string(), v)).collect()
    }

    /// UGameplayStatics::SpawnEmitterAtLocation: `system` (its short name or object path) at `loc`, `rotator`
    /// (FRotator pitch, yaw, roll, degrees), `scale`, with its instance parameters (`Name`, or `Name.X` for one axis
    /// of a vector). None when the system has nothing to play. Its first tick is the next frame's (a managed PSC
    /// joins the PrePhysics manager tick after the one that spawned it); `spawn_before_manager` for one spawned
    /// before that tick.
    pub fn spawn(
        &mut self,
        system: &str,
        loc: [f64; 3],
        rotator: [f64; 3],
        scale: [f64; 3],
        params: &[(&str, f64)],
    ) -> Option<SystemId> {
        self.spawn_at(system, loc, rotator, scale, params, false)
    }

    /// `spawn` for a system spawned before the frame's PrePhysics manager tick: it ticks the same frame.
    pub fn spawn_before_manager(
        &mut self,
        system: &str,
        loc: [f64; 3],
        rotator: [f64; 3],
        scale: [f64; 3],
        params: &[(&str, f64)],
    ) -> Option<SystemId> {
        self.spawn_at(system, loc, rotator, scale, params, true)
    }

    fn spawn_at(
        &mut self,
        system: &str,
        loc: V3,
        rotator: V3,
        scale: V3,
        params: &[(&str, f64)],
        before_manager: bool,
    ) -> Option<SystemId> {
        let (t, sections) = self.template(system)?;
        let mut s = SystemInstance::new(t, loc, rotator, 1.0, Self::params_of(params), false, self.rng.as_mut());
        let rot = s.sys.rot;
        s.set_transform(loc, rot, Some(scale));
        s.waiting = !before_manager;
        self.meet(&mut s);
        s.warm_up(self.game_time, self.view.map(|v| v.origin), self.rng.as_mut());
        Some(self.insert(s, sections))
    }

    /// UGameplayStatics::SpawnEmitterAttached: an auto-destroying system that follows `at`; its owner moves it with
    /// `attach` before each `tick`.
    pub fn spawn_attached(
        &mut self,
        system: &str,
        at: &Attachment,
        scale: f64,
        params: &[(&str, f64)],
    ) -> Option<SystemId> {
        let (t, sections) = self.template(system)?;
        let mut s = SystemInstance::new(t, at.loc, [0.0; 3], scale, Self::params_of(params), false, self.rng.as_mut());
        s.set_transform(at.loc, rows(at), at.scale);
        s.follow = Some((at.loc, rows(at), at.scale));
        self.meet(&mut s);
        s.warm_up(self.game_time, self.view.map(|v| v.origin), self.rng.as_mut());
        Some(self.insert(s, sections))
    }

    /// The attached system's new world transform.
    pub fn attach(&mut self, id: SystemId, at: &Attachment) {
        if let Some(s) = self.get(id) {
            s.follow = Some((at.loc, rows(at), at.scale));
        }
    }

    /// DestroyComponent: its particles vanish at once.
    pub fn kill(&mut self, id: SystemId) {
        if let Some(s) = self.get(id) {
            s.kill();
        }
    }

    /// DeactivateSystem: no more spawning; it ends once its particles are gone. A kept component (`attach_component`)
    /// already inactive is left as it is.
    pub fn deactivate(&mut self, id: SystemId) {
        if let Some(s) = self.get(id)
            && (!s.persistent || s.sys.active)
        {
            s.deactivate();
        }
    }

    /// SpawnEmitterAttached for a component its owner keeps and activates and deactivates later: it starts
    /// inactive and the owner moves it with `set_location` before each tick. `rotator` in degrees. None when the
    /// system has nothing to play.
    pub fn attach_component(
        &mut self,
        system: &str,
        loc: [f64; 3],
        rotator: [f64; 3],
        scale: f64,
        params: &[(&str, f64)],
    ) -> Option<SystemId> {
        let (t, sections) = self.template(system)?;
        let mut s = SystemInstance::new(t, loc, rotator, scale, Self::params_of(params), true, self.rng.as_mut());
        s.sys.active = false;
        self.meet(&mut s);
        Some(self.insert(s, sections))
    }

    /// The kept component's new world location (its rotation and scale stay).
    pub fn set_location(&mut self, id: SystemId, loc: [f64; 3]) {
        if let Some(s) = self.get(id) {
            let rot = s.sys.rot;
            s.set_transform(loc, rot, None);
        }
    }

    /// Activate(false) on a kept component (SystemInstance::activate): an inactive one restarts its emitters and
    /// warms up; particles already alive keep going.
    pub fn activate(&mut self, id: SystemId) {
        let (now, view) = (self.game_time, self.view.map(|v| v.origin));
        let (surfaces, collision_view) = (self.surfaces.clone(), self.collision_view());
        let Some(slot) = self.slots.get_mut(id.index as usize) else {
            return;
        };
        if slot.generation != id.generation {
            return;
        }
        if let Some(s) = slot.sys.as_mut() {
            s.sys.surfaces = surfaces;
            s.sys.collision_view = collision_view;
            s.activate(now, view, self.rng.as_mut());
        }
    }

    /// The system is not active (deactivated, or a kept component not yet activated); a dead handle answers true.
    pub fn deactivated(&self, id: SystemId) -> bool {
        self.system(id).is_none_or(|s| !s.sys.active)
    }

    /// The live particle count of the system's emitter `emitter` (0 for a dead handle or no such emitter).
    pub fn particles(&self, id: SystemId, emitter: usize) -> usize {
        self.emitters(id).and_then(|e| e.get(emitter)).map_or(0, |e| e.particle_count())
    }

    /// UParticleSystem::IsImmortal (bIsImmortal, +0xb6 bit 0): the post-load pass 142bf36c0 sets it when an
    /// emitter with LOD 0 enabled has EmitterLoops 0 in its RequiredModule. False for a system the index lacks.
    pub fn immortal(&mut self, system: &str) -> bool {
        self.template(system).is_some_and(|(t, _)| t.emitters.iter().any(|e| e.loops == 0.0))
    }

    /// A level's own system: an Emitter actor's component (`next` false: it ticks from the frame it is made) or
    /// one a level actor spawns (`next`: SpawnEmitterAtLocation, first ticked a frame later as `spawn`'s); a
    /// follower is moved with `attach`. `sort_priority`: the component's TranslucencySortPriority; `on_spawn`:
    /// the owner reads its OnParticleSpawn through `spawned`. `clear` keeps it.
    pub fn spawn_level(
        &mut self,
        system: &str,
        at: &Attachment,
        next: bool,
        params: &[(&str, f64)],
        sort_priority: i32,
        on_spawn: bool,
    ) -> Option<SystemId> {
        let (t, sections) = self.template(system)?;
        let mut s = SystemInstance::new(t, at.loc, [0.0; 3], 1.0, Self::params_of(params), false, self.rng.as_mut());
        s.set_transform(at.loc, rows(at), at.scale);
        s.level = true;
        s.waiting = next;
        s.on_spawn = on_spawn;
        s.sort_priority = sort_priority;
        self.meet(&mut s);
        s.warm_up(self.game_time, self.view.map(|v| v.origin), self.rng.as_mut());
        Some(self.insert(s, sections))
    }

    /// The latest tick's spawn events (event name, location) of a system spawned with `on_spawn`.
    pub fn spawned(&self, id: SystemId) -> &[(Arc<str>, V3)] {
        self.system(id).map_or(&[], |s| s.spawned.as_slice())
    }

    /// The live list's `k`th system leaves: its slot is freed and its handles answer as dead.
    fn remove_live(&mut self, k: usize) {
        let i = self.live.remove(k);
        let slot = &mut self.slots[i as usize];
        slot.sys = None;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(i);
    }

    /// The end of a level: every system goes, the kept components too. `keep_level`: the end of a match whose
    /// level goes on; the level's own systems stay.
    pub fn end_all(&mut self, keep_level: bool) {
        let mut k = self.live.len();
        while k > 0 {
            k -= 1;
            let level = self.slots[self.live[k] as usize].sys.as_ref().is_some_and(|s| s.level);
            if !(keep_level && level) {
                self.remove_live(k);
            }
        }
    }

    /// Drops every one-shot system; kept components stay, inactive and empty, and the level's systems stay.
    pub fn clear(&mut self) {
        let mut k = self.live.len();
        while k > 0 {
            k -= 1;
            let s = self.slots[self.live[k] as usize].sys.as_mut().unwrap();
            if s.level {
                continue;
            }
            if s.persistent {
                s.sys.active = false;
                for e in &mut s.emitters {
                    e.clear_particles();
                }
            } else {
                self.remove_live(k);
            }
        }
    }

    /// The component's CustomTimeDilation.
    pub fn set_time_dilation(&mut self, id: SystemId, dilation: f64) {
        if let Some(s) = self.get(id) {
            s.time_dilation = dilation;
        }
    }

    /// USceneComponent::SetVisibility: a hidden system is not drawn.
    pub fn set_visible(&mut self, id: SystemId, on: bool) {
        if let Some(s) = self.get(id) {
            s.hidden = !on;
        }
    }

    /// (the port's own Ghosts) A ghosted system is not drawn, whatever its visibility.
    pub fn set_ghost(&mut self, id: SystemId, on: bool) {
        if let Some(s) = self.get(id) {
            s.ghost = on;
        }
    }

    /// SetBeamSourcePoint (142bf87c0): the point of beam `index` of the system's emitter `emitter` (its place in the
    /// system), for a BeamSource with SourceMethod UserSet; the array grows with zero points to `index`.
    pub fn set_beam_source_point(&mut self, id: SystemId, emitter: i32, point: [f64; 3], index: i32) {
        if let Some(s) = self.get(id) {
            put_beam_point(&mut s.sys.beam_sources, emitter, point, index);
        }
    }

    /// SetBeamTargetPoint: as set_beam_source_point, for a BeamTarget with TargetMethod UserSet.
    pub fn set_beam_target_point(&mut self, id: SystemId, emitter: i32, point: [f64; 3], index: i32) {
        if let Some(s) = self.get(id) {
            put_beam_point(&mut s.sys.beam_targets, emitter, point, index);
        }
    }

    /// SetActorParameter: the place of the actor named `name` (a beam end with method Actor reads it at each tick;
    /// None for no actor, the end's own point then). The owner sets it again whenever the actor moves.
    // Unverified: one point is kept for the beam's source, so an owner
    // sets it each tick (Wonder Woman's lasso tick does) or the beam reads where the actor was at the last set.
    pub fn set_beam_actor(&mut self, id: SystemId, name: &str, at: Option<[f64; 3]>) {
        if let Some(s) = self.get(id) {
            let a = &mut s.sys.beam_actors;
            match a.iter_mut().find(|(n, _)| n == name) {
                Some(e) => e.1 = at,
                None => a.push((name.to_string(), at)),
            }
        }
    }

    /// SetFloatParameter, or a vector parameter's three axes at once (`Name.X`: one axis).
    pub fn set_param(&mut self, id: SystemId, name: &str, v: f64) {
        if let Some(s) = self.get(id) {
            s.sys.params.insert(name.to_string(), v);
        }
    }

    pub fn alive(&self, id: SystemId) -> bool {
        self.slots
            .get(id.index as usize)
            .is_some_and(|s| s.generation == id.generation && s.sys.as_ref().is_some_and(|s| !s.done()))
    }

    /// The live system's emitter instances (a check's probe).
    pub fn emitters(&self, id: SystemId) -> Option<&[EmitterInstance]> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.sys.as_ref().map(|s| s.emitters.as_slice())
    }

    /// The live system (a check's probe).
    pub fn system(&self, id: SystemId) -> Option<&SystemInstance> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.sys.as_ref()
    }

    /// The player's clock: the sum of the steps ticked (View GameTime for the materials).
    pub fn game_time(&self) -> f64 {
        self.game_time
    }

    /// The live system, to fill its emitters one by one (a check's replay).
    pub fn system_mut(&mut self, id: SystemId) -> Option<&mut SystemInstance> {
        self.get(id)
    }

    /// Advances every system by `dt` seconds of game time; a system that has ended leaves.
    pub fn tick(&mut self, dt: f64) {
        self.game_time += dt;
        let view = self.view.map(|v| v.origin);
        let collision_view = self.collision_view();
        for &i in &self.live {
            let s = self.slots[i as usize].sys.as_mut().unwrap();
            s.sys.surfaces.clone_from(&self.surfaces);
            s.sys.collision_view = collision_view;
            if s.waiting {
                s.waiting = false;
            } else {
                s.sys.world_time = self.game_time;
                s.tick(dt * s.time_dilation, view, self.rng.as_mut());
            }
        }
        let mut k = self.live.len();
        while k > 0 {
            k -= 1;
            if self.slots[self.live[k] as usize].sys.as_ref().unwrap().done() {
                self.remove_live(k);
            }
        }
    }

    /// What every system's Collision modules meet from now on (VfxPlayer.setSurfaces): the shown stage's
    /// horizontal surfaces, or None (no stage: the modules meet nothing).
    pub fn set_surfaces(&mut self, surfaces: Option<Vec<Surface>>) {
        self.surfaces = surfaces.map(Arc::from);
    }

    /// The Collision modules' LOD distance factor: the drawn view's horizontal FOV / max(0.01, DefaultFOV), the
    /// PlayerCameraManager's DefaultFOV being 90 (142b59fe5). 1 until set.
    pub fn set_lod_factor(&mut self, lod: f64) {
        self.lod = lod;
    }

    /// The Collision modules' view set outright (a replay's; `draws` sets the eye otherwise).
    pub fn set_collision_view(&mut self, loc: [f64; 3], lod: f64) {
        self.collision_loc = Some(loc);
        self.lod = lod;
    }

    fn collision_view(&self) -> Option<CollisionView> {
        self.collision_loc.map(|loc| CollisionView { loc, lod: self.lod })
    }

    /// A new system meets the stage through the view of the last drawn frame (VfxPlayer.meet).
    fn meet(&self, s: &mut SystemInstance) {
        s.sys.surfaces.clone_from(&self.surfaces);
        s.sys.collision_view = self.collision_view();
    }

    /// The view the next draws are made for: the eye and the forward axis (UE world). The draws' sort and the
    /// ribbons' camera-up read the forward axis; OrientZAxisTowardCamera reads the eye at the next tick.
    pub fn set_view(&mut self, origin: [f64; 3], forward: [f64; 3]) {
        self.view = Some(View { origin, forward });
    }

    /// The camera's up axis (UE world) for the next draws: a beam's up where its own cannot be worked out. (0, 0,
    /// 1) until set.
    pub fn set_camera_up(&mut self, up: [f64; 3]) {
        self.cam_up = up;
    }

    /// The draws of every visible emitter with live particles, seen from `view_origin` (the sort's eye); `out` is
    /// cleared and filled, in the translucent pass's order (TranslucencySortPriority, then back to front by the
    /// view Z of the system's Bounds origin, then MeshIdInPrimitive). The renderer draws a BLEND_Modulate material's
    /// first (its stable partition).
    /// `alpha`: where between the last two ticks the frame falls (1 the latest tick; fill::Blend).
    pub fn draws(&mut self, view_origin: [f64; 3], alpha: f32, out: &mut Vec<EmitterDraw>) {
        // Unverified: without a set_view the forward axis is +X (UE's default camera facing); the sprite sort by
        // ViewProjDepth and the ribbons' camera up read it.
        let forward = self.view.map_or([1.0, 0.0, 0.0], |v| v.forward);
        let view = View { origin: view_origin, forward };
        self.view = Some(view);
        self.collision_loc = Some(view_origin);
        self.spare.extend(out.drain(..).map(|d| (d.vertices, d.material_scalars)));
        self.params.clear();
        for &i in &self.live {
            let slot = &mut self.slots[i as usize];
            let s = slot.sys.as_mut().unwrap();
            if s.hidden || s.ghost {
                continue;
            }
            let z = view_z(s.bounds.o, &view);
            let blend = Blend { alpha: f64::from(alpha), from: s.draw_from };
            let mut id = 0;
            for (ei, em) in s.emitters.iter_mut().enumerate() {
                if em.particle_count() == 0 {
                    continue;
                }
                let t = em.t.clone();
                let (mut vertices, mut material_scalars) = self.spare.pop().unwrap_or_default();
                vertices.clear();
                let (kind, stride, count) = match t.kind {
                    Kind::Sprite => (
                        EmitterKind::Sprite,
                        SPRITE_STRIDE,
                        fill_sprites(&t, &s.sys, em, &view, &blend, &mut self.sprite_sort, &mut vertices),
                    ),
                    Kind::Mesh => (
                        EmitterKind::Mesh { mesh: t.mesh.clone().unwrap() },
                        MESH_STRIDE,
                        fill_meshes(&t, &s.sys, em, &view, &blend, &mut vertices),
                    ),
                    Kind::Ribbon => {
                        (EmitterKind::Ribbon, RIBBON_STRIDE, fill_ribbon(&t, &s.sys, em, &view, &mut vertices))
                    }
                    Kind::Beam => (
                        EmitterKind::Beam,
                        RIBBON_STRIDE,
                        fill_beam(&t, &s.sys, em, &view, self.cam_up, &mut *self.rng, &mut vertices),
                    ),
                };
                if count == 0 {
                    self.spare.push((vertices, material_scalars));
                    continue;
                }
                material_scalars.clone_from(&s.material_scalars);
                let mut p = draw_params(&t, &s.sys, slot.sections[ei]);
                p.view_z = z as f32;
                p.mesh_id = id;
                id += 1;
                out.push(EmitterDraw {
                    kind,
                    material: t.material.clone(),
                    material_scalars,
                    local_to_world: None,
                    vertices,
                    stride,
                    count,
                    sort: t.sort,
                    sort_priority: s.sort_priority,
                });
                self.params.push(p);
            }
        }
        // The translucent pass sort (SortByProjectedZ): a stable insertion sort, a draw after another when its
        // priority is higher, else when it is nearer (the view Z descending), else by MeshIdInPrimitive; each
        // difference tested in a chain where 0 and NaN fall through.
        for i in 1..out.len() {
            let mut j = i;
            while j > 0 && draws_after(&out[j - 1], &self.params[j - 1], &out[j], &self.params[j]) {
                out.swap(j - 1, j);
                self.params.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    /// The uniform buffer values of the draws the last `draws` handed out, one per draw in the same order.
    pub fn draw_params(&self) -> &[DrawParams] {
        &self.params
    }
}

/// One entry of a beam's user point array (VfxPlayer.beams' put): grown with zero points to `index`; a negative
/// emitter or index sets nothing.
fn put_beam_point(arr: &mut Vec<Vec<V3>>, emitter: i32, point: V3, index: i32) {
    let (Ok(e), Ok(i)) = (usize::try_from(emitter), usize::try_from(index)) else { return };
    if arr.len() <= e {
        arr.resize(e + 1, Vec::new());
    }
    let a = &mut arr[e];
    if a.len() <= i {
        a.resize(i + 1, [0.0; 3]);
    }
    a[i] = point;
}

fn draws_after(a: &EmitterDraw, ap: &DrawParams, b: &EmitterDraw, bp: &DrawParams) -> bool {
    let p = a.sort_priority - b.sort_priority;
    if p != 0 {
        return p > 0;
    }
    let z = f64::from(bp.view_z) - f64::from(ap.view_z);
    if z != 0.0 && !z.is_nan() {
        return z > 0.0;
    }
    ap.mesh_id > bp.mesh_id
}
