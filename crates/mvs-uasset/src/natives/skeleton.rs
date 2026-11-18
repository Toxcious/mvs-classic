//! USkeleton's reference skeleton, and the read every heavy kind's native tail starts with: an export read from its
//! serial offset through its tagged properties and the ported classes on its chain, to where its unported class's
//! data starts. Read for the skeleton, mesh and animation kinds, never entered in NATIVES: the object kind keeps
//! these classes `partial`. It follows CUE4Parse 1.2.2 (USkeleton.Deserialize, FReferenceSkeleton,
//! FMeshBoneInfo), the UE 4.26 cooked branch only.

use std::sync::Arc;

use crate::archive::{ReadError, Result};
use crate::objects::{ExportArchive, IssueKind, UObj, export_archive, new_object, super_deserialize};
use crate::package::{Package, PackageCache};

/// An export read up to its native data: the object (its tagged properties and the ported classes' fields), the
/// archive at the start of the class's native data, and where the export ends in it.
pub struct NativeData<'a> {
    pub obj: UObj,
    pub ar: ExportArchive<'a>,
    pub end: usize,
}

/// Export `index` of `pkg` read from its serial offset through its tagged properties and the ported classes on its
/// chain (the object kind's read, without its issue for the cut), stopped where `cls` (a C# class name, `USkeleton`)
/// starts: the export must be partial at `cls`. What the read reports (a value not ported) stays on `ar.issues`, which
/// no heavy kind reads.
pub fn at_native_data<'a>(
    cache: &'a PackageCache,
    pkg: &'a Arc<Package>,
    index: usize,
    cls: &str,
) -> Result<NativeData<'a>> {
    let Some(e) = pkg.exports.get(index) else {
        return Err(ReadError::parse(format!("{}: no export {index}", pkg.path)));
    };
    let mut obj = new_object(cache, pkg, index);
    // the object's constructor throws where the class does not resolve
    if let Some(i) = obj.issues.iter().find(|i| i.kind == IssueKind::Read) {
        return Err(ReadError::parse(i.message.clone()));
    }
    if obj.partial != Some(cls) {
        return Err(ReadError::parse(format!(
            "{}.{}: a {} ({}), not a {cls}",
            pkg.path,
            e.object_name,
            obj.type_name,
            obj.reg.unwrap_or("UObject")
        )));
    }
    let (mut ar, end) = export_archive(cache, pkg, index)?;
    super_deserialize(&mut obj, &mut ar, end, 0)?;
    Ok(NativeData { obj, ar, end })
}

/// Fails unless the read of `what` ended where the export does.
pub fn ends_at(ar: &ExportArchive<'_>, end: usize, what: &str) -> Result<()> {
    if ar.pos != end {
        return Err(ReadError::parse(format!("{}: {what} ended at {}, the export at {end}", ar.name, ar.pos)));
    }
    Ok(())
}

/// FReferenceSkeleton of a Skeleton export or a SkeletalMesh export: bone names, parents, and the pose, 10 numbers a
/// bone (translation xyz, rotation xyzw, scale xyz), float32 values as stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReferenceSkeleton {
    pub bones: Vec<String>,
    pub parents: Vec<i32>,
    pub pose: Vec<f32>,
}

/// FReferenceSkeleton(FAssetArchive) for a cooked 4.26 package, read at the archive's place (USkeleton's native data,
/// or USkeletalMesh's after its bounds and materials: the mesh reader calls this): FMeshBoneInfo[] (name, parent; no
/// export name in a cooked package), FTransform[] (rotation xyzw, translation xyz, scale xyz, float32), then
/// FinalNameToIndexMap, checked against the names. The pose is the stored one: CUE4Parse's AdjustBoneScales (each
/// translation scaled by the product of its ancestors' scales, done on read) is not applied.
pub fn read_reference_skeleton(ar: &mut ExportArchive<'_>) -> Result<ReferenceSkeleton> {
    let n = ar.i32()?;
    let Ok(count) = usize::try_from(n) else {
        return Err(ReadError::parse(format!("{}: bone count {n} at {}", ar.name, ar.pos - 4)));
    };
    if !ar.filter_editor_only() {
        return Err(ReadError::not_ported(format!(
            "{}: FMeshBoneInfo with export names (editor package) not ported",
            ar.name
        )));
    }
    ar.need(count * 12)?;
    let mut bones = Vec::with_capacity(count);
    let mut parents = Vec::with_capacity(count);
    for i in 0..count {
        let name = ar.fname()?;
        let p = ar.i32()?;
        let ok = usize::try_from(p).map_or(p == -1, |p| p < i) && (p == -1) == (i == 0);
        if !ok {
            return Err(ReadError::parse(format!("{}: bone {i} {name} has parent {p}", ar.name)));
        }
        bones.push(name);
        parents.push(p);
    }
    let np = ar.i32()?;
    if np != n {
        return Err(ReadError::parse(format!("{}: {np} reference poses for {n} bones", ar.name)));
    }
    ar.need(count * 40)?;
    let mut pose = vec![0f32; count * 10];
    for o in (0..count).map(|i| i * 10) {
        let [rx, ry, rz, rw] = [ar.f32()?, ar.f32()?, ar.f32()?, ar.f32()?];
        let [tx, ty, tz] = [ar.f32()?, ar.f32()?, ar.f32()?];
        let [sx, sy, sz] = [ar.f32()?, ar.f32()?, ar.f32()?];
        pose[o..o + 10].copy_from_slice(&[tx, ty, tz, rx, ry, rz, rw, sx, sy, sz]);
    }
    let nm = ar.i32()?;
    if nm != n {
        return Err(ReadError::parse(format!("{}: name map of {nm} for {n} bones", ar.name)));
    }
    for _ in 0..count {
        let name = ar.fname()?;
        let at = ar.i32()?;
        let bone = usize::try_from(at).ok().and_then(|a| bones.get(a));
        if bone != Some(&name) {
            // (`undefined` is printed for a bone past the list)
            let shown = bone.map_or("undefined", String::as_str);
            return Err(ReadError::parse(format!("{}: name map gives {name} -> {at} ({shown})", ar.name)));
        }
    }
    Ok(ReferenceSkeleton { bones, parents, pose })
}

/// The reference skeleton of a Skeleton export (a SkeletalMesh's is read with its render data through
/// [`read_reference_skeleton`]).
pub fn read_skeleton(cache: &PackageCache, pkg: &Arc<Package>, index: usize) -> Result<ReferenceSkeleton> {
    let mut d = at_native_data(cache, pkg, index, "USkeleton")?;
    read_reference_skeleton(&mut d.ar)
}
