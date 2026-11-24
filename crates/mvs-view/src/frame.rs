//! The frame input: what the renderer draws one tick with. Places are UE world (cm, Z up), matrices UE's row-vector
//! form (p' = p M), as the build's View and Primitive buffers take them.

use std::sync::Arc;

use mvs_sim::Event;

/// A 4x4 matrix, rows as UE's FMatrix M[row][column] (row vectors: p' = p M).
pub type M4 = [[f64; 4]; 4];

/// The view the frame is drawn from (FViewMatrices' inputs).
#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    /// WorldCameraOrigin, cm. The renderer draws in translated world (PreViewTranslation = -origin).
    pub origin: [f64; 3],
    /// TranslatedWorldToView: the view rotation (UE's view axes: X right, Y up, Z forward), no translation.
    pub translated_world_to_view: M4,
    /// ViewToClip: UE's reversed-Z perspective projection.
    pub view_to_clip: M4,
}

/// The times the View buffer and the materials read, seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameTime {
    /// GameTime: the world's time, held by pauses and dilated with it.
    pub game: f32,
    /// RealTime.
    pub real: f32,
    /// DeltaTime: the game time since the last frame.
    pub delta: f32,
}

/// A material slot's material and the parameters set on its instance (a dynamic material instance's scalars,
/// vectors and textures over the material's own).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialRef {
    /// The material's object path in the material kind.
    pub path: Arc<str>,
    pub scalars: Vec<(Arc<str>, f32)>,
    pub vectors: Vec<(Arc<str>, [f32; 4])>,
    /// (parameter, texture object path).
    pub textures: Vec<(Arc<str>, Arc<str>)>,
}

/// A mesh a match may draw, with one material set it may take: what the renderer's warm-up (`Renderer::prewarm`)
/// makes the shaders, states, input layouts and assets of before the first frame. A skeletal mesh is warmed as the
/// views draw one (GPU skinned), a static one as a static mesh.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct WarmMesh {
    /// The mesh's object path in the mesh kind; None: an effects' component mesh of its own geometry
    /// (FLocalVertexFactory).
    pub mesh: Option<Arc<str>>,
    /// Per material slot, the material's object path; None (or past the last): the mesh's own.
    pub materials: Vec<Option<Arc<str>>>,
    /// It may draw custom depth (a stencil value, the outlines).
    pub outlined: bool,
    /// It may draw in PFGCamera's priority captures (a fighter's own meshes).
    pub captured: bool,
    /// The first material goes on every slot (an effects' component mesh: one material over the whole mesh).
    pub every_slot: bool,
}

/// One drawn mesh component: a skeletal mesh posed, or a static mesh (no bones).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshPose {
    /// The mesh's object path in the mesh kind.
    pub mesh: Arc<str>,
    pub lod: u8,
    /// The component's LocalToWorld as FMatrix3x4: the row-vector matrix's three columns, each with its
    /// translation (cm) as the fourth value.
    pub to_world: [f32; 12],
    /// Per bone of the mesh's reference skeleton (its own order): the skinning matrix (FSkeletalMeshObject's
    /// ReferenceToLocal: the ref pose's inverse times the bone's component-space transform), as FMatrix3x4 in
    /// component space, cm. Empty for a static mesh.
    pub bones: Vec<[f32; 12]>,
    /// Per render section of `lod`: drawn (a hidden section is a section the view turned off).
    pub shown: Vec<bool>,
    /// Per material slot of the mesh.
    pub materials: Vec<MaterialRef>,
    /// CustomDepthStencilValue; 0 draws no custom depth.
    pub custom_stencil: u8,
    /// Left out of the camera's priority captures (PFGCamera's; a held item is its own actor).
    pub capture_skip: bool,
    /// bReceivesDecals false on an actor's mesh component (a fighter's meshes receive none whatever this says).
    pub no_decals: bool,
    /// USkinnedMeshComponent's active morph targets (SetMorphTarget): (morph target name, weight), in the order set.
    pub morphs: Vec<(Arc<str>, f32)>,
}

/// A fighter's view: its body, parts and held props.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FighterPose {
    /// The fighter's id (its arena id, as the World keeps it), not its place in the World's fighters.
    pub fighter: u32,
    pub meshes: Vec<MeshPose>,
    /// Its index in PFGCamera's priority stack as SetOpacityMaskTextures last set it (0: OpacityToggle 1; k > 0: its
    /// materials read priority capture min(k, 3)); None for a fighter never in the stack.
    pub priority: Option<u8>,
    /// The view is posed but not drawn (every placed view is posed; the root of
    /// one whose fighter is invisible whole is hidden): its bones are the effects' and the actors' sockets, the
    /// renderer draws none of its meshes.
    pub hidden: bool,
    /// The fighter wears a heart skin the effects draw petals for (heart_skin.rs: slot 0's Skin BaseColor Opacity Max is
    /// not 0 and its character has an entry).
    pub heart: bool,
}

/// A view whose meshes are the stage scenery's, as it places them this frame (The
/// Court's basketball, Item_Basketball_C, whose three instances by material the court block claims): the renderer's
/// `Renderer::scenery_place(block, part, shown)`.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneryPlace {
    /// The scenery block and its part (the ball: "court", "ball").
    pub block: &'static str,
    pub part: &'static str,
    /// The variant shown (a key of the part's data: the ball's "gray", "red", "blue") at its component's UE
    /// transform (row-vector, `stage::transform`'s form); None: every variant hidden.
    pub shown: Option<(&'static str, M4)>,
}

/// An actor's view (a projectile, a spawned actor, a stage actor that moves).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActorPose {
    pub meshes: Vec<MeshPose>,
    /// The spawned actor it draws (None: a pose of the tape's or a stage's).
    pub actor: Option<mvs_sim::actors::ActorRef>,
    /// The bones a class's view enters for its actor's PlayVFX notifies, by name.
    pub bones: Vec<(Arc<str>, crate::actor_view::Xform)>,
    /// A second view on a fighter it draws (crate::second: a Disguise's copy, a Polymorph mesh), whose meshes'
    /// particle notifies spawn on its first mesh.
    pub second: Option<SecondRef>,
}

/// Which second view on a fighter an actor pose draws.
#[derive(Clone, Debug, PartialEq)]
pub struct SecondRef {
    /// The fighter it is on (its arena id): a Disguise's owner, a Polymorph's victim.
    pub fighter: i32,
    pub kind: SecondKind,
    /// The character whose skeleton data (sockets, mirror table) the first mesh has; None: none (a kit's prop mesh,
    /// whose sockets are its bones).
    pub skeleton: Option<Arc<str>>,
    /// The view faces left.
    pub left: bool,
    /// Which take of a view by a buff this is (a new value: a new take, the old view dropped and a new one made, so the
    /// effects on the last take end, a victim switch to an identical fighter included).
    pub take: u64,
}

/// The kinds of second view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SecondKind {
    /// Buff_Disguise's DisguiseMesh.
    Disguise,
    /// Buff_PolymorphParent's Polymorph mesh.
    Polymorph,
}

/// One frame to draw: the tick it shows, the camera, the posed views and the tick's sim events (they spawn the
/// effects the renderer plays).
#[derive(Clone, Copy, Debug)]
pub struct FrameInput<'a> {
    pub tick: u32,
    /// Where between the tick before `tick` (0) and `tick`, the latest (1), the frame is drawn.
    pub alpha: f32,
    pub time: FrameTime,
    pub camera: &'a Camera,
    pub fighters: &'a [FighterPose],
    pub actors: &'a [ActorPose],
    pub events: &'a [Event],
}
