//! An emitter's template: its Required / Spawn set-up,
//! ribbon spec, draw set-up and modules, read once per loaded emitter.

use std::sync::Arc;

use serde_json::Value;

use mvs_math::hypot::hypot3;

use crate::data::{Dist, EmitterData, dist, dist_of, flag_or, num_or, prop, str_or, truthy, truthy_value, xyz_or};
use crate::draw::SortMode;
use crate::math::V3;

/// One entry of a ParameterDynamic module's DynamicParams (the first four).
#[derive(Clone, Debug)]
pub struct DynamicParam {
    pub method: DynMethod,
    /// ParamValue is read (EDPV_UserSet or bScaleVelocityByParamValue).
    pub sampled: bool,
    pub spawn_only: bool,
    pub emitter_time: bool,
    pub value: Dist,
}

/// EEmitterDynamicParameterValue as the module's update reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynMethod {
    Other,
    AutoSet,
    VelocityX,
    VelocityY,
    VelocityZ,
    VelocityMag,
}

/// One of an Orbit module's three options with its amount.
#[derive(Clone, Debug)]
pub struct OrbitOption {
    pub amount: Dist,
    pub spawn: bool,
    pub update: bool,
    pub emitter_time: bool,
}

/// A module the emitter runs, its properties read once. `None` does nothing at spawn or update (the template-level
/// modules, and a type with no case).
#[derive(Clone, Debug)]
// (one array of a template's modules, read in place: the size difference costs nothing per tick)
#[allow(clippy::large_enum_variant)]
pub enum Module {
    None,
    Lifetime {
        lifetime: Dist,
    },
    Size {
        start: Dist,
    },
    Color {
        color: Dist,
        alpha: Dist,
    },
    ColorOverLife {
        color: Dist,
        alpha: Dist,
    },
    Rotation {
        start: Dist,
    },
    RotationRate {
        start: Dist,
    },
    Velocity {
        start: Dist,
        radial: Dist,
        world: bool,
    },
    Location {
        start: Dist,
    },
    LocationSphere {
        pos: [bool; 3],
        neg: [bool; 3],
        radius: Dist,
        surface: bool,
        start: Dist,
        velocity: bool,
        velocity_scale: Dist,
    },
    LocationCylinder {
        r0: usize,
        r1: usize,
        h: usize,
        pos: [bool; 3],
        neg: [bool; 3],
        start: Dist,
        radius: Dist,
        height: Dist,
        surface: bool,
        velocity: bool,
        radial_velocity: bool,
        velocity_scale: Dist,
    },
    CameraOffset {
        offset: Dist,
        spawn_only: bool,
    },
    SubUV {
        index: Dist,
    },
    /// UParticleModuleSubUVMovie: FrameRate (at EmitterTime with bUseEmitterTime), StartingFrame.
    SubUvMovie {
        frame_rate: Dist,
        emitter_time: bool,
        starting_frame: f64,
    },
    /// UParticleModuleLocationDirect: Location, LocationOffset, ScaleFactor.
    LocationDirect {
        location: Dist,
        offset: Dist,
        scale: Dist,
    },
    /// UParticleModuleMeshRotationRateOverLife: RotRate, bScaleRotRate.
    MeshRotationRateOverLife {
        rate: Dist,
        scale: bool,
    },
    ParameterDynamic {
        params: Vec<DynamicParam>,
    },
    ColorScaleOverLife {
        emitter_time: bool,
        color: Dist,
        alpha: Dist,
    },
    SizeMultiplyLife {
        mult: Dist,
        axes: [bool; 3],
    },
    RotationRateMultiplyLife {
        mult: Dist,
    },
    AttractorPoint {
        position: Dist,
        world_position: bool,
        range: Dist,
        by_distance: bool,
        strength: Dist,
        neg: [bool; 3],
        pos: [bool; 3],
        base: bool,
    },
    SourceMovement {
        scale: Dist,
    },
    VelocityOverLifetime {
        absolute: bool,
        vel: Dist,
    },
    LocationEmitter {
        name: String,
        sequential: bool,
        inherit_velocity: bool,
        velocity_scale: f64,
        inherit_rotation: bool,
        rotation_scale: f64,
    },
    Orbit {
        offset: OrbitOption,
        rotation: OrbitOption,
        rate: OrbitOption,
    },
    TypeDataMesh {
        rpy: Dist,
    },
    MeshRotation {
        start: Dist,
        inherit_parent: bool,
    },
    MeshRotationRate {
        start: Dist,
    },
    Acceleration {
        accel: Dist,
        owner_scale: bool,
        world: bool,
    },
    AccelerationDrag {
        drag: Dist,
    },
    AccelerationOverLifetime {
        accel: Dist,
        world: bool,
    },
    /// UParticleModuleVelocityCone: Angle, Velocity, Direction (unit, +Z when absent or zero), bInWorldSpace.
    VelocityCone {
        angle: Dist,
        velocity: Dist,
        direction: V3,
        world: bool,
    },
    /// UParticleModuleRotationOverLifetime: RotationOverLife, Scale (default true).
    RotationOverLifetime {
        over: Dist,
        scale: bool,
    },
    /// UParticleModuleSizeScale: SizeScale.
    SizeScale {
        scale: Dist,
    },
    /// UParticleModuleSizeScaleBySpeed: SpeedScale (X, Y; default 0) and MaxScale (X, Y; default 1).
    SizeScaleBySpeed {
        speed: [f64; 2],
        max: [f64; 2],
    },
    /// UParticleModuleMeshRotationRateMultiplyLife: LifeMultiplier.
    MeshRotationRateMultiplyLife {
        mult: Dist,
    },
    /// UParticleModuleAccelerationConstant: Acceleration (a plain vector), bAlwaysInWorldSpace.
    AccelerationConstant {
        accel: V3,
        world: bool,
    },
    /// UParticleModuleCollision: DampingFactor, DampingFactorRotation, MaxCollisions, DelayAmount (None when absent),
    /// CollisionCompletionOption, MaxCollisionDistance (default 1000).
    Collision {
        damping: Dist,
        damping_rotation: Dist,
        max: Dist,
        delay: Option<Dist>,
        done: Completion,
        reach: f64,
    },
}

/// ECollisionCompletionOption after the last counted hit: EPCC_Kill kills, the others set particle Flags
/// (EPCC_Freeze's full freeze is left out: no module update here skips a frozen particle and no emitter here takes
/// it, so it and an unknown option set none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Kill,
    Flags(u32),
}

/// The particle Flags bits the port keeps (EParticleStates, the values UParticleModuleCollision::Update 142c5e3e0 sets
/// and tests).
pub const PF_FREEZE: u32 = 0x0400_0000;
pub const PF_IGNORE_COLLISIONS: u32 = 0x0800_0000;
pub const PF_FREEZE_TRANSLATION: u32 = 0x1000_0000;
pub const PF_FREEZE_ROTATION: u32 = 0x2000_0000;
/// The four the Collision Update skips a particle on.
pub const PF_COLLISION_IGNORE_CHECK: u32 = 0x3c00_0000;
pub const PF_DELAY_COLLISIONS: u32 = 0x4000_0000;
pub const PF_COLLISION_HAS_OCCURRED: u32 = 0x8000_0000;

fn completion(option: &str) -> Completion {
    match option {
        "EPCC_HaltCollisions" => Completion::Flags(PF_IGNORE_COLLISIONS),
        "EPCC_FreezeTranslation" => Completion::Flags(PF_FREEZE_TRANSLATION),
        "EPCC_FreezeRotation" => Completion::Flags(PF_FREEZE_ROTATION),
        "EPCC_FreezeMovement" => Completion::Flags(PF_FREEZE_TRANSLATION | PF_FREEZE_ROTATION),
        "EPCC_Kill" => Completion::Kill,
        _ => Completion::Flags(0),
    }
}

/// A ribbon's TrailSource with SourceMethod PET2SRCM_Particle: the source emitter's name and EPSSM_Random (else
/// sequential, the class default: ctor 142c66940 writes 1 at +0x88; no exported TrailSource sets it).
#[derive(Clone, Debug)]
pub struct TrailSource {
    pub name: String,
    pub random: bool,
}

/// The module types module_of has no port for (each an unported! arm there).
pub const UNPORTED_MODULES: [&str; 0] = [];

/// The module types EmitterInstance.update has a case for (UPDATE_TYPES).
const UPDATE_TYPES: [&str; 22] = [
    "Orbit",
    "ColorOverLife",
    "ColorScaleOverLife",
    "SizeMultiplyLife",
    "SizeScale",
    "SizeScaleBySpeed",
    "SubUV",
    "SubUVMovie",
    "LocationDirect",
    "ParameterDynamic",
    "CameraOffset",
    "Acceleration",
    "AccelerationConstant",
    "Collision",
    "AccelerationOverLifetime",
    "AccelerationDrag",
    "RotationRateMultiplyLife",
    "MeshRotationRateOverLife",
    "MeshRotationRateMultiplyLife",
    "RotationOverLifetime",
    "AttractorPoint",
    "VelocityOverLifetime",
];

fn signs(m: &Value, pre: &str) -> [bool; 3] {
    ["X", "Y", "Z"].map(|a| flag_or(m, &format!("{pre}{a}"), true))
}

fn orbit_option(m: &Value, options: &str, amount: &str) -> OrbitOption {
    let o = prop(m, options);
    OrbitOption {
        amount: dist(m, amount),
        spawn: o.is_none_or(|o| flag_or(o, "bProcessDuringSpawn", true)),
        update: o.is_some_and(|o| truthy(o, "bProcessDuringUpdate")),
        emitter_time: o.is_some_and(|o| truthy(o, "bUseEmitterTime")),
    }
}

fn dynamic_params(m: &Value) -> Vec<DynamicParam> {
    let Some(list) = prop(m, "DynamicParams").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .take(4)
        .map(|d| {
            let vm = str_or(d, "ValueMethod", "EDPV_UserSet");
            let method = if vm.ends_with("EDPV_AutoSet") {
                DynMethod::AutoSet
            } else if vm.ends_with("EDPV_VelocityX") {
                DynMethod::VelocityX
            } else if vm.ends_with("EDPV_VelocityY") {
                DynMethod::VelocityY
            } else if vm.ends_with("EDPV_VelocityZ") {
                DynMethod::VelocityZ
            } else if vm.ends_with("EDPV_VelocityMag") {
                DynMethod::VelocityMag
            } else {
                DynMethod::Other
            };
            DynamicParam {
                method,
                sampled: vm.ends_with("EDPV_UserSet") || truthy(d, "bScaleVelocityByParamValue"),
                spawn_only: truthy(d, "bSpawnTimeOnly"),
                emitter_time: truthy(d, "bUseEmitterTime"),
                value: dist(d, "ParamValue"),
            }
        })
        .collect()
}

/// A module's properties, read as its spawn and update cases read them. A type the build plays and the port has not
/// taken over panics.
fn module_of(m: &Value) -> Module {
    let ty = str_or(m, "type", "");
    match ty.as_str() {
        "Lifetime" => Module::Lifetime { lifetime: dist(m, "Lifetime") },
        // Deviation: each _Seeded module below draws from the player's one stream as its plain module does, not from
        // the module's own FRandomStream (GetRandomStream 142c13ce0: the seed instance parameter, else RandomSeeds[0];
        // reset at every emitter loop by bResetSeedOnEmitterLooping, which the Seeded ctors set), so its fixed
        // sequence, the same at every spawn of the system, is not reproduced; the distribution is the module's. Size_
        // Seeded's Spawn is Size's own 142c5b1c0 handed the module's stream (its StartSize and UV-flip draws);
        // Color_Seeded (ctor 142c339e0, vtable 1442c4a08) the same 142c59470.
        "Size" | "Size_Seeded" => Module::Size { start: dist(m, "StartSize") },
        "Color" | "Color_Seeded" => Module::Color { color: dist(m, "StartColor"), alpha: dist(m, "StartAlpha") },
        "ColorOverLife" => Module::ColorOverLife { color: dist(m, "ColorOverLife"), alpha: dist(m, "AlphaOverLife") },
        "Rotation" | "Rotation_Seeded" => Module::Rotation { start: dist(m, "StartRotation") },
        "RotationRate" | "RotationRate_Seeded" => Module::RotationRate { start: dist(m, "StartRotationRate") },
        "VelocityCone" => Module::VelocityCone {
            angle: dist(m, "Angle"),
            velocity: dist(m, "Velocity"),
            direction: {
                let f = xyz_or(m, "Direction", 0.0);
                let l = hypot3(f[0], f[1], f[2]);
                if l > 1e-8 { [f[0] / l, f[1] / l, f[2] / l] } else { [0.0, 0.0, 1.0] }
            },
            world: truthy(m, "bInWorldSpace"),
        },
        "Velocity" => Module::Velocity {
            start: dist(m, "StartVelocity"),
            radial: dist(m, "StartVelocityRadial"),
            world: truthy(m, "bInWorldSpace"),
        },
        "Location" => Module::Location { start: dist(m, "StartLocation") },
        // Deviation: the _Seeded variant draws from the player's one stream as the plain module does, not from the
        // module's own FRandomStream (its fixed sequence, the same at every spawn, is not reproduced).
        "LocationPrimitiveSphere" | "LocationPrimitiveSphere_Seeded" => Module::LocationSphere {
            pos: signs(m, "Positive_"),
            neg: signs(m, "Negative_"),
            radius: dist(m, "StartRadius"),
            surface: truthy(m, "SurfaceOnly"),
            start: dist(m, "StartLocation"),
            velocity: truthy(m, "Velocity"),
            velocity_scale: dist(m, "VelocityScale"),
        },
        // (LocationPrimitiveCylinder_Seeded, vtable 1442c7070: Spawn 142c5a350 fetches the module's stream and calls
        // the same 142c5b940 with it; the Deviation above)
        "LocationPrimitiveCylinder" | "LocationPrimitiveCylinder_Seeded" => {
            let ha = str_or(m, "HeightAxis", "PMLPC_HEIGHTAXIS_Z");
            let x = ha.ends_with("_X");
            let y = ha.ends_with("_Y");
            Module::LocationCylinder {
                r0: usize::from(x),
                r1: if x || y { 2 } else { 1 },
                h: if x {
                    0
                } else if y {
                    1
                } else {
                    2
                },
                pos: signs(m, "Positive_"),
                neg: signs(m, "Negative_"),
                start: dist(m, "StartLocation"),
                radius: dist(m, "StartRadius"),
                height: dist(m, "StartHeight"),
                surface: truthy(m, "SurfaceOnly"),
                velocity: truthy(m, "Velocity"),
                radial_velocity: flag_or(m, "bRadialVelocity", true),
                velocity_scale: dist(m, "VelocityScale"),
            }
        }
        "CameraOffset" => {
            Module::CameraOffset { offset: dist(m, "CameraOffset"), spawn_only: truthy(m, "bSpawnTimeOnly") }
        }
        "SubUV" => Module::SubUV { index: dist(m, "SubImageIndex") },
        // Unverified: bUseRealTime (the time dilation's divide) is not read; no emitter here sets it.
        "SubUVMovie" => Module::SubUvMovie {
            frame_rate: dist(m, "FrameRate"),
            emitter_time: truthy(m, "bUseEmitterTime"),
            starting_frame: num_or(m, "StartingFrame", 1.0),
        },
        "LocationDirect" => Module::LocationDirect {
            location: dist(m, "Location"),
            offset: dist(m, "LocationOffset"),
            scale: dist(m, "ScaleFactor"),
        },
        "MeshRotationRateOverLife" => {
            Module::MeshRotationRateOverLife { rate: dist(m, "RotRate"), scale: truthy(m, "bScaleRotRate") }
        }
        "ParameterDynamic" => Module::ParameterDynamic { params: dynamic_params(m) },
        "ColorScaleOverLife" => Module::ColorScaleOverLife {
            emitter_time: truthy(m, "bEmitterTime"),
            color: dist(m, "ColorScaleOverLife"),
            alpha: dist(m, "AlphaScaleOverLife"),
        },
        "SizeMultiplyLife" => Module::SizeMultiplyLife {
            mult: dist(m, "LifeMultiplier"),
            axes: [flag_or(m, "MultiplyX", true), flag_or(m, "MultiplyY", true), flag_or(m, "MultiplyZ", true)],
        },
        "RotationRateMultiplyLife" => Module::RotationRateMultiplyLife { mult: dist(m, "LifeMultiplier") },
        "AttractorPoint" => Module::AttractorPoint {
            position: dist(m, "Position"),
            world_position: truthy(m, "bUseWorldSpacePosition"),
            range: dist(m, "Range"),
            by_distance: flag_or(m, "StrengthByDistance", true),
            strength: dist(m, "Strength"),
            neg: signs(m, "Negative_"),
            pos: signs(m, "Positive_"),
            base: truthy(m, "bAffectBaseVelocity"),
        },
        "SourceMovement" => Module::SourceMovement { scale: dist(m, "SourceMovementScale") },
        "VelocityOverLifetime" => {
            Module::VelocityOverLifetime { absolute: truthy(m, "Absolute"), vel: dist(m, "VelOverLife") }
        }
        "LocationEmitter" => Module::LocationEmitter {
            name: str_or(m, "EmitterName", "undefined"),
            sequential: prop(m, "SelectionMethod").and_then(Value::as_str) == Some("ELESM_Sequential"),
            inherit_velocity: truthy(m, "InheritSourceVelocity"),
            velocity_scale: num_or(m, "InheritSourceVelocityScale", 1.0),
            inherit_rotation: truthy(m, "bInheritSourceRotation"),
            rotation_scale: num_or(m, "InheritSourceRotationScale", 1.0),
        },
        "Orbit" => Module::Orbit {
            offset: orbit_option(m, "OffsetOptions", "OffsetAmount"),
            rotation: orbit_option(m, "RotationOptions", "RotationAmount"),
            rate: orbit_option(m, "RotationRateOptions", "RotationRateAmount"),
        },
        "TypeDataMesh" => Module::TypeDataMesh { rpy: dist(m, "RollPitchYawRange") },
        // Deviation: MeshRotation_Seeded as LocationPrimitiveSphere_Seeded.
        "MeshRotation" | "MeshRotation_Seeded" => {
            Module::MeshRotation { start: dist(m, "StartRotation"), inherit_parent: truthy(m, "bInheritParent") }
        }
        "MeshRotationRate" => Module::MeshRotationRate { start: dist(m, "StartRotationRate") },
        "Acceleration" => Module::Acceleration {
            accel: dist(m, "Acceleration"),
            owner_scale: truthy(m, "bApplyOwnerScale"),
            world: truthy(m, "bAlwaysInWorldSpace"),
        },
        "AccelerationDrag" => Module::AccelerationDrag { drag: dist(m, "DragCoefficientRaw") },
        "RotationOverLifetime" => {
            Module::RotationOverLifetime { over: dist(m, "RotationOverLife"), scale: flag_or(m, "Scale", true) }
        }
        "SizeScale" => Module::SizeScale { scale: dist(m, "SizeScale") },
        // (the ctor 142c34a30 writes MaxScale (1, 1) and leaves SpeedScale at the zeroed CDO's (0, 0))
        "SizeScaleBySpeed" => {
            let ss = prop(m, "SpeedScale").unwrap_or(&Value::Null);
            let ms = prop(m, "MaxScale").unwrap_or(&Value::Null);
            Module::SizeScaleBySpeed {
                speed: [num_or(ss, "X", 0.0), num_or(ss, "Y", 0.0)],
                max: [num_or(ms, "X", 1.0), num_or(ms, "Y", 1.0)],
            }
        }
        "MeshRotationRateMultiplyLife" => Module::MeshRotationRateMultiplyLife { mult: dist(m, "LifeMultiplier") },
        // (DampingFactor, DampingFactorRotation, MaxCollisions, ParticleMass, DelayAmount, CollisionCompletionOption,
        // bOnlyVerticalNormalsDecrementCount, MaxCollisionDistance and bApplyPhysics are the properties the build's
        // data sets; ParticleMass and bApplyPhysics' impulse have nothing to push here: what the traces meet never
        // simulates)
        "Collision" => Module::Collision {
            damping: dist(m, "DampingFactor"),
            damping_rotation: dist(m, "DampingFactorRotation"),
            max: dist(m, "MaxCollisions"),
            delay: prop(m, "DelayAmount").filter(|v| truthy_value(v)).map(dist_of),
            done: completion(&str_or(m, "CollisionCompletionOption", "EPCC_Kill")),
            reach: num_or(m, "MaxCollisionDistance", 1000.0),
        },
        "AccelerationOverLifetime" => Module::AccelerationOverLifetime {
            accel: dist(m, "AccelOverLife"),
            world: truthy(m, "bAlwaysInWorldSpace"),
        },
        "AccelerationConstant" => Module::AccelerationConstant {
            accel: xyz_or(m, "Acceleration", 0.0),
            world: truthy(m, "bAlwaysInWorldSpace"),
        },
        // (the beam modules are not in the LOD's spawn / update lists' work: the beam instance calls them itself from
        // PostSpawn and Tick_ModulePostUpdate, beam_spec and EmitterInstance::tick_beam)
        // PivotOffset, TypeDataRibbon, OrientationAxisLock, EventGenerator and EventReceiverSpawn act through the
        // template; any other type (LocationBoneSocket among them) has no case: nothing.
        _ => Module::None,
    }
}

/// EParticleEventType of an event (EPET_Spawn, EPET_Death).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventType {
    Spawn,
    Death,
    Collision,
}

/// An EventGenerator entry.
#[derive(Clone, Debug)]
pub struct EventSpec {
    pub name: Arc<str>,
    pub freq: f64,
}

/// An EventReceiverSpawn module.
#[derive(Clone, Debug)]
pub struct Receiver {
    pub name: String,
    /// The type it takes; None for EPET_Any.
    pub ty: Option<EventType>,
    pub count: Dist,
    pub psys_loc: bool,
    pub inherit: bool,
    pub inherit_scale: Dist,
}

/// A burst list entry.
#[derive(Clone, Copy, Debug)]
pub struct Burst {
    pub count: f64,
    pub count_low: f64,
    pub time: f64,
}

/// The SpawnPerUnit module.
#[derive(Clone, Debug)]
pub struct SpawnPerUnit {
    pub value: Dist,
    pub unit: f64,
    pub tol: f64,
    pub max_frame: f64,
    pub ignore_rate: bool,
    pub process_rate: bool,
    pub process_bursts: bool,
    pub keep: V3,
}

/// TypeDataRibbon (ctor 142c66a50).
#[derive(Clone, Debug)]
pub struct RibbonSpec {
    pub trails: usize,
    pub max: f64,
    pub tiling: f64,
    pub dead_on_deactivate: bool,
    pub dead_on_loss: bool,
    pub every_frame: bool,
    pub prev_tangent: bool,
    pub initial: bool,
    pub source_up: bool,
    pub camera_up: bool,
    pub step: f64,
    pub tan_scale: f64,
    pub tan_diff: bool,
}

/// A beam end's module (BeamSource ctor 142bdd3e0, BeamTarget 142bdd570).
#[derive(Clone, Debug)]
pub struct BeamEnd {
    /// SourceMethod / TargetMethod: 0 Default, 1 UserSet, 2 Emitter, 3 Particle, 4 Actor.
    pub method: u8,
    /// SourceName / TargetName: the actor instance parameter (Actor).
    pub name: String,
    pub absolute: bool,
    pub point: Dist,
    /// The tangent method (+0x8c; ResolveSourceData 142bf5450 / ResolveTargetData 142bf5cd0): 0 Direct, 1 UserSet,
    /// 2 Distribution, 3 Emitter.
    pub tan_method: u8,
    pub tangent: Dist,
    pub strength: Dist,
    /// The point, its tangent and its strength all locked: the module's Update does nothing.
    pub locked: bool,
}

/// A BeamModifier module as the beam instance keeps it (CacheModuleValues 142be3550: a modifier with
/// PositionOptions, TangentOptions or StrengthOptions bModify (+0x34, +0x80, +0xd4 bit 0) goes to the source slot
/// (ModifierType +0x30 = 0) or the target slot (1), the last such module of the LOD winning), and the payload TypeData
/// Spawn (142bfb770) and Update (142bfdfd0) apply: flags bit 0 / 1 modify / scale the position, 0x10 / 0x20 the
/// strength (bit 2 / 3, the tangent, is not ported: no module here sets it).
#[derive(Clone, Debug)]
pub struct BeamMod {
    pub flags: u32,
    pub pos: Dist,
    pub str: Dist,
}

/// An enabled low-frequency BeamNoise (ctor 142bdd240: NoiseLockRadius 1, NoiseLockTime 0, NoiseTension 0.5,
/// NoiseTessellation 1).
#[derive(Clone, Debug)]
pub struct BeamNoise {
    pub freq: f64,
    pub low: f64,
    pub range: Dist,
    /// NoiseRange's out range (GetOutRange 142a33e80: the min and max over its axes; UpdateBoundingBox's noise
    /// shift).
    pub out_range: (f64, f64),
    pub range_scale: Dist,
    pub lock_time: f64,
    pub tension: f64,
    pub tess: f64,
    pub tangent_strength: Dist,
    /// FrequencyDistance (> 0: the TypeData Update caps the noise points by the beam's length).
    pub freq_dist: f64,
    /// bApplyNoiseScale and NoiseScale: with FrequencyDistance, the noise distance scale.
    pub apply_scale: bool,
    pub scale: Dist,
    /// bTargetNoise: the target point is displaced too (the interpolated-noise fill's last point).
    pub target_noise: bool,
}

/// A distribution's out range (FRawDistributionVector::GetOutRange 142a33e80): the cooked MinValue / MaxValue with a
/// lookup table, else the distribution object's; (0, 0) for none.
// Deviation: the particles kind does not carry MinValue / MaxValue: the min and max over the table's values (an
// unbaked curve's OutVals) stand in. Equal for a constant or uniform table; a cooked curve's samples can miss an
// overshoot between keys. Only the beam's bounding box (culling) reads it.
fn out_range(d: &Dist) -> (f64, f64) {
    let vals: Vec<f64> = if let Some(t) = &d.table {
        t.v.clone()
    } else if let Some(c) = &d.curve {
        c.v.chunks(11).filter(|p| p.len() == 11).flat_map(|p| p[1..4].to_vec()).collect()
    } else {
        Vec::new()
    };
    if vals.is_empty() {
        return (0.0, 0.0);
    }
    vals.iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (mvs_math::js::min(lo, v), mvs_math::js::max(hi, v)))
}

/// TypeDataBeam2 (ctor 142bdd700: BeamMethod Target, TextureTile 1, TextureTileDistance 0, Sheets 1, MaxBeamCount 0,
/// Speed 10, InterpolationPoints 0, bAlwaysOn false, UpVectorStepSize 0, TaperMethod None; InitParameters 142bf03c0
/// turns a MaxBeamCount of 0 into 1 and takes it as the instance's BeamCount) with the LOD's enabled beam modules
/// (CacheModuleValues 142be3550 keeps the first enabled module of each kind; the store leaves disabled modules out,
/// so a disabled BeamNoise, PS_TetherBeam's, does not exist for the instance).
#[derive(Clone, Debug)]
pub struct BeamSpec {
    /// The emitter's place in the system (SetBeamSourcePoint's EmitterIndex), -1 without one.
    pub index: i64,
    pub count: usize,
    pub always_on: bool,
    pub distance: Dist,
    /// BeamMethod PEB2M_Distance.
    pub by_distance: bool,
    pub sheets: f64,
    pub tile: f64,
    pub tile_dist: f64,
    pub up_step: f64,
    /// InterpolationPoints (with noise: the interpolated-noise fill 142c3eb10).
    pub interp: usize,
    /// TaperFactor / TaperScale with a TaperMethod.
    pub taper: Option<(Dist, Dist)>,
    pub source: Option<BeamEnd>,
    pub target: Option<BeamEnd>,
    pub src_mod: Option<BeamMod>,
    pub tgt_mod: Option<BeamMod>,
    pub noise: Option<BeamNoise>,
    /// What the beam has that the port leaves out (warnings), drawn all the same.
    pub gaps: Vec<String>,
}

/// An enum value's name without its type (`EType::Value` -> `Value`).
fn enum_tail(s: &str) -> &str {
    s.rsplit("::").next().unwrap_or(s)
}

const BEAM_METHODS: [&str; 5] =
    ["PEB2STM_Default", "PEB2STM_UserSet", "PEB2STM_Emitter", "PEB2STM_Particle", "PEB2STM_Actor"];
const BEAM_TANGENT_METHODS: [&str; 4] =
    ["PEB2STTM_Direct", "PEB2STTM_UserSet", "PEB2STTM_Distribution", "PEB2STTM_Emitter"];

/// The emitter's beam, None without a TypeDataBeam2.
fn beam_spec(e: &EmitterData) -> Option<BeamSpec> {
    let find = |ty: &str| e.modules.iter().find(|m| m.get("type").and_then(Value::as_str) == Some(ty));
    let td = find("TypeDataBeam2")?;
    let mut gaps = Vec::new();
    let mut end = |m: Option<&Value>, k: &str| -> Option<BeamEnd> {
        let m = m?;
        let method_name = str_or(m, &format!("{k}Method"), "PEB2STM_Default");
        let method = BEAM_METHODS.iter().position(|x| *x == enum_tail(&method_name)).unwrap_or(0);
        // Unverified: Target UserSet, SetBeamTargetPoint, is taken as SetBeamSourcePoint's rule.
        if method == 3 {
            gaps.push(format!("{k}Method {method_name}"));
        }
        let tm = str_or(m, &format!("{k}TangentMethod"), "PEB2STTM_Direct");
        let tan_method = BEAM_TANGENT_METHODS.iter().position(|x| *x == enum_tail(&tm));
        if tan_method.is_none() {
            gaps.push(format!("{k}TangentMethod {tm}"));
        }
        Some(BeamEnd {
            method: method as u8,
            name: str_or(m, &format!("{k}Name"), "None"),
            absolute: truthy(m, &format!("b{k}Absolute")),
            point: dist(m, k),
            tan_method: tan_method.unwrap_or(0) as u8,
            tangent: dist(m, &format!("{k}Tangent")),
            strength: dist(m, &format!("{k}Strength")),
            // (bLockSourceStength: the build's spelling)
            locked: truthy(m, &format!("bLock{k}"))
                && truthy(m, &format!("bLock{k}Tangent"))
                && truthy(m, &format!("bLock{k}Stength")),
        })
    };
    let source = end(find("BeamSource"), "Source");
    let target = end(find("BeamTarget"), "Target");
    let nm = find("BeamNoise");
    let noisy = nm.is_some_and(|n| truthy(n, "bLowFreq_Enabled") && num_or(n, "Frequency", 0.0) != 0.0);
    let count = num_or(td, "MaxBeamCount", 0.0);
    let count = if count != 0.0 && !count.is_nan() { count as usize } else { 1 };
    let sheets = mvs_math::js::max(num_or(td, "Sheets", 1.0), 1.0);
    let tess = mvs_math::js::max(nm.map_or(1.0, |n| num_or(n, "NoiseTessellation", 1.0)), 1.0);
    let interp = mvs_math::js::max(num_or(td, "InterpolationPoints", 0.0), 0.0) as usize;
    let tapered = !str_or(td, "TaperMethod", "PEBTM_None").ends_with("PEBTM_None");
    if num_or(td, "Speed", 10.0) != 0.0 {
        gaps.push("Speed".into());
    }
    // (the noise fill's taper is not ported; the interpolated-noise fill 142c3eb10's is)
    if noisy && tapered && interp == 0 {
        gaps.push("TaperMethod with noise".into());
    }
    if str_or(td, "BeamMethod", "").ends_with("PEB2M_Branch") {
        gaps.push("BeamMethod Branch".into());
    }
    // CacheModuleValues (142be3550): a modifier counts with any of its three bModify set; ModifierType 0 (source) or
    // 1 (target), the last of each kind kept. Unverified: the ctor (142bdd120) was not read: an unserialised
    // ModifierType is taken as PEB2MT_Source.
    let (mut src_mod, mut tgt_mod) = (None, None);
    let null = Value::Null;
    for m in e.modules.iter().filter(|m| m.get("type").and_then(Value::as_str) == Some("BeamModifier")) {
        let po = prop(m, "PositionOptions").unwrap_or(&null);
        let to = prop(m, "TangentOptions").unwrap_or(&null);
        let so = prop(m, "StrengthOptions").unwrap_or(&null);
        if !truthy(po, "bModify") && !truthy(to, "bModify") && !truthy(so, "bModify") {
            continue;
        }
        if truthy(to, "bModify") {
            gaps.push("BeamModifier TangentOptions".into());
        }
        let bit = |b: bool, v: u32| if b { v } else { 0 };
        let md = BeamMod {
            flags: bit(truthy(po, "bModify"), 1)
                | bit(truthy(po, "bScale"), 2)
                | bit(truthy(so, "bModify"), 0x10)
                | bit(truthy(so, "bScale"), 0x20),
            pos: dist(m, "Position"),
            str: dist(m, "Strength"),
        };
        if truthy(po, "bLock") || truthy(so, "bLock") {
            gaps.push("BeamModifier bLock".into());
        }
        let mt = str_or(m, "ModifierType", "PEB2MT_Source");
        match enum_tail(&mt) {
            "PEB2MT_Source" => src_mod = Some(md),
            "PEB2MT_Target" => tgt_mod = Some(md),
            _ => gaps.push(format!("BeamModifier ModifierType {mt}")),
        }
    }
    let noise = match nm {
        Some(n) if noisy => {
            let speed = prop(n, "NoiseSpeed").and_then(|d| prop(d, "table")).and_then(|t| prop(t, "v"));
            if speed.and_then(Value::as_array).is_some_and(|v| v.iter().any(|x| crate::data::number_value(x) != 0.0)) {
                gaps.push("NoiseSpeed".into());
            }
            // (a NoiseLockTime above 1e-4 leaves the points of the Spawn: emitter.rs update_beam)
            if truthy(n, "bOscillate")
                || truthy(n, "bUseNoiseTangents")
                || truthy(n, "bSmooth")
                || truthy(n, "bNoiseLock")
            {
                gaps.push("noise oscillate / noise tangents / smooth noise / noise lock".into());
            }
            // (bTargetNoise: the interpolated-noise fill's alone)
            if interp == 0 && truthy(n, "bTargetNoise") {
                gaps.push("target noise without InterpolationPoints".into());
            }
            Some(BeamNoise {
                freq: num_or(n, "Frequency", 0.0),
                low: num_or(n, "Frequency_LowRange", 0.0),
                range: dist(n, "NoiseRange"),
                out_range: out_range(&dist(n, "NoiseRange")),
                range_scale: dist(n, "NoiseRangeScale"),
                lock_time: num_or(n, "NoiseLockTime", 0.0),
                tension: num_or(n, "NoiseTension", 0.5),
                tess,
                tangent_strength: dist(n, "NoiseTangentStrength"),
                freq_dist: num_or(n, "FrequencyDistance", 0.0),
                apply_scale: truthy(n, "bApplyNoiseScale"),
                scale: dist(n, "NoiseScale"),
                target_noise: truthy(n, "bTargetNoise"),
            })
        }
        _ => None,
    };
    Some(BeamSpec {
        index: e.index.unwrap_or(-1),
        count,
        always_on: truthy(td, "bAlwaysOn"),
        distance: dist(td, "Distance"),
        by_distance: str_or(td, "BeamMethod", "PEB2M_Target").ends_with("PEB2M_Distance"),
        sheets,
        tile: num_or(td, "TextureTile", 1.0),
        tile_dist: num_or(td, "TextureTileDistance", 0.0),
        up_step: num_or(td, "UpVectorStepSize", 0.0),
        interp,
        taper: tapered.then(|| (dist(td, "TaperFactor"), dist(td, "TaperScale"))),
        source,
        target,
        src_mod,
        tgt_mod,
        noise,
        gaps,
    })
}

/// Required ScreenAlignment as the sprite fill takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Square,
    Rect,
    Velocity,
}

/// What the emitter draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sprite,
    Mesh,
    Ribbon,
    /// A TypeDataBeam2 emitter: drawn as a strip, as a ribbon.
    Beam,
}

/// Required InterpolationMethod (SubUV).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubUvMethod {
    None,
    Linear,
    LinearBlend,
    Random,
    RandomBlend,
    Other,
}

/// Required UVFlippingMode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flip {
    None,
    FlipUv,
    RandomFlipUv,
    FlipU,
    RandomFlipU,
    FlipV,
    RandomFlipV,
    RandomFlipUvIndependent,
}

/// ComputeLockedAxes (142c3c4c0): (AxisLockUp, AxisLockRight) for each fixed EParticleAxisLock as signed unit axes
/// (1 = X, 2 = Y, 3 = Z) of LocalToWorld for a local-space emitter or of the identity.
fn axis_lock(flag: &str) -> Option<[i32; 2]> {
    match flag {
        "EPAL_X" => Some([-3, -2]),
        "EPAL_Y" => Some([-3, 1]),
        "EPAL_Z" => Some([-1, 2]),
        "EPAL_NEGATIVE_X" => Some([-3, 2]),
        "EPAL_NEGATIVE_Y" => Some([-3, -1]),
        "EPAL_NEGATIVE_Z" => Some([-1, -2]),
        _ => None,
    }
}

/// An emitter template.
#[derive(Debug)]
pub struct EmitterTemplate {
    pub name: String,
    /// The Required module's Material (a DrawRef).
    pub material: Arc<str>,
    /// A mesh emitter's mesh: the mesh kind's entry path.
    pub mesh: Option<Arc<str>>,
    pub local: bool,
    /// Required EmitterOrigin.
    pub origin: V3,
    pub duration: f64,
    pub duration_low: f64,
    pub duration_range: bool,
    pub loops: f64,
    pub delay: f64,
    pub delay_low: f64,
    pub delay_range: bool,
    pub delay_first_loop_only: bool,
    pub recalc_each_loop: bool,
    pub legacy_time: bool,
    pub legacy_spawn: bool,
    pub kill_on_deactivate: bool,
    pub kill_on_completed: bool,
    pub max_rate: f64,
    pub bursts: Vec<Burst>,
    pub burst_scale: Dist,
    pub rate: Dist,
    pub rate_scale: Dist,
    pub spawn_events: Vec<EventSpec>,
    pub death_events: Vec<EventSpec>,
    /// The EventGenerator's EPET_Collision entries (the Collision module reports them).
    pub collision_events: Vec<EventSpec>,
    pub receive: Vec<Receiver>,
    pub align: Align,
    pub sub_h: f64,
    pub sub_v: f64,
    pub flip: Flip,
    pub interp: SubUvMethod,
    pub random_changes: f64,
    pub random_time: Option<f64>,
    pub sort: SortMode,
    pub max_draw: f64,
    /// Every module but Required, Spawn, SpawnPerUnit and TrailSource, in order.
    pub mods: Vec<Module>,
    /// The indices in `mods` of the ones the per-tick update runs.
    pub upd_mods: Vec<usize>,
    /// The indices in `mods` of the SourceMovement modules (the FinalUpdate pass).
    pub final_mods: Vec<usize>,
    pub lock: Option<[i32; 2]>,
    pub rotate_axis: i32,
    pub rotate_z: bool,
    pub spu: Option<SpawnPerUnit>,
    pub kind: Kind,
    pub mesh_extent: V3,
    pub cam_facing: bool,
    pub cam_facing_no_up: bool,
    /// A mesh emitter's PSA_Velocity (1) / PSA_AwayFromCenter (2) turn.
    pub mesh_align: u8,
    pub pivot: [f64; 2],
    pub ribbon: Option<RibbonSpec>,
    /// TypeDataBeam2 and its beam modules.
    pub beam: Option<BeamSpec>,
    /// The first Lifetime module's Lifetime.
    pub lifetime: Option<Dist>,
    /// The Orbit module's ChainMode ends in Scale (Some), or Add / Link (Some(false)); None without an Orbit module.
    pub orbit_scale: Option<bool>,
    /// A ribbon's particle TrailSource.
    pub trail_source: Option<TrailSource>,
}

impl EmitterTemplate {
    /// The template of an emitter; `mesh` is a mesh emitter's (mesh kind path, ExtendedBounds BoxExtent).
    pub fn new(e: &EmitterData, mesh: Option<(Arc<str>, V3)>) -> EmitterTemplate {
        let find = |ty: &str| e.modules.iter().find(|m| m.get("type").and_then(Value::as_str) == Some(ty));
        let null = Value::Null;
        let rb = find("TypeDataRibbon");
        let mut beam = beam_spec(e);
        let ribbon = rb.map(|rb| RibbonSpec {
            trails: mvs_math::js::max(num_or(rb, "MaxTrailCount", 1.0), 1.0) as usize,
            max: num_or(rb, "MaxParticleInTrailCount", 500.0),
            tiling: num_or(rb, "TilingDistance", 0.0),
            dead_on_deactivate: flag_or(rb, "bDeadTrailsOnDeactivate", true),
            dead_on_loss: flag_or(rb, "bDeadTrailsOnSourceLoss", true),
            every_frame: truthy(rb, "bTangentRecalculationEveryFrame"),
            prev_tangent: flag_or(rb, "bEnablePreviousTangentRecalculation", true)
                && !truthy(rb, "bTangentRecalculationEveryFrame"),
            initial: truthy(rb, "bSpawnInitialParticle"),
            source_up: str_or(rb, "RenderAxis", "").ends_with("SourceUp"),
            camera_up: prop(rb, "RenderAxis").is_none() || str_or(rb, "RenderAxis", "").ends_with("CameraUp"),
            step: num_or(rb, "DistanceTessellationStepSize", 15.0),
            tan_scale: num_or(rb, "TangentTessellationScalar", 5.0),
            tan_diff: truthy(rb, "bEnableTangentDiffInterpScale"),
        });
        let trail_source = find("TrailSource")
            .filter(|ts| ribbon.is_some() && str_or(ts, "SourceMethod", "PET2SRCM_Default") == "PET2SRCM_Particle")
            .map(|ts| TrailSource {
                name: str_or(ts, "SourceName", ""),
                random: prop(ts, "SelectionMethod").and_then(Value::as_str) == Some("EPSSM_Random"),
            });
        let req = find("Required").unwrap_or(&null);
        let spawn = find("Spawn").unwrap_or(&null);
        let lifetime = find("Lifetime").map(|m| dist(m, "Lifetime"));
        if let Some(b) = beam.as_mut()
            && (truthy(req, "bUseLocalSpace")
                || prop(spawn, "BurstList").and_then(Value::as_array).is_some_and(|l| !l.is_empty()))
        {
            b.gaps.push("local space or a burst list".into());
        }
        if let Some(b) = beam.as_mut()
            && e.modules.iter().any(|m| m.get("type").and_then(Value::as_str) == Some("SourceMovement"))
        {
            // (tick_beam has no FinalUpdate pass)
            b.gaps.push("SourceMovement".into());
        }
        let bursts = prop(spawn, "BurstList").and_then(Value::as_array).map_or_else(Vec::new, |l| {
            l.iter()
                .map(|b| Burst {
                    count: num_or(b, "Count", f64::NAN),
                    count_low: num_or(b, "CountLow", f64::NAN),
                    time: num_or(b, "Time", f64::NAN),
                })
                .collect()
        });
        let rate = dist(spawn, "Rate");
        let rate_scale = dist(spawn, "RateScale");
        let max_rate = rate.table_max() * rate_scale.table_max();
        let mut spawn_events = Vec::new();
        let mut death_events = Vec::new();
        let mut collision_events = Vec::new();
        let mut receive = Vec::new();
        for m in &e.modules {
            match m.get("type").and_then(Value::as_str) {
                Some("EventGenerator") => {
                    for ev in prop(m, "Events").and_then(Value::as_array).into_iter().flatten() {
                        let spec =
                            EventSpec { name: str_or(ev, "CustomName", "").into(), freq: num_or(ev, "Frequency", 0.0) };
                        match prop(ev, "Type").and_then(Value::as_str) {
                            Some("EPET_Spawn") => spawn_events.push(spec),
                            Some("EPET_Death") => death_events.push(spec),
                            // (EPET_Collision, reported by the Collision module; any other type is taken as one)
                            _ => collision_events.push(spec),
                        }
                    }
                }
                Some("EventReceiverSpawn") => {
                    let gt = str_or(m, "EventGeneratorType", "EPET_Any");
                    let ty = match gt.as_str() {
                        "EPET_Spawn" => Some(EventType::Spawn),
                        "EPET_Death" => Some(EventType::Death),
                        "EPET_Collision" => Some(EventType::Collision),
                        "EPET_Any" => None,
                        _ => continue,
                    };
                    receive.push(Receiver {
                        name: str_or(m, "EventName", ""),
                        ty,
                        count: dist(m, "SpawnCount"),
                        psys_loc: truthy(m, "bUsePSysLocation"),
                        inherit: truthy(m, "bInheritVelocity"),
                        inherit_scale: dist(m, "InheritVelocityScale"),
                    });
                }
                _ => {}
            }
        }
        let is_mesh = mesh.is_some();
        // Sprites (UpdateRenderThreadResources 142c64010, fill 142c50180): PSA_Velocity takes the velocity tangents;
        // every other alignment without an axis lock is camera facing, and the fill squares the size (Y = X) for
        // FacingCameraPosition, Square and FacingCameraDistanceBlend only.
        let sa = str_or(req, "ScreenAlignment", "PSA_Square");
        let align = match sa.as_str() {
            "PSA_Velocity" => Align::Velocity,
            "PSA_Rectangle" | "PSA_AwayFromCenter" | "PSA_TypeSpecific" => Align::Rect,
            _ => Align::Square,
        };
        let flip = match str_or(req, "UVFlippingMode", "None").replace("EParticleUVFlipMode::", "").as_str() {
            "FlipUV" => Flip::FlipUv,
            "RandomFlipUV" => Flip::RandomFlipUv,
            "FlipUOnly" => Flip::FlipU,
            "RandomFlipUOnly" => Flip::RandomFlipU,
            "FlipVOnly" => Flip::FlipV,
            "RandomFlipVOnly" => Flip::RandomFlipV,
            "RandomFlipUVIndependent" => Flip::RandomFlipUvIndependent,
            _ => Flip::None,
        };
        let interp = match str_or(req, "InterpolationMethod", "PSUVIM_None").as_str() {
            "PSUVIM_None" => SubUvMethod::None,
            "PSUVIM_Linear" => SubUvMethod::Linear,
            "PSUVIM_Linear_Blend" => SubUvMethod::LinearBlend,
            "PSUVIM_Random" => SubUvMethod::Random,
            "PSUVIM_Random_Blend" => SubUvMethod::RandomBlend,
            _ => SubUvMethod::Other,
        };
        let sort = match str_or(req, "SortMode", "PSORTMODE_None").as_str() {
            "PSORTMODE_ViewProjDepth" => SortMode::ViewProjDepth,
            "PSORTMODE_DistanceToView" => SortMode::DistanceToView,
            "PSORTMODE_Age_OldestFirst" => SortMode::AgeOldestFirst,
            "PSORTMODE_Age_NewestFirst" => SortMode::AgeNewestFirst,
            _ => SortMode::None,
        };
        let spu = find("SpawnPerUnit").map(|s| SpawnPerUnit {
            value: dist(s, "SpawnPerUnit"),
            unit: num_or(s, "UnitScalar", 50.0),
            tol: num_or(s, "MovementTolerance", 0.1),
            max_frame: num_or(s, "MaxFrameDistance", 0.0),
            ignore_rate: truthy(s, "bIgnoreSpawnRateWhenMoving"),
            process_rate: flag_or(s, "bProcessSpawnRate", true),
            process_bursts: flag_or(s, "bProcessBurstList", true),
            keep: [
                !truthy(s, "bIgnoreMovementAlongX"),
                !truthy(s, "bIgnoreMovementAlongY"),
                !truthy(s, "bIgnoreMovementAlongZ"),
            ]
            .map(|k| f64::from(u8::from(k))),
        });
        let kept: Vec<&Value> = e
            .modules
            .iter()
            .filter(|m| {
                !matches!(
                    m.get("type").and_then(Value::as_str),
                    Some("Required" | "Spawn" | "SpawnPerUnit" | "TrailSource")
                )
            })
            .collect();
        let mods: Vec<Module> = kept.iter().map(|m| module_of(m)).collect();
        let type_of = |m: &Value| m.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        let upd_mods = (0..kept.len()).filter(|&i| UPDATE_TYPES.contains(&type_of(kept[i]).as_str())).collect();
        let final_mods = (0..kept.len()).filter(|&i| type_of(kept[i]) == "SourceMovement").collect();
        let orbit_scale = kept
            .iter()
            .find(|m| type_of(m) == "Orbit")
            .map(|m| str_or(m, "ChainMode", "EOChainMode_Add").ends_with("Scale"));
        let lock_flag = kept
            .iter()
            .find(|m| type_of(m) == "OrientationAxisLock")
            .map_or_else(|| "EPAL_NONE".to_string(), |m| str_or(m, "LockAxisFlags", "EPAL_NONE"));
        let lock = axis_lock(&lock_flag);
        let rotate_axis = match lock_flag.as_str() {
            "EPAL_ROTATE_X" => 1,
            "EPAL_ROTATE_Y" => 2,
            "EPAL_ROTATE_Z" => 3,
            _ => 0,
        };
        let mut pivot = [-0.5, -0.5];
        for m in kept.iter().filter(|m| type_of(m) == "PivotOffset") {
            let p = xyz_or(m, "PivotOffset", 0.0);
            pivot = [pivot[0] + p[0], pivot[1] + p[1]];
        }
        let td = find("TypeDataMesh");
        let cam_facing = td.is_some_and(|t| truthy(t, "bCameraFacing"));
        let facing =
            td.map_or_else(|| "XAxisFacing_NoUp".to_string(), |t| str_or(t, "CameraFacingOption", "XAxisFacing_NoUp"));
        let kind = if is_mesh {
            Kind::Mesh
        } else if ribbon.is_some() {
            Kind::Ribbon
        } else if beam.is_some() {
            Kind::Beam
        } else {
            Kind::Sprite
        };
        let (mesh, mesh_extent) = match mesh {
            Some((p, x)) => (Some(p), x),
            None => (None, [0.0; 3]),
        };
        EmitterTemplate {
            name: e.name.clone(),
            material: Arc::from(str_or(req, "Material", "").as_str()),
            mesh,
            local: truthy(req, "bUseLocalSpace"),
            origin: xyz_or(req, "EmitterOrigin", 0.0),
            duration: num_or(req, "EmitterDuration", 1.0),
            duration_low: num_or(req, "EmitterDurationLow", 0.0),
            duration_range: truthy(req, "bEmitterDurationUseRange"),
            loops: num_or(req, "EmitterLoops", 0.0),
            delay: num_or(req, "EmitterDelay", 0.0),
            delay_low: num_or(req, "EmitterDelayLow", 0.0),
            delay_range: truthy(req, "bEmitterDelayUseRange"),
            delay_first_loop_only: truthy(req, "bDelayFirstLoopOnly"),
            recalc_each_loop: truthy(req, "bDurationRecalcEachLoop"),
            legacy_time: flag_or(req, "bUseLegacyEmitterTime", true),
            legacy_spawn: e.legacy_spawning.unwrap_or(false),
            kill_on_deactivate: truthy(req, "bKillOnDeactivate"),
            kill_on_completed: truthy(req, "bKillOnCompleted"),
            max_rate,
            bursts,
            burst_scale: dist(spawn, "BurstScale"),
            rate,
            rate_scale,
            spawn_events,
            death_events,
            collision_events,
            receive,
            align,
            sub_h: num_or(req, "SubImages_Horizontal", 1.0),
            sub_v: num_or(req, "SubImages_Vertical", 1.0),
            flip,
            interp,
            random_changes: num_or(req, "RandomImageChanges", 0.0),
            random_time: prop(req, "RandomImageTime").map(crate::data::number_value),
            sort,
            max_draw: if flag_or(req, "bUseMaxDrawCount", true) { num_or(req, "MaxDrawCount", 500.0) } else { -1.0 },
            mods,
            upd_mods,
            final_mods,
            lock,
            rotate_axis,
            rotate_z: lock_flag == "EPAL_ROTATE_Z",
            spu,
            kind,
            mesh_extent,
            cam_facing,
            cam_facing_no_up: cam_facing && facing == "XAxisFacing_NoUp",
            mesh_align: if !is_mesh {
                0
            } else if sa == "PSA_Velocity" {
                1
            } else if sa == "PSA_AwayFromCenter" {
                2
            } else {
                0
            },
            pivot,
            ribbon,
            beam,
            lifetime,
            orbit_scale,
            trail_source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every UNPORTED_MODULES type is an unported! arm of module_of (none is left), and the ported module types are
    /// not listed.
    #[test]
    fn unported_list_matches_module_of() {
        for ty in UNPORTED_MODULES {
            let m = serde_json::json!({ "type": ty });
            assert!(std::panic::catch_unwind(|| module_of(&m)).is_err(), "{ty} is ported");
        }
        for ty in [
            "SubUVMovie",
            "LocationDirect",
            "MeshRotationRateOverLife",
            "SubUV",
            "Size_Seeded",
            "Color_Seeded",
            "Rotation_Seeded",
            "RotationRate_Seeded",
            "VelocityCone",
            "LocationPrimitiveCylinder_Seeded",
            "RotationOverLifetime",
            "SizeScale",
            "SizeScaleBySpeed",
            "MeshRotationRateMultiplyLife",
            "Collision",
            "AccelerationConstant",
            "TypeDataBeam2",
            "BeamSource",
            "BeamTarget",
            "BeamNoise",
            "BeamModifier",
        ] {
            assert!(!UNPORTED_MODULES.contains(&ty));
            let m = serde_json::json!({ "type": ty });
            let _ = module_of(&m);
        }
    }
}
