//! What one emitter hands the renderer each frame: its particles as the build's particle vertex factory takes them
//! (FDynamicSpriteEmitterData, FDynamicMeshEmitterData, the beam and trail emitters' data).

use std::sync::Arc;

/// The emitter's TypeData: which vertex factory draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum EmitterKind {
    /// No TypeData: FParticleSpriteVertexFactory.
    Sprite,
    /// TypeDataMesh: FMeshParticleVertexFactory over the static mesh at this object path.
    Mesh { mesh: Arc<str> },
    /// TypeDataBeam2: FParticleBeamTrailVertexFactory.
    Beam,
    /// TypeDataRibbon: FParticleBeamTrailVertexFactory.
    Ribbon,
    /// TypeDataAnimTrail: FParticleBeamTrailVertexFactory.
    AnimTrail,
}

/// UParticleModuleRequired SortMode (EParticleSortMode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    None,
    ViewProjDepth,
    DistanceToView,
    AgeOldestFirst,
    AgeNewestFirst,
}

/// One emitter's draw.
#[derive(Clone, Debug, PartialEq)]
pub struct EmitterDraw {
    pub kind: EmitterKind,
    /// The material's object path (the emitter's, or its system's override).
    pub material: Arc<str>,
    /// The component's dynamic material instance scalars.
    pub material_scalars: Vec<(Arc<str>, f32)>,
    /// A local-space emitter's LocalToWorld as FMatrix3x4 (the row-vector matrix's three columns, each with its
    /// translation); None for a world-space one.
    pub local_to_world: Option<[f32; 12]>,
    /// The particles in the build's vertex layout for `kind` (FParticleSpriteVertex, FMeshParticleInstanceVertex,
    /// FParticleBeamTrailVertex), already sorted.
    pub vertices: Vec<u8>,
    /// Bytes per vertex in `vertices`.
    pub stride: u32,
    pub count: u32,
    pub sort: SortMode,
    /// The component's TranslucencySortPriority.
    pub sort_priority: i32,
}

/// What the vertex factory's uniform buffer takes of one emitter (FParticleSpriteUniformParameters,
/// FMeshParticleUniformParameters), aligned with the draws `Player::draws` hands out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawParams {
    /// AxisLockUp / AxisLockRight (OrientationAxisLock through LocalToWorld.GetUnitAxis), zero without a lock; a
    /// rotate lock leaves Up zero.
    pub lock_up: [f32; 3],
    pub lock_right: [f32; 3],
    /// A fixed axis lock. The renderer culls a sprite's back only when its material is one-sided, axis_locked is
    /// set and velocity_aligned is not; every other sprite draws both faces.
    pub axis_locked: bool,
    /// EPAL_ROTATE_X / Y / Z: the sprite turns about lock_right.
    pub rotate_lock: bool,
    /// PSA_Velocity.
    pub velocity_aligned: bool,
    /// The sprite's corner offset: (-0.5, -0.5) plus the PivotOffset modules'.
    pub pivot: [f32; 2],
    /// SubImages_Horizontal, SubImages_Vertical.
    pub sub_images: [f32; 2],
    /// RotationScale and RotationBias (142c64010).
    pub rotation_scale: f32,
    pub rotation_bias: f32,
    /// A mesh emitter's one section (else every section).
    pub mesh_section: Option<u32>,
    /// The component's Scale3D turns a mesh inside out (an odd count of negative axes): the other cull side.
    pub mirrored: bool,
    /// The system's translucent sort key parts: the view Z of its Bounds origin as a float, and MeshIdInPrimitive.
    pub view_z: f32,
    pub mesh_id: u32,
}
