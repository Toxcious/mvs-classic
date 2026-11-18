//! Custom versions of an unversioned package. The build's packages name none, so CUE4Parse's per-class Get() falls
//! back to its default for GAME_UE4_26 (each class's Get(FArchive), evaluated for that game).

use crate::archive::Guid;

/// The UE 4.26 defaults, by GUID.
pub const UE4_26_CUSTOM_VERSIONS: &[(Guid, i32)] = &[
    (Guid::from_parts(0xAF43A65D, 0x7FD34947, 0x98733E8E, 0xD9C1BB05), 15), // FAnimObjectVersion: GroomBindingSerialization
    (Guid::from_parts(0x29E575DD, 0xE0A34627, 0x9D10D276, 0x232CDCEA), 17), // FAnimPhysObjectVersion: GeometryCacheAssetDeprecation
    (Guid::from_parts(0xA7820CFB, 0x20A74359, 0x8C542C14, 0x9623CF50), 6), // FControlRigObjectVersion: BlueprintVariableSupport
    (Guid::from_parts(0x375EC13C, 0x06E448FB, 0xB50084F0, 0x262A717E), 4), // FCoreObjectVersion: FProperties
    (Guid::from_parts(0x174F1F0B, 0xB4C645A5, 0xB13F2EE8, 0xD0FB917D), 10), // FDestructionObjectVersion: ImplicitObjectDoCollideAttribute
    (Guid::from_parts(0xE4B068ED, 0xF49442E9, 0xA231DA0B, 0x2E46BB41), 40), // FEditorObjectVersion: SkeletalMeshSourceDataSupport16bitOfMaterialNumber
    (Guid::from_parts(0x430C4D19, 0x71544970, 0x87699B69, 0xDF90B0E5), 15), // FFoliageCustomVersion: FoliageDiscardOnLoad
    (Guid::from_parts(0x601D1886, 0xAC644F84, 0xAA16D3DE, 0x0DEAC7D6), 43), // FFortniteMainBranchObjectVersion: ChaosSolverPropertiesMoved
    (Guid::from_parts(0xE7086368, 0x6B234C58, 0x84391B70, 0x16265E91), 1), // FFortniteReleaseBranchCustomObjectVersion: DisableLevelset_v14_10
    (Guid::from_parts(0x5B4C06B7, 0x24634AF8, 0x805BBF70, 0xCDF5D0DD), 0), // FFortniteSeasonBranchObjectVersion: BeforeCustomVersionWasAdded
    (Guid::from_parts(0xCFFC743F, 0x43B04480, 0x939114DF, 0x171D2073), 37), // FFrameworkObjectVersion: StoringUCSSerializationIndex
    (Guid::from_parts(0x92738C43, 0x29884D9C, 0x9A3D9BBE, 0x6EFF9FC0), 0), // FInterchangeCustomVersion: BeforeCustomVersionWasAdded
    (Guid::from_parts(0xB02B49B5, 0xBB2044E9, 0xA30432B7, 0x52E40360), 3), // FMobileObjectVersion: StoreReflectionCaptureCompressedMobile
    (Guid::from_parts(0xFCF57AFA, 0x50764283, 0xB9A9E658, 0xFFA02D32), 66), // FNiagaraCustomVersion: SignificanceHandlers
    (Guid::from_parts(0x5426C227, 0x4B3145B2, 0x9B9BED1F, 0x327FB126), 0), // FOverridablePropertyBagCustomVersion: BeforeCustomVersionWasAdded
    (Guid::from_parts(0x2763920D, 0x0F784B39, 0x986E4BB3, 0xA88D666D), 0), // FPCGCustomVersion: BeforeCustomVersionWasAdded
    (Guid::from_parts(0x78F01B33, 0xEBEA4F98, 0xB9B484EA, 0xCCB95AA2), 14), // FPhysicsObjectVersion: ChaosConvexHasUniqueEdgeSet
    (Guid::from_parts(0x134A157E, 0xD5E249A3, 0x8D4E843C, 0x98FE9E31), 0), // FPropertyBagCustomVersion: BeforeCustomVersionWasAdded
    (Guid::from_parts(0x5579F886, 0x933A4C1F, 0x83BA087B, 0x6361B92F), 2), // FRecomputeTangentCustomVersion: RecomputeTangentVertexColorMask
    (Guid::from_parts(0x12F88B9F, 0x88754AFC, 0xA67CD90C, 0x383ABD29), 44), // FRenderingObjectVersion: VolumeExtinctionBecomesRGB
    (Guid::from_parts(0x7B5AE74C, 0xD2704C10, 0xA9585798, 0x0B212A5A), 12), // FSequencerObjectVersion: SerializeFloatChannelCompletely
    (Guid::from_parts(0xD78A4A00, 0xE8584697, 0xBAA819B5, 0x487D46B4), 18), // FSkeletalMeshCustomVersion: RemoveEnableClothLOD
    (Guid::from_parts(0x697DD581, 0xE64F41AB, 0xAA4A51EC, 0xBEB7B628), 0), // FUE5MainStreamObjectVersion: BeforeCustomVersionWasAdded
    (Guid::from_parts(0xD89B5E42, 0x24BD4D46, 0x8412ACA8, 0xDF641779), 0), // FUE5ReleaseStreamObjectVersion: BeforeCustomVersionWasAdded
    (Guid::from_parts(0x59DA5D52, 0x12324948, 0xB8785978, 0x70B8E98B), 0), // FUE5SpecialProjectStreamObjectVersion: BeforeCustomVersionWasAdded
    (Guid::from_parts(0x24BB7AF3, 0x56464F83, 0x1F2F2DC2, 0x49AD96FF), 5), // FVariantManagerObjectVersion: StoreDisplayOrder
    (Guid::from_parts(0x9C54D522, 0xA8264FBE, 0x94210746, 0x61B482D0), 38), // FReleaseObjectVersion: StructureDataAddedToConvex
];

/// The 4.26 default of a custom version, or -1 when the table has none.
pub fn default_version(guid: Guid) -> i32 {
    UE4_26_CUSTOM_VERSIONS.iter().find(|(g, _)| *g == guid).map_or(-1, |&(_, v)| v)
}

pub const FORTNITE_MAIN: Guid = Guid::from_parts(0x601D1886, 0xAC644F84, 0xAA16D3DE, 0x0DEAC7D6);
pub const RELEASE: Guid = Guid::from_parts(0x9C54D522, 0xA8264FBE, 0x94210746, 0x61B482D0);
pub const EDITOR: Guid = Guid::from_parts(0xE4B068ED, 0xF49442E9, 0xA231DA0B, 0x2E46BB41);
pub const FRAMEWORK: Guid = Guid::from_parts(0xCFFC743F, 0x43B04480, 0x939114DF, 0x171D2073);
pub const CORE: Guid = Guid::from_parts(0x375EC13C, 0x06E448FB, 0xB50084F0, 0x262A717E);
pub const ANIM: Guid = Guid::from_parts(0xAF43A65D, 0x7FD34947, 0x98733E8E, 0xD9C1BB05);
pub const ANIM_PHYS: Guid = Guid::from_parts(0x29E575DD, 0xE0A34627, 0x9D10D276, 0x232CDCEA);
pub const RENDERING: Guid = Guid::from_parts(0x12F88B9F, 0x88754AFC, 0xA67CD90C, 0x383ABD29);
pub const PHYSICS: Guid = Guid::from_parts(0x78F01B33, 0xEBEA4F98, 0xB9B484EA, 0xCCB95AA2);
pub const SKELETAL_MESH: Guid = Guid::from_parts(0xD78A4A00, 0xE8584697, 0xBAA819B5, 0x487D46B4);
pub const MOBILE: Guid = Guid::from_parts(0xB02B49B5, 0xBB2044E9, 0xA30432B7, 0x52E40360);
pub const NIAGARA: Guid = Guid::from_parts(0xFCF57AFA, 0x50764283, 0xB9A9E658, 0xFFA02D32);
