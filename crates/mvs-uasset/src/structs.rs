//! Native structs: the struct types CUE4Parse reads by a native serializer instead of as tagged properties, each as
//! a struct of its member names (`{ X, Y, Z }`).

use crate::archive::{ReadError, Result};
use crate::objects::ExportArchive;
use crate::props::{ReadType, read_soft_object_path};
use crate::value::{Props, Value};
use crate::versions;

/// Builds a struct of named values.
macro_rules! st {
    ($($k:literal => $v:expr),* $(,)?) => {
        Value::Struct(Props::from([$(($k.to_owned(), $v)),*]))
    };
}

fn f(ar: &mut ExportArchive<'_>) -> Result<Value> {
    Ok(Value::Float(ar.f32()?))
}

fn int(v: impl Into<i64>) -> Value {
    Value::Int(v.into())
}

fn vec3(ar: &mut ExportArchive<'_>) -> Result<Value> {
    Ok(st! { "X" => f(ar)?, "Y" => f(ar)?, "Z" => f(ar)? })
}

fn vec2(ar: &mut ExportArchive<'_>) -> Result<Value> {
    Ok(st! { "X" => f(ar)?, "Y" => f(ar)? })
}

/// A C# enum as Newtonsoft's StringEnumConverter writes it: its name, or the number when the value has none.
pub fn enum_text(names: &[&str], v: u8) -> Value {
    match names.get(usize::from(v)) {
        Some(n) => Value::Str((*n).to_owned()),
        None => int(v),
    }
}

pub const RCIM: &[&str] = &["RCIM_Linear", "RCIM_Constant", "RCIM_Cubic", "RCIM_None"];
pub const RCTM: &[&str] = &["RCTM_Auto", "RCTM_User", "RCTM_Break", "RCTM_None"];
pub const RCTWM: &[&str] = &["RCTWM_WeightedNone", "RCTWM_WeightedArrive", "RCTWM_WeightedLeave", "RCTWM_WeightedBoth"];
const EXTRAP: &[&str] =
    &["RCCE_Cycle", "RCCE_CycleWithOffset", "RCCE_Oscillate", "RCCE_Linear", "RCCE_Constant", "RCCE_None"];
const SEQUENCER: crate::archive::Guid =
    crate::archive::Guid::from_parts(0x7B5AE74C, 0xD2704C10, 0xA9585798, 0x0B212A5A);

/// FColor.Hex: RRGGBB, or AARRGGBB when alpha is neither 0 nor 255.
pub fn color_hex(r: u8, g: u8, b: u8, a: u8) -> String {
    if a != 0 && a != 255 { format!("{a:02X}{r:02X}{g:02X}{b:02X}") } else { format!("{r:02X}{g:02X}{b:02X}") }
}

/// FLinearColor's sRGB conversion in float32 (CUE4Parse's ToFColor(sRGB: true)).
fn srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        // Math.pow on float32 operands, its result rounded to float32
        let p = f64::from(v).powf(f64::from(5.0f32 / 12.0)) as f32;
        p * 1.055 - 0.055
    }
}

/// FLinearColor.Hex: ToFColor(sRGB: true).Hex.
pub fn linear_color_hex(r: f32, g: f32, b: f32, a: f32) -> String {
    // a NaN passes the clamp as itself and floors to 0, as `Math.floor(NaN) & 0xff`
    let clamp01 = |v: f32| v.clamp(0.0, 1.0);
    let q = |v: f32| {
        let x = (v * 255.999).floor();
        if x.is_nan() { 0 } else { (x as i64 & 0xff) as u8 }
    };
    color_hex(q(srgb(clamp01(r))), q(srgb(clamp01(g))), q(srgb(clamp01(b))), q(clamp01(a)))
}

pub fn read_linear_color(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let (r, g, b, a) = (ar.f32()?, ar.f32()?, ar.f32()?, ar.f32()?);
    let hex = Value::Str(linear_color_hex(r, g, b, a));
    Ok(
        st! { "R" => Value::Float(r), "G" => Value::Float(g), "B" => Value::Float(b), "A" => Value::Float(a), "Hex" => hex },
    )
}

pub fn read_color(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let (b, g, r, a) = (ar.u8()?, ar.u8()?, ar.u8()?, ar.u8()?);
    Ok(st! { "B" => int(b), "G" => int(g), "R" => int(r), "A" => int(a), "Hex" => Value::Str(color_hex(r, g, b, a)) })
}

pub fn read_quat(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let (x, y, z, w) = (ar.f32()?, ar.f32()?, ar.f32()?, ar.f32()?);
    let sq = x * x + y * y + z * z + w * w;
    Ok(st! {
        "X" => Value::Float(x),
        "Y" => Value::Float(y),
        "Z" => Value::Float(z),
        "W" => Value::Float(w),
        "IsNormalized" => Value::Bool((1.0 - sq).abs() < 0.01),
        "Size" => Value::Float(sq.sqrt()),
        "SizeSquared" => Value::Float(sq),
    })
}

fn per_platform(ar: &mut ExportArchive<'_>, read: fn(&mut ExportArchive<'_>) -> Result<Value>) -> Result<Value> {
    let cooked = ar.bool()?;
    let default = read(ar)?;
    let mut per = Value::Null;
    if !cooked && ar.filter_editor_only() {
        let n = ar.i32()?;
        let mut m = Props::new();
        for _ in 0..n {
            let k = ar.fname()?;
            m.insert(k, read(ar)?);
        }
        per = Value::Struct(m);
    }
    Ok(st! { "bCooked" => Value::Bool(cooked), "Default" => default.clone(), "PerPlatform" => per, "Value" => default })
}

fn expression_input(ar: &mut ExportArchive<'_>) -> Result<Props> {
    let expression = if ar.filter_editor_only() { Value::Null } else { Value::Ref(ar.index()?) };
    let mut o = Props::new();
    o.insert("Expression".into(), expression);
    o.insert("OutputIndex".into(), int(ar.i32()?));
    o.insert("InputName".into(), Value::Str(ar.fname()?));
    for k in ["Mask", "MaskR", "MaskG", "MaskB", "MaskA"] {
        o.insert(k.into(), int(ar.i32()?));
    }
    let name = if ar.filter_editor_only() { ar.fname()? } else { "None".to_owned() };
    o.insert("ExpressionName".into(), Value::Str(name));
    Ok(o)
}

fn material_input(ar: &mut ExportArchive<'_>, constant: fn(&mut ExportArchive<'_>) -> Result<Value>) -> Result<Value> {
    let mut o = expression_input(ar)?;
    o.insert("UseConstant".into(), Value::Bool(ar.bool()?));
    o.insert("Constant".into(), constant(ar)?);
    Ok(Value::Struct(o))
}

fn weighted_sampler(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let n = ar.i32()?;
    let mut prob = Vec::new();
    for _ in 0..n {
        prob.push(f(ar)?);
    }
    let m = ar.i32()?;
    let mut alias = Vec::new();
    for _ in 0..m {
        alias.push(int(ar.i32()?));
    }
    Ok(st! { "Prob" => Value::Array(prob), "Alias" => Value::Array(alias), "TotalWeight" => f(ar)? })
}

/// TRange<FFrameNumber> read raw: per bound a byte ERangeBoundTypes (written as a number) and an int.
fn read_range(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let bound = |ar: &mut ExportArchive<'_>| -> Result<Value> {
        Ok(st! { "Type" => int(ar.u8()?), "Value" => st! { "Value" => int(ar.i32()?) } })
    };
    Ok(st! { "LowerBound" => bound(ar)?, "UpperBound" => bound(ar)? })
}

/// FMovieSceneEvaluationTreeNode, read raw (Pack 1: the range, the parent handle and index, two entry handles).
fn read_tree_node(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let range = read_range(ar)?;
    let parent = st! { "ChildrenHandle" => st! { "EntryIndex" => int(ar.i32()?) }, "Index" => int(ar.i32()?) };
    Ok(st! {
        "Range" => range,
        "Parent" => parent,
        "ChildrenID" => st! { "EntryIndex" => int(ar.i32()?) },
        "DataID" => st! { "EntryIndex" => int(ar.i32()?) },
    })
}

/// TEvaluationTreeEntryContainer: the FEntry array, then the items.
fn read_entry_container(
    ar: &mut ExportArchive<'_>,
    item: fn(&mut ExportArchive<'_>) -> Result<Value>,
) -> Result<Value> {
    let mut entries = Vec::new();
    for _ in 0..ar.i32()? {
        entries.push(st! { "StartIndex" => int(ar.i32()?), "Size" => int(ar.i32()?), "Capacity" => int(ar.i32()?) });
    }
    let mut items = Vec::new();
    for _ in 0..ar.i32()? {
        items.push(item(ar)?);
    }
    Ok(st! { "Entries" => Value::Array(entries), "Items" => Value::Array(items) })
}

/// FMovieSceneChannel<float>: FSequencerObjectVersion 12 >= SerializeFloatChannelCompletely, so the whole channel is
/// read; bShowCurve needs FFortniteMainBranchObjectVersion >= 53 (4.26: 43). Values are FMovieSceneValue<float> in the
/// full layout (tangent with 3 bytes padding, 2 after the modes).
fn read_float_channel(ar: &mut ExportArchive<'_>) -> Result<Value> {
    if ar.custom(SEQUENCER) < 12 && ar.custom(versions::FORTNITE_MAIN) < 53 {
        return Err(ReadError::not_ported("MovieSceneFloatChannel before SerializeFloatChannelCompletely not ported"));
    }
    let pre = enum_text(EXTRAP, ar.u8()?);
    let post = enum_text(EXTRAP, ar.u8()?);
    ar.i32()?; // serialised element size
    let mut times = Vec::new();
    for _ in 0..ar.i32()? {
        times.push(st! { "Value" => int(ar.i32()?) });
    }
    ar.i32()?;
    let mut values = Vec::new();
    for _ in 0..ar.i32()? {
        let value = f(ar)?;
        let tangent = st! {
            "ArriveTangent" => f(ar)?,
            "LeaveTangent" => f(ar)?,
            "ArriveTangentWeight" => f(ar)?,
            "LeaveTangentWeight" => f(ar)?,
            "TangentWeightMode" => int(ar.u8()?),
        };
        ar.skip(3)?;
        let interp = int(ar.u8()?);
        let tangent_mode = int(ar.u8()?);
        ar.skip(2)?;
        values.push(
            st! { "Value" => value, "Tangent" => tangent, "InterpMode" => interp, "TangentMode" => tangent_mode },
        );
    }
    let default = f(ar)?;
    let has_default = Value::Bool(ar.bool()?);
    let tick = st! { "Numerator" => int(ar.i32()?), "Denominator" => int(ar.i32()?) };
    let show = ar.custom(versions::FORTNITE_MAIN) >= 53 && ar.bool()?;
    Ok(st! {
        "PreInfinityExtrap" => pre,
        "PostInfinityExtrap" => post,
        "Times" => Value::Array(times),
        "Values" => Value::Array(values),
        "DefaultValue" => default,
        "bHasDefaultValue" => has_default,
        "TickResolution" => tick,
        "bShowCurve" => Value::Bool(show),
        "StructFallback" => Value::Null,
    })
}

/// Reads a native struct of type `struct_type`; `None` (nothing read) when CUE4Parse reads that type as tagged
/// properties.
pub fn read_native_struct(ar: &mut ExportArchive<'_>, struct_type: &str, rt: ReadType) -> Result<Option<Value>> {
    let _ = rt;
    Ok(Some(match struct_type {
        "Vector"
        | "Vector_NetQuantize"
        | "Vector_NetQuantize10"
        | "Vector_NetQuantize100"
        | "Vector_NetQuantizeNormal" => vec3(ar)?,
        "Vector2D" => vec2(ar)?,
        "Vector4" => st! { "X" => f(ar)?, "Y" => f(ar)?, "Z" => f(ar)?, "W" => f(ar)? },
        "Rotator" => st! { "Pitch" => f(ar)?, "Yaw" => f(ar)?, "Roll" => f(ar)? },
        "Quat" => read_quat(ar)?,
        "LinearColor" => read_linear_color(ar)?,
        "Color" => read_color(ar)?,
        "Guid" => Value::Str(ar.guid()?.to_string()),
        "IntPoint" => st! { "X" => int(ar.i32()?), "Y" => int(ar.i32()?) },
        "IntVector" => st! { "X" => int(ar.i32()?), "Y" => int(ar.i32()?), "Z" => int(ar.i32()?) },
        "Box" => st! { "Min" => vec3(ar)?, "Max" => vec3(ar)?, "IsValid" => int(ar.u8()?) },
        "Box2D" => st! { "Min" => vec2(ar)?, "Max" => vec2(ar)?, "bIsValid" => int(ar.u8()?) },
        "Sphere" => st! { "Center" => vec3(ar)?, "W" => f(ar)? },
        "Plane" => {
            let v = vec3(ar)?;
            let w = f(ar)?;
            let c = |k: &str| v.as_struct().and_then(|s| s.get(k)).cloned().unwrap_or(Value::Null);
            let (x, y, z) = (c("X"), c("Y"), c("Z"));
            st! { "Vector" => v, "W" => w, "X" => x, "Y" => y, "Z" => z }
        }
        "TwoVectors" => st! { "V1" => vec3(ar)?, "V2" => vec3(ar)? },
        "Matrix" => {
            let mut o = Props::new();
            for r in 0..4 {
                for c in 0..4 {
                    o.insert(format!("M{r}{c}"), f(ar)?);
                }
            }
            Value::Struct(o)
        }
        "FrameNumber" => st! { "Value" => int(ar.i32()?) },
        "MovieSceneFloatChannel" => read_float_channel(ar)?,
        // TMovieSceneEvaluationTree<FEntityAndMetaDataIndex>: the base tree, then the data container
        "MovieSceneEvaluationFieldEntityTree" => {
            let root = read_tree_node(ar)?;
            let children = read_entry_container(ar, read_tree_node)?;
            let data = read_entry_container(ar, |ar| {
                Ok(st! { "EntityIndex" => int(ar.i32()?), "MetaDataIndex" => int(ar.i32()?) })
            })?;
            st! { "SerializedData" => st! { "Data" => data, "RootNode" => root, "ChildNodes" => children } }
        }
        "MovieSceneFrameRange" => st! { "Value" => read_range(ar)? },
        "DateTime" | "Timespan" => st! { "Ticks" => int(ar.i64()?) },
        "SoftObjectPath" | "SoftClassPath" | "StringAssetReference" | "StringClassReference" => {
            read_soft_object_path(ar)?
        }
        "GameplayTagContainer" => {
            let n = ar.i32()?;
            let mut tags = Vec::new();
            for _ in 0..n {
                tags.push(Value::Str(ar.fname()?));
            }
            st! { "GameplayTags" => Value::Array(tags) }
        }
        "PerPlatformFloat" => per_platform(ar, f)?,
        "PerPlatformInt" => per_platform(ar, |ar| Ok(int(ar.i32()?)))?,
        "PerPlatformBool" => per_platform(ar, |ar| Ok(Value::Bool(ar.bool()?)))?,
        "RichCurveKey" => st! {
            "InterpMode" => enum_text(RCIM, ar.u8()?),
            "TangentMode" => enum_text(RCTM, ar.u8()?),
            "TangentWeightMode" => enum_text(RCTWM, ar.u8()?),
            "Time" => f(ar)?,
            "Value" => f(ar)?,
            "ArriveTangent" => f(ar)?,
            "ArriveTangentWeight" => f(ar)?,
            "LeaveTangent" => f(ar)?,
            "LeaveTangentWeight" => f(ar)?,
        },
        "SimpleCurveKey" => st! { "Time" => f(ar)?, "Value" => f(ar)? },
        "SmartName" => Value::Str(ar.fname()?),
        "NavAgentSelector" => st! { "PackedBits" => int(ar.u32()?) },
        // FExpressionInput (FCoreObjectVersion >= MaterialInputNativeSerialize): a cooked (FilterEditorOnly) package
        // has no Expression index; FMaterialInput<T> adds UseConstant and Constant. ColorMaterialInput is FColor
        // before FFortniteMainBranchObjectVersion MaterialInputUsesLinearColor (171; 4.26 has 43).
        "ExpressionInput" | "MaterialAttributesInput" => Value::Struct(expression_input(ar)?),
        "ColorMaterialInput" => material_input(ar, read_color)?,
        "ScalarMaterialInput" => material_input(ar, f)?,
        "VectorMaterialInput" => material_input(ar, vec3)?,
        "Vector2MaterialInput" => material_input(ar, vec2)?,
        "SkeletalMeshSamplingLODBuiltData" => st! { "AreaWeightedTriangleSampler" => weighted_sampler(ar)? },
        // FFontData: FEditorObjectVersion 40 >= AddedFontFaceAssets (9); FFontDataConverter's fields
        "FontData" => {
            if !ar.bool()? {
                st! { "Hinting" => int(0), "LoadingPolicy" => int(0), "SubFaceIndex" => int(0) }
            } else {
                let i = ar.i32()?;
                let mut o = Props::new();
                o.insert("LocalFontFaceAsset".into(), Value::Ref(ar.cache.try_resolve(ar.pkg, i)?));
                if i == 0 {
                    // the converter writes LocalFontFaceAsset (null) and not these when the index was read
                    ar.fstring()?;
                    ar.u8()?;
                    ar.u8()?;
                }
                o.insert("SubFaceIndex".into(), int(ar.i32()?)); // GAME_UE4_20 and later
                Value::Struct(o)
            }
        }
        // FFontCharacter: UE3 version 864 >= Release122 and FONT_FORMAT_AND_UV_TILING_CHANGES (506)
        "FontCharacter" => st! {
            "StartU" => int(ar.i32()?),
            "StartV" => int(ar.i32()?),
            "USize" => int(ar.i32()?),
            "VSize" => int(ar.i32()?),
            "TextureIndex" => int(ar.u8()?),
            "VerticalOffset" => int(ar.i32()?),
        },
        // an empty FStructFallback that reads nothing (its tagged properties stay in the stream, past by the tag size)
        "KeyHandleMap" => Value::Struct(Props::new()),
        t if NOT_PORTED_STRUCTS.contains(&t) => {
            return Err(ReadError::not_ported(format!("native struct {t} not ported")));
        }
        _ => return Ok(None),
    }))
}

/// The struct names FScriptStruct (CUE4Parse 1.2.2) reads with its own code for a GAME_UE4_26 package (cases without
/// an Ar.Game gate, including those it reads raw) that this file does not port: met in a package, they are reported,
/// never read as tagged properties. PCGPoint, ActorReference and MovieSceneTangentData fall back to tagged properties
/// for 4.26 and are left out; generated from the decompile's switch.
static NOT_PORTED_STRUCTS: &[&str] = &[
    "Box2f",
    "Box3f",
    "CapsuleShape",
    "CompressedRichCurve",
    "NameCurveKey",
    "StringCurveKey",
    "SkeletalMeshSamplingRegionBuiltData",
    "PerPlatformFrameRate",
    "PerPlatformFString",
    "PerPlatformERigLogicFloatingPointType",
    "PerPlatformERigLogicCalculationType",
    "PerQualityLevelInt",
    "PerQualityLevelFloat",
    "PannerDetails",
    "Int32Point",
    "IntVector2",
    "Int32Vector2",
    "UintVector2",
    "Uint32Point",
    "UintVector",
    "IntVector4",
    "UintVector4",
    "Uint32Vector4",
    "Int64Vector2",
    "Int64Point",
    "UInt64Vector2",
    "UInt64Point",
    "Int64Vector",
    "UInt64Vector",
    "Int64Vector4",
    "UInt64Vector4",
    "LevelSequenceObjectReferenceMap",
    "NiagaraVariable",
    "NiagaraVariableBase",
    "NiagaraDataChannelVariable",
    "NiagaraVariableWithOffset",
    "NiagaraDataInterfaceGPUParamInfo",
    "MaterialOverrideNanite",
    "MaterialLayersFunctionsTree",
    "MovieSceneEvalTemplatePtr",
    "MovieSceneEvaluationKey",
    "MovieSceneEventParameters",
    "MovieSceneDoubleChannel",
    "MovieSceneFloatValue",
    "MovieSceneDoubleValue",
    "MovieSceneSegment",
    "MovieSceneSegmentIdentifier",
    "MovieSceneSequenceID",
    "MovieSceneSequenceInstanceDataPtr",
    "MovieSceneSubSequenceTree",
    "MovieSceneSubSectionFieldData",
    "MovieSceneTimeWarpVariant",
    "MovieSceneTrackFieldData",
    "MovieSceneTrackIdentifier",
    "MovieSceneTrackImplementationPtr",
    "MidiEvent",
    "Plane4f",
    "Plane4d",
    "Quat4f",
    "Quat4d",
    "Rotator3f",
    "Rotator3d",
    "RawAnimSequenceTrack",
    "Spline",
    "Sphere3f",
    "Sphere3d",
    "SectionEvaluationDataTree",
    "Transform3f",
    "TypedParameter",
    "UniqueNetIdRepl",
    "Vector2f",
    "DeprecateSlateVector2D",
    "Vector3f",
    "Vector3d",
    "Vector4f",
    "Vector4d",
    "ClothLODDataCommon",
    "ClothLODData",
    "ClothTetherData",
    "Matrix44f",
    "InstancedStruct",
    "InstancedStructContainer",
    "InstancedPropertyBag",
    "RigVMPropertyBag",
    "InstancedOverridablePropertyBag",
    "WorldConditionQueryDefinition",
    "UniversalObjectLocatorFragment",
    "ShaderValueTypeHandle",
    "AnimationAttributeIdentifier",
    "AttributeCurve",
    "PCGDataPtrWrapper",
    "PCGPointArray",
    "CacheEventTrack",
    "StateTreeInstanceData",
    "InstancedStructArray",
    "DataCacheDuplicatedObjectData",
    "EdGraphPinType",
    "ControlRigOverrideContainer",
    "UInt32Property",
    "ZeroSizeProperty",
    "ConnectivityCube",
    "FortActorRecord",
    "GameplayEventFunction",
    "GameplayEventDescriptor",
    "DistanceQuantity",
    "SpeedQuantity",
    "MassQuantity",
    "SwBitfield_TargetRotatorMask",
    "RsBitfield_NavPermissionDetailFlags",
    "RsBitfield_NavPermissionFlags",
    "RsBitfield_NavState",
    "RsBitfield_HeroLoadoutFlags",
    "RsBitfield_HeroBufferFlags",
    "RsBitfield_HeroInputFlags",
    "RsBitfield_HeroUpgradeFlags",
    "RsBitfield_RsIkBoneTypes",
    "RsBitfield_UINavigationInput",
    "RsBitfield_WorldMapLevelType",
    "RsBitfield_WorldMapLODLevel",
    "RsBitfield_WorldMapWidgetFilterType",
    "LegoGraphPartInstance",
    "Core1047ReleaseFlag",
    "TLJsonGuid",
    "TLJsonVector",
    "TLJsonVector2D",
    "SceneFaceDefSeamline",
    "VGCoverDataPoint",
    "VectorDouble",
    "SoundAttenuationPluginSettingsWithOverride",
    "TedInstancedStruct",
    "TedMarkerHandle",
    "FoWAgentHandle",
    "NiagaraUserParameterModifier",
    "VinInstancedStruct",
    "VinInstancedPropertyBag",
    "AnyValue",
    "EveryPlatformFloat",
    "EveryPlatformBool",
    "EveryPlatformInt",
    "HavokAIAnyArray",
    "HavokNavAnyArray",
    "SUDSValue",
    "PMTimelineSectionFloatMapping",
    "PMTimelineSectionEventFloatMapping",
    "PMFloatMapping",
    "PMBoundFloatMapping",
    "FWAbilityResourceFloatMapping",
    "GameplayEffectFloatMapping",
    "PMTimelineFloatMapping",
    "FWPerkContextFloatMapping",
    "PMTimelineObjectBindingDef",
    "GameplayEffectApplicationDataHandle",
    "PMTimelineRelevancy",
    "LSAAudioSectionkey",
    "FixedPoint",
    "TslSomeSKStruct",
    "PowerQuantity",
    "ForceQuantity",
    "TimeUnit",
    "PressureQuantity",
    "VolumeQuantity",
    "SOS_GDValue",
    "GG_CrowdGeneratorLocations",
    "VoyagePackedLocalTransform",
    "VoyageFloat16",
    "MercunaUsageSpec",
    "MercunaUsageTypes",
    "VAL_CharacterCustomizationVariantOptionsArray",
    "ParamRegistryInfo",
    "R5CollisionApproximation",
    "R5SoftAssetPath",
    "AnimMontageContainer",
    "NamedGuid",
    "RDialogueFactValue",
];
