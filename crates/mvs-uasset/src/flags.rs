//! Flag enums as CUE4Parse's ToStringBitfield writes them (generated from CUE4Parse 1.2.2's enums): every value, in
//! Enum.GetValues order, whose bits are all set (a zero value only when the whole field is zero), joined by " | ".

/// `(value, name)` in Enum.GetValues order.
pub type FlagTable = &'static [(u64, &'static str)];

/// EnumUtils.ToStringBitfield: "0" when no value matches.
pub fn bitfield(value: u64, table: FlagTable) -> String {
    let mut out = String::new();
    for &(f, n) in table {
        let hit = if f == 0 { value == 0 } else { value & f == f };
        if hit {
            if !out.is_empty() {
                out.push_str(" | ");
            }
            out.push_str(n);
        }
    }
    if out.is_empty() { "0".to_owned() } else { out }
}

/// Enum.ToStringBitfield of EObjectFlags (UObject.WriteJson's `Flags`, the struct natives' flags): `RF_NoFlags` for 0.
pub fn object_flags_text(flags: u32) -> String {
    bitfield(u64::from(flags), OBJECT_FLAGS)
}

pub const PROPERTY_FLAGS: FlagTable = &[
    (0x0, "None"),
    (0x1, "Edit"),
    (0x2, "ConstParm"),
    (0x4, "BlueprintVisible"),
    (0x8, "ExportObject"),
    (0x10, "BlueprintReadOnly"),
    (0x20, "Net"),
    (0x40, "EditFixedSize"),
    (0x80, "Parm"),
    (0x100, "OutParm"),
    (0x200, "ZeroConstructor"),
    (0x400, "ReturnParm"),
    (0x800, "DisableEditOnTemplate"),
    (0x1000, "NonNullable"),
    (0x2000, "Transient"),
    (0x4000, "Config"),
    (0x8000, "RequiredParm"),
    (0x10000, "DisableEditOnInstance"),
    (0x20000, "EditConst"),
    (0x40000, "GlobalConfig"),
    (0x80000, "InstancedReference"),
    (0x200000, "DuplicateTransient"),
    (0x1000000, "SaveGame"),
    (0x2000000, "NoClear"),
    (0x8000000, "ReferenceParm"),
    (0x8008582, "ParmFlags"),
    (0x10000000, "BlueprintAssignable"),
    (0x20000000, "Deprecated"),
    (0x40000000, "IsPlainOldData"),
    (0x80000000, "RepSkip"),
    (0x100000000, "RepNotify"),
    (0x200000000, "Interp"),
    (0x400000000, "NonTransactional"),
    (0x800000000, "EditorOnly"),
    (0x800000000, "DevelopmentAssets"),
    (0x1000000000, "NoDestructor"),
    (0x4000000000, "AutoWeak"),
    (0x8000000000, "ContainsInstancedReference"),
    (0x8000080008, "InterfaceClearMask"),
    (0x10000000000, "AssetRegistrySearchable"),
    (0x20000000000, "SimpleDisplay"),
    (0x40000000000, "AdvancedDisplay"),
    (0x80000000000, "Protected"),
    (0x100000000000, "BlueprintCallable"),
    (0x200000000000, "BlueprintAuthorityOnly"),
    (0x400000000000, "TextExportTransient"),
    (0x800000000000, "NonPIEDuplicateTransient"),
    (0x1000000000000, "ExposeOnSpawn"),
    (0x2000000000000, "PersistentInstance"),
    (0x4000000000000, "UObjectWrapper"),
    (0x600c8200a4008, "PropagateToArrayInner"),
    (0x600c8200a4009, "PropagateToMapValue"),
    (0x600c8200a4009, "PropagateToMapKey"),
    (0x600c8200a4009, "PropagateToSetElement"),
    (0x8000000000000, "HasGetValueTypeHash"),
    (0x8001040000200, "ComputedFlags"),
    (0x10000000000000, "NativeAccessSpecifierPublic"),
    (0x20000000000000, "NativeAccessSpecifierProtected"),
    (0x40000000000000, "NativeAccessSpecifierPrivate"),
    (0x70000000000000, "NativeAccessSpecifiers"),
    (0x80000000000000, "SkipSerialization"),
    (0xffffffffffffffff, "AllFlags"),
];

pub const FUNCTION_FLAGS: FlagTable = &[
    (0x0, "FUNC_None"),
    (0x1, "FUNC_Final"),
    (0x2, "FUNC_RequiredAPI"),
    (0x4, "FUNC_BlueprintAuthorityOnly"),
    (0x8, "FUNC_BlueprintCosmetic"),
    (0x40, "FUNC_Net"),
    (0x80, "FUNC_NetReliable"),
    (0x100, "FUNC_NetRequest"),
    (0x200, "FUNC_Exec"),
    (0x400, "FUNC_Native"),
    (0x800, "FUNC_Event"),
    (0x1000, "FUNC_NetResponse"),
    (0x2000, "FUNC_Static"),
    (0x4000, "FUNC_NetMulticast"),
    (0x8000, "FUNC_UbergraphFunction"),
    (0x10000, "FUNC_MulticastDelegate"),
    (0x20000, "FUNC_Public"),
    (0x40000, "FUNC_Private"),
    (0x80000, "FUNC_Protected"),
    (0x100000, "FUNC_Delegate"),
    (0x200000, "FUNC_NetServer"),
    (0x400000, "FUNC_HasOutParms"),
    (0x800000, "FUNC_HasDefaults"),
    (0x1000000, "FUNC_NetClient"),
    (0x2000000, "FUNC_DLLImport"),
    (0x4000000, "FUNC_BlueprintCallable"),
    (0x8000000, "FUNC_BlueprintEvent"),
    (0x10000000, "FUNC_BlueprintPure"),
    (0x20000000, "FUNC_EditorOnly"),
    (0x40000000, "FUNC_Const"),
    (0x80000000, "FUNC_NetValidate"),
    (0xffffffff, "FUNC_AllFlags"),
];

pub const CLASS_FLAGS: FlagTable = &[
    (0x0, "CLASS_None"),
    (0x1, "CLASS_Abstract"),
    (0x2, "CLASS_DefaultConfig"),
    (0x4, "CLASS_Config"),
    (0x8, "CLASS_Transient"),
    (0x10, "CLASS_Optional"),
    (0x20, "CLASS_MatchedSerializers"),
    (0x40, "CLASS_ProjectUserConfig"),
    (0x80, "CLASS_Native"),
    (0x100, "CLASS_NoExport"),
    (0x200, "CLASS_NotPlaceable"),
    (0x400, "CLASS_PerObjectConfig"),
    (0x800, "CLASS_ReplicationDataIsSetUp"),
    (0x1000, "CLASS_EditInlineNew"),
    (0x2000, "CLASS_CollapseCategories"),
    (0x4000, "CLASS_Interface"),
    (0x8000, "CLASS_CustomConstructor"),
    (0x10000, "CLASS_Const"),
    (0x20000, "CLASS_NeedsDeferredDependencyLoading"),
    (0x40000, "CLASS_CompiledFromBlueprint"),
    (0x80000, "CLASS_MinimalAPI"),
    (0x100000, "CLASS_RequiredAPI"),
    (0x200000, "CLASS_DefaultToInstanced"),
    (0x400000, "CLASS_TokenStreamAssembled"),
    (0x800000, "CLASS_HasInstancedReference"),
    (0x1000000, "CLASS_Hidden"),
    (0x2000000, "CLASS_Deprecated"),
    (0x4000000, "CLASS_HideDropDown"),
    (0x8000000, "CLASS_GlobalUserConfig"),
    (0x10000000, "CLASS_Intrinsic"),
    (0x20000000, "CLASS_Constructed"),
    (0x40000000, "CLASS_ConfigDoNotCheckDefaults"),
    (0x80000000, "CLASS_NewerVersionExists"),
];

pub const OBJECT_FLAGS: FlagTable = &[
    (0x0, "RF_NoFlags"),
    (0x1, "RF_Public"),
    (0x2, "RF_Standalone"),
    (0x4, "RF_MarkAsNative"),
    (0x8, "RF_Transactional"),
    (0x10, "RF_ClassDefaultObject"),
    (0x20, "RF_ArchetypeObject"),
    (0x40, "RF_Transient"),
    (0x80, "RF_MarkAsRootSet"),
    (0x100, "RF_TagGarbageTemp"),
    (0x200, "RF_NeedInitialization"),
    (0x400, "RF_NeedLoad"),
    (0x800, "RF_KeepForCooker"),
    (0x1000, "RF_NeedPostLoad"),
    (0x2000, "RF_NeedPostLoadSubobjects"),
    (0x4000, "RF_NewerVersionExists"),
    (0x8000, "RF_BeginDestroyed"),
    (0x10000, "RF_FinishDestroyed"),
    (0x20000, "RF_BeingRegenerated"),
    (0x40000, "RF_DefaultSubObject"),
    (0x80000, "RF_WasLoaded"),
    (0x100000, "RF_TextExportTransient"),
    (0x200000, "RF_LoadCompleted"),
    (0x400000, "RF_InheritableComponentTemplate"),
    (0x800000, "RF_DuplicateTransient"),
    (0x1000000, "RF_StrongRefOnFrame"),
    (0x2000000, "RF_NonPIEDuplicateTransient"),
    (0x4000000, "RF_Dynamic"),
    (0x8000000, "RF_WillBeLoaded"),
];

pub const BULK_DATA_FLAGS: FlagTable = &[
    (0x0, "BULKDATA_None"),
    (0x1, "BULKDATA_PayloadAtEndOfFile"),
    (0x2, "BULKDATA_SerializeCompressedZLIB"),
    (0x2, "BULKDATA_SerializeCompressed"),
    (0x4, "BULKDATA_ForceSingleElementSerialization"),
    (0x8, "BULKDATA_SingleUse"),
    (0x10, "BULKDATA_CompressedLZO"),
    (0x20, "BULKDATA_Unused"),
    (0x40, "BULKDATA_ForceInlinePayload"),
    (0x80, "BULKDATA_ForceStreamPayload"),
    (0x100, "BULKDATA_PayloadInSeperateFile"),
    (0x200, "BULKDATA_SerializeCompressedBitWindow"),
    (0x400, "BULKDATA_Force_NOT_InlinePayload"),
    (0x800, "BULKDATA_OptionalPayload"),
    (0x1000, "BULKDATA_MemoryMappedPayload"),
    (0x2000, "BULKDATA_Size64Bit"),
    (0x4000, "BULKDATA_DuplicateNonOptionalPayload"),
    (0x8000, "BULKDATA_BadDataVersion"),
    (0x10000, "BULKDATA_NoOffsetFixUp"),
    (0x20000, "BULKDATA_WorkspaceDomainPayload"),
    (0x40000, "BULKDATA_LazyLoadable"),
    (0x10000000, "BULKDATA_AlwaysAllowDiscard"),
    (0x20000000, "BULKDATA_HasAsyncReadPending"),
    (0x40000000, "BULKDATA_DataIsMemoryMapped"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitfield_as_cue4parse() {
        assert_eq!(object_flags_text(0), "RF_NoFlags");
        assert_eq!(object_flags_text(0x10 | 0x80000), "RF_ClassDefaultObject | RF_WasLoaded");
        assert_eq!(bitfield(0, &[(1, "A")]), "0");
        assert_eq!(bitfield(0x41, BULK_DATA_FLAGS), "BULKDATA_PayloadAtEndOfFile | BULKDATA_ForceInlinePayload");
    }
}
