//! The shapes of the scenery bind's per-map parts: what a map's own Blueprint classes and level Blueprints add to its
//! scenery entry, one part per map (`parts/<map>.rs`), entered by class name as the characters' classes are (no
//! default for a class no part enters).
//!
//! The parts work on the object kind's JSON of an object's properties ([`JsonProps`]: references as object paths,
//! members by declared name), the shape their tables and blocks were written over; a level object stays an
//! [`ObjView`]. The hooks run in line.

use std::borrow::Cow;
use std::sync::Arc;

use indexmap::IndexMap;

use crate::json::Json;
use crate::kind::{Cx, KindError};
use crate::kinds::mesh::MeshHeader;
use crate::objview::ObjView;
use crate::stage_levels::StageLevel;

/// An object's properties as the object kind's JSON holds them.
pub type JsonProps = IndexMap<String, Json>;

/// A component's place as the core writes it: its relative location, rotation (degrees: pitch, yaw, roll) and scale,
/// or with an AttachParent its world transform composed up the chain.
pub type SceneryPlace = JsonProps;

/// A static mesh component's lighting as a slot material is made for it: with a
/// shadow map or not. The parts pass none.
#[derive(Clone, Copy, Debug)]
pub struct SlotLightmap {
    pub shadow: bool,
}

/// What gives a level object's properties to [`SceneryBindCtx::place_of`] and `util::world_of` (a function of a level
/// object to its properties): an actor's components over their templates ([`super::parts::util::ActorProps`]), the
/// objects' own ([`OwnProps`]) or a closure.
pub trait PropsOf {
    fn props_of<'s>(&'s self, c: &ObjView<'_>) -> Result<Cow<'s, JsonProps>, KindError>;
}

impl<F: Fn(&ObjView<'_>) -> Result<JsonProps, KindError>> PropsOf for F {
    fn props_of<'s>(&'s self, c: &ObjView<'_>) -> Result<Cow<'s, JsonProps>, KindError> {
        self(c).map(Cow::Owned)
    }
}

/// The objects' own tagged properties (`(c) => c.properties`).
pub struct OwnProps;

impl PropsOf for OwnProps {
    fn props_of<'s>(&'s self, c: &ObjView<'_>) -> Result<Cow<'s, JsonProps>, KindError> {
        Ok(Cow::Owned(crate::json::props_json(c.cache(), c.props())))
    }
}

/// The options of [`SceneryBindCtx::add_skeletal`] (addSkeletal's `opts`; a bare string there is
/// `anim_bp`).
#[derive(Clone, Debug, Default)]
pub struct SkeletalOpts {
    /// The AnimBlueprint class a part read the graph of (instead of the component's AnimClass).
    pub anim_bp: Option<String>,
    /// The one looping sequence the part read that graph's pose to be.
    pub play: Option<SkeletalPlay>,
    /// `[skeletal mesh, bone]` it is attached to (its place under the bone).
    pub attach: Option<Json>,
    /// It starts hidden (a block shows it).
    pub hidden: bool,
    /// The part's own fields of the skeletal entry.
    pub fields: Option<JsonProps>,
    /// The part says it is not playing (checked).
    pub paused: bool,
}

#[derive(Clone, Debug)]
pub struct SkeletalPlay {
    pub anim: String,
    pub rate: f64,
}

/// What a part's hooks get of the level being bound. The core's side keeps its state behind
/// interior mutability: every method takes `&self`.
pub trait SceneryBindCtx<'a> {
    /// The stage key.
    fn stage(&self) -> &str;
    /// The level, loaded (its World, objects and actors).
    fn level(&self) -> &StageLevel<'a>;
    /// The run's reads (a part reads the animation kind's curves through it: Sky Arena's arm).
    fn cx(&self) -> &Cx<'a>;
    /// An object of the build.
    fn load(&self, path: &str) -> Result<ObjView<'a>, KindError>;
    /// An actor's DefaultSceneRoot properties.
    fn root_of(&self, owner: &ObjView<'a>) -> Result<JsonProps, KindError>;
    /// An animation a scenery mesh plays, by build object path; root motion refused unless `root_motion`.
    fn anim_path(&self, anim: &str, root_motion: bool) -> Result<String, KindError>;
    /// A skeletal mesh's material in the skin context (or its effect pass) as a DrawRef, `None` for one not built (a
    /// lit translucency with surface lighting).
    fn skin_material(&self, material: &str, name: &str) -> Result<Option<String>, KindError>;
    /// A skeletal mesh component written as the core writes one (name, component properties, its place, the
    /// options); returns the animation it plays, if any.
    fn add_skeletal(
        &self,
        name: &str,
        component: &JsonProps,
        at: &SceneryPlace,
        opts: &SkeletalOpts,
    ) -> Result<Option<String>, KindError>;
    /// A particle system the scenery spawns (the `particles` kind), by its build object path: its short name.
    fn add_system(&self, system: &str) -> Result<String, KindError>;
    /// A mesh's header (the `mesh` kind's: its material slots and LOD sections).
    fn mesh_header(&self, mesh: &str) -> Result<Arc<MeshHeader>, KindError>;
    /// A static mesh the scenery draws (the `mesh` kind): its index in the entry's meshes.
    fn add_mesh(&self, mesh: &str) -> Result<usize, KindError>;
    /// A component's slot materials made for its lighting policy (lightmap / shadow map / none), as DrawRefs (`None`:
    /// a slot not drawn).
    fn build_for(
        &self,
        materials: &[Option<String>],
        lightmap: Option<&SlotLightmap>,
        channels: Option<&[bool]>,
    ) -> Result<Vec<Option<String>>, KindError>;
    /// A component's place (above); `props` gives a level object's properties (its own when `None`).
    fn place_of(&self, component: &ObjView<'a>, props: Option<&dyn PropsOf>) -> Result<SceneryPlace, KindError>;
    /// A placed emitter the part writes (a Blueprint actor's particle component).
    fn add_emitter(&self, emitter: JsonProps);
    /// An instance written whole, hidden at the start, for the part's block to place.
    fn add_instance(&self, instance: JsonProps);
}

/// A StaticMeshActor a placed actor of a part's class attaches to one of its components at BeginPlay
/// (K2_AttachToComponent): the attached actor's name, the parent component, the properties of the parent's actor's
/// components, and the location / rotation / scale EAttachmentRule (0 KeepRelative, 1 KeepWorld).
pub struct SceneryBeginAttach<'a> {
    pub actor: String,
    pub parent: ObjView<'a>,
    pub props: Box<dyn PropsOf + 'a>,
    pub rules: [u8; 3],
}

/// The actor a level Blueprint's `instance` hook is asked about: a placed actor, or the core's stand-in for a
/// component with no actor of its own (the `{ path: owner.path + '.' + name, class: '', properties: {} }`
/// object: no tags, no properties).
#[derive(Clone, Copy)]
pub enum InstanceActor<'v, 'a> {
    Placed(&'v ObjView<'a>),
    Path(&'v str),
}

impl InstanceActor<'_, '_> {
    pub fn path(&self) -> &str {
        match self {
            InstanceActor::Placed(o) => o.path(),
            InstanceActor::Path(p) => p,
        }
    }

    /// Its name (the last of its chain).
    pub fn name(&self) -> &str {
        crate::objview::short_name(self.path())
    }
}

/// What a level Blueprint's part reads of its level (`SceneryLevelRun`): the actor tags its script reads. Its other
/// members, the instance fields and the hidden actors it shows, are [`SceneryPartRun`]'s `instance`, `shows` and
/// `skeletal_shows`, asked about the level the last `level` call bound.
#[derive(Clone, Debug, Default)]
pub struct SceneryLevelRun {
    pub tags: Vec<String>,
}

/// What `static_mesh` gives: the component's properties to write, the instance's own fields, and `placed` when the
/// part wrote the component's world transform into `p`'s relative one.
#[derive(Clone, Debug, Default)]
pub struct SceneryMeshOut {
    pub p: JsonProps,
    pub extra: JsonProps,
    pub placed: bool,
}

/// A part's answer for a static mesh component: no hook (the core throws), not drawn
/// (`null`), or drawn.
#[derive(Clone, Debug)]
pub enum StaticMeshOut {
    NoHook,
    NotDrawn,
    Drawn(SceneryMeshOut),
}

/// One bind's state of a part. A hook the part does not have keeps its default.
pub trait SceneryPartRun<'a> {
    /// A level Blueprint's own: the actor tags its script reads; the level's state for the hooks below.
    fn level(&mut self, cls: &str, ctx: &dyn SceneryBindCtx<'a>) -> Result<SceneryLevelRun, KindError> {
        let _ = ctx;
        Err(KindError::Failed(format!("scenery: level Blueprint {cls}: the part has no level hook")))
    }
    /// The instance fields of an actor of the level `level` bound.
    fn instance(&mut self, actor: InstanceActor<'_, 'a>) -> Result<JsonProps, KindError> {
        let _ = actor;
        Ok(JsonProps::new())
    }
    /// A hidden StaticMeshActor of a gameplay or lighting level its script shows: its fields, or `None`.
    fn shows(&mut self, actor: &ObjView<'a>) -> Result<Option<JsonProps>, KindError> {
        let _ = actor;
        Ok(None)
    }
    /// A hidden SkeletalMeshActor its script shows: the skeletal entry's fields, or `None` (Tree Fort's snails).
    fn skeletal_shows(&mut self, actor: &ObjView<'a>) -> Result<Option<JsonProps>, KindError> {
        let _ = actor;
        Ok(None)
    }
    /// A skeletal mesh component of one of its classes: true when the part wrote it.
    fn skeletal_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        component: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        let _ = (cls, ctx, owner, component);
        Ok(false)
    }
    /// A static mesh component of one of its classes: its properties to write and the instance's own fields.
    fn static_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        component: &ObjView<'a>,
        props: &JsonProps,
    ) -> Result<StaticMeshOut, KindError> {
        let _ = (cls, ctx, owner, component, props);
        Ok(StaticMeshOut::NoHook)
    }
    /// A particle component of one of its classes: true when the part took it (`ctx.add_emitter`, or kept for its
    /// block).
    fn particle_system(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        component: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        let _ = (cls, ctx, owner, component);
        Ok(false)
    }
    /// A placed SkeletalMeshActor whose AnimBlueprint the part enters: true when written.
    fn skeletal_actor(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        component: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        let _ = (cls, ctx, owner, component);
        Ok(false)
    }
    /// The entry's blocks of the part: after the instances.
    fn level_blocks(&mut self) -> Result<JsonProps, KindError> {
        Ok(JsonProps::new())
    }
    /// The entry's blocks of the part: after the skeletal meshes.
    fn actor_blocks(&mut self) -> Result<JsonProps, KindError> {
        Ok(JsonProps::new())
    }
    /// A line for the import's log.
    fn summary(&self) -> String;
}

/// A map's part.
pub trait SceneryPart: Sync {
    fn name(&self) -> &'static str;
    /// The placed Blueprint actor classes (with _C) whose components it writes.
    fn blueprints(&self) -> &'static [&'static str] {
        &[]
    }
    /// The level Blueprint classes (Map_..._C) whose level script it reads.
    fn levels(&self) -> &'static [&'static str] {
        &[]
    }
    /// The actor tags those level scripts read (an actor with one in a level whose script does not read it fails).
    fn level_tags(&self) -> &'static [&'static str] {
        &[]
    }
    /// Class -> why the scenery needs nothing of it.
    fn ignore(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
    /// The AnimBlueprint classes (with _C) of placed SkeletalMeshActors it writes (`skeletal_actor`).
    fn anim_blueprints(&self) -> &'static [&'static str] {
        &[]
    }
    /// Translucent materials whose entry keeps its preshaders for the renderer's dynamic instances.
    fn keep_params(&self) -> &'static [&'static str] {
        &[]
    }
    /// Its Blueprint classes whose particle components it writes in a gameplay or lighting level too.
    fn gameplay_particles(&self) -> &'static [&'static str] {
        &[]
    }
    /// The materials (a shader map's owner) whose base pass vertex shader's world position offset the renderer runs: a
    /// static mesh slot drawn with one, or with an instance of one, gets `wpo`.
    fn wpo(&self) -> &'static [&'static str] {
        &[]
    }
    /// The StaticMeshActors a placed actor of its class attaches at BeginPlay.
    fn begin_attach<'a>(
        &self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
    ) -> Result<Vec<SceneryBeginAttach<'a>>, KindError> {
        let _ = (cls, ctx, owner);
        Ok(Vec::new())
    }
    /// One bind's state.
    fn run<'a>(&self) -> Box<dyn SceneryPartRun<'a> + 'a>;
}
