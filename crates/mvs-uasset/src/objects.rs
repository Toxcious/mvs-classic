//! Exports as CUE4Parse constructs and serializes them: the C# type it builds an export as (ConstructObject: the
//! first registered class up the class's SuperStruct chain), UObject.Deserialize, and the native classes on that
//! type's chain (`natives`). The export archive is here too.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::archive::{Archive, Guid, ReadError, Result};
use crate::fields::StructFields;
use crate::package::{Package, PackageCache, Resolved};
use crate::value::{ALLOC_OVERHEAD, Props, alloc_bytes, props_heap_bytes};
use crate::{classes, natives, props, versions};

pub const RF_CLASS_DEFAULT_OBJECT: u32 = 0x10;
pub const RF_WAS_LOADED: u32 = 0x80000;

/// What went wrong reading an export (kept on the object it is about).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueKind {
    /// The export's read failed: it holds what was read before.
    Read,
    /// A class on its chain has a native serializer the port lacks (the object is then `partial`).
    Native,
    /// A value, a native struct or a text history the port lacks (the value was read as null).
    NotPorted,
}

#[derive(Clone, Debug)]
pub struct ReadIssue {
    pub kind: IssueKind,
    pub message: String,
}

/// An export as read.
pub struct UObj {
    pub pkg: Arc<Package>,
    pub index: usize,
    pub name: String,
    /// The export's object flags with RF_WasLoaded.
    pub flags: u32,
    pub cls: Option<Resolved>,
    /// Its class's name (`UObject` without one).
    pub type_name: String,
    /// The registered type it is built as (`None`: a plain UObject).
    pub reg: Option<&'static str>,
    /// The native classes it runs, most derived first: the type's chain below `partial` when there is one.
    pub chain: Vec<&'static str>,
    /// The most basic class on the type's chain the port has no reader for: the object reads its tagged properties
    /// and the classes below this one and stops where this class's data starts. `None`: read whole.
    pub partial: Option<&'static str>,
    pub props: Props,
    /// The native classes' fields, by CUE4Parse's names; what they write comes from here (`natives`).
    pub data: Props,
    pub object_guid: Option<Guid>,
    pub issues: Vec<ReadIssue>,
}

impl UObj {
    /// The heap bytes it holds of its own, itself included (its package and the objects it refers to are not its
    /// own): what the package cache counts against its budget for it.
    pub fn heap_bytes(&self) -> usize {
        let cls = match &self.cls {
            Some(Resolved::Script(n)) => alloc_bytes(n.capacity()),
            _ => 0,
        };
        let issues: usize = self.issues.iter().map(|i| alloc_bytes(i.message.capacity())).sum();
        size_of::<UObj>()
            + 2 * ALLOC_OVERHEAD
            + alloc_bytes(self.name.capacity())
            + alloc_bytes(self.type_name.capacity())
            + alloc_bytes(self.chain.capacity() * size_of::<&str>())
            + cls
            + props_heap_bytes(&self.props)
            + props_heap_bytes(&self.data)
            + alloc_bytes(self.issues.capacity() * size_of::<ReadIssue>())
            + issues
    }
}

/// The archive of one export: its package's export data, names and imports through the package.
pub struct ExportArchive<'a> {
    pub ar: Archive<'a>,
    pub pkg: &'a Arc<Package>,
    pub cache: &'a PackageCache,
    /// The export being read.
    pub export_index: usize,
    /// What the read reported (`report`); moved onto the object at the end.
    pub issues: Vec<ReadIssue>,
}

impl<'a> Deref for ExportArchive<'a> {
    type Target = Archive<'a>;
    fn deref(&self) -> &Archive<'a> {
        &self.ar
    }
}

impl DerefMut for ExportArchive<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ar
    }
}

impl<'a> ExportArchive<'a> {
    /// The absolute offset in the package of `pos` (the .uasset's length is added: `pos` is in the .uexp).
    pub fn abs_pos(&self) -> usize {
        self.ar.pos + self.pkg.uasset_len
    }

    /// An FName: index and number; an index outside the name map is a parse error.
    pub fn fname(&mut self) -> Result<String> {
        let i = self.ar.i32()?;
        let n = self.ar.i32()?;
        self.pkg.name_text(i, n)
    }

    /// An FPackageIndex, resolved (`None` for 0 or an index past the tables).
    pub fn index(&mut self) -> Result<Option<Resolved>> {
        let i = self.ar.i32()?;
        self.cache.try_resolve(self.pkg, i)
    }

    /// The package's UE4 version (522).
    pub fn ver(&self) -> i32 {
        self.pkg.summary.file_version_ue4
    }

    /// A custom version: the package's own, else the UE 4.26 default (`versions`), else -1.
    pub fn custom(&self, guid: Guid) -> i32 {
        let own = self.pkg.summary.custom_versions.iter().find(|(g, _)| *g == guid);
        own.map_or_else(|| versions::default_version(guid), |&(_, v)| v)
    }

    pub fn filter_editor_only(&self) -> bool {
        self.pkg.summary.package_flags & crate::package::PKG_FILTER_EDITOR_ONLY != 0
    }

    /// Records an issue of the export being read.
    pub fn report(&mut self, kind: IssueKind, message: String) {
        self.issues.push(ReadIssue { kind, message });
    }
}

/// Package.DeserializeObject: an error leaves the object with what it read and goes on its issues; a class on the
/// chain the port has no reader for is an issue too (`Native`, naming `partial`). Called by
/// [`PackageCache::object`], which holds the result.
pub fn deserialize_object(cache: &PackageCache, pkg: &Arc<Package>, index: usize) -> UObj {
    let mut obj = new_object(cache, pkg, index);
    if pkg.exports[index].serial_size == 0 {
        return obj;
    }
    let (reported, r) = read_export(cache, pkg, index, &mut obj);
    obj.issues.extend(reported);
    if let Err(err) = r {
        let kind = if matches!(err, ReadError::NotPorted(_)) { IssueKind::NotPorted } else { IssueKind::Read };
        obj.issues.push(ReadIssue { kind, message: err.to_string() });
    }
    obj
}

/// The object an export is built as, nothing read yet: its class, the type CUE4Parse
/// builds, the native chain and `partial`. A class that does not resolve is a `Read` issue, an unported class on the
/// chain a `Native` one.
pub fn new_object(cache: &PackageCache, pkg: &Arc<Package>, index: usize) -> UObj {
    let e = &pkg.exports[index];
    let mut issues = Vec::new();
    // Deviation: a class index that names a package that does not parse is a read issue of the object, read without a
    // class, not an error out of the read.
    let cls = cache.try_resolve(pkg, e.class_index).unwrap_or_else(|err| {
        issues.push(ReadIssue { kind: IssueKind::Read, message: err.to_string() });
        None
    });
    let type_name = cls.as_ref().map_or_else(|| "UObject".to_owned(), |c| c.name().to_owned());
    // Deviation: as the class index, a class or super on the chain whose package does not parse is a read issue
    // (not an error out of the read); the object is then built as a plain UObject.
    let reg = construct_type(cache, cls.as_ref(), e.object_flags).unwrap_or_else(|err| {
        issues.push(ReadIssue { kind: IssueKind::Read, message: err.to_string() });
        None
    });
    let full = classes::chain_of(reg);
    let cut = full.iter().rposition(|c| natives::native(c).is_none());
    let partial = cut.map(|i| full[i]);
    let chain = full[cut.map_or(0, |i| i + 1)..].to_vec();
    if let Some(p) = partial {
        let message = format!("{type_name}: native class {p} (of {}) not ported", reg.unwrap_or_default());
        issues.push(ReadIssue { kind: IssueKind::Native, message });
    }
    UObj {
        pkg: pkg.clone(),
        index,
        name: e.object_name.clone(),
        flags: e.object_flags | RF_WAS_LOADED,
        cls,
        type_name,
        reg,
        chain,
        partial,
        props: Props::new(),
        data: Props::new(),
        object_guid: None,
        issues,
    }
}

/// The export's data read into `obj` from its serial offset: what the read reported, and its result.
fn read_export(cache: &PackageCache, pkg: &Arc<Package>, index: usize, obj: &mut UObj) -> (Vec<ReadIssue>, Result<()>) {
    let (mut ar, end) = match export_archive(cache, pkg, index) {
        Ok(a) => a,
        Err(err) => return (Vec::new(), Err(err)),
    };
    let r = super_deserialize(obj, &mut ar, end, 0);
    (ar.issues, r)
}

/// The archive of export `index` at its serial offset, and where the export ends in it.
pub fn export_archive<'a>(
    cache: &'a PackageCache,
    pkg: &'a Arc<Package>,
    index: usize,
) -> Result<(ExportArchive<'a>, usize)> {
    let e = &pkg.exports[index];
    let data = pkg.uexp()?;
    let start = e.serial_offset - pkg.uasset_len as i64;
    let (Ok(pos), Ok(size)) = (usize::try_from(start), usize::try_from(e.serial_size)) else {
        let at = (e.serial_offset, e.serial_size);
        return Err(ReadError::parse(format!("{}: export {index} at {at:?} lies outside its data", pkg.path)));
    };
    let mut ar =
        ExportArchive { ar: Archive::new(data, &pkg.path), pkg, cache, export_index: index, issues: Vec::new() };
    ar.pos = pos;
    Ok((ar, pos + size))
}

/// AbstractUePackage.ConstructObject: the registered type an export of class `cls` is built as (the first registered
/// class up the class's SuperStruct chain), or `None` for a plain UObject (registered types that override nothing
/// count as plain). `flags`: the export's object flags. Errors: a class or super on the chain names a package that
/// does not parse.
pub fn construct_type(cache: &PackageCache, cls: Option<&Resolved>, flags: u32) -> Result<Option<&'static str>> {
    construct_type_at(cache, cls.cloned(), flags, 0)
}

fn construct_type_at(
    cache: &PackageCache,
    cls: Option<Resolved>,
    flags: u32,
    mut depth: u32,
) -> Result<Option<&'static str>> {
    let mut s = cls;
    while let Some(r) = s {
        if depth >= 64 {
            break;
        }
        depth += 1;
        let (pkg, i) = match &r {
            Resolved::Import { pkg, index } => {
                let cn = &pkg.imports[*index].class_name;
                if cn != "Class" && cn != "ScriptStruct" {
                    return Ok(None);
                }
                return Ok(classes::registered(r.name())); // a UScriptClass: no SuperStruct
            }
            Resolved::Script(name) => return Ok(classes::registered(name)),
            Resolved::Package(_) => return Ok(None),
            Resolved::Export { pkg, index } => (pkg, *index),
        };
        // the export is the class object: it is a UStruct only when its own type is
        let e = &pkg.exports[i];
        let own =
            classes::chain_of(construct_type_at(cache, cache.try_resolve(pkg, e.class_index)?, e.object_flags, depth)?);
        if !own.contains(&"UStruct") {
            return Ok(None);
        }
        if own.contains(&"UClass") {
            if let Some(t) = classes::registered(&e.object_name) {
                return Ok(Some(t));
            }
            if own.contains(&"UBlueprintGeneratedClass") && flags & RF_CLASS_DEFAULT_OBJECT != 0 {
                return Ok(None);
            }
        }
        s = cache.try_resolve(pkg, e.super_index)?;
    }
    Ok(None)
}

/// Calls the first Deserialize on the chain at or after index `from` (C#'s base.Deserialize), or UObject's own at
/// the bottom (`base_deserialize`). The object's read passes 0; a native at chain index `level` calls this first
/// with `level + 1`.
pub fn super_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, from: usize) -> Result<()> {
    for i in from..obj.chain.len() {
        if let Some(f) = natives::native(obj.chain[i]).and_then(|n| n.deserialize) {
            return f(obj, ar, valid_pos, i);
        }
    }
    base_deserialize(obj, ar, valid_pos)
}

/// UObject.Deserialize for a versioned UE 4 package: the tagged properties (`props::read_tagged_properties`, with
/// the class's fields), then the object GUID (not on a class default object).
pub fn base_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize) -> Result<()> {
    let fields = StructFields::of(obj.cls.clone());
    props::read_tagged_properties(ar, &mut obj.props, Some(&fields))?;
    if obj.flags & RF_CLASS_DEFAULT_OBJECT == 0 && ar.bool()? {
        if ar.pos + 16 > valid_pos {
            return Err(ReadError::parse("Unexpected EOF in ObjectGuid"));
        }
        obj.object_guid = Some(ar.guid()?);
    }
    Ok(())
}

/// For a native class's object path (`/Script/<Module>.<Name>`): the first class on its chain the port has no
/// reader for, else `None`; `None` for any other path.
pub fn unported_native_of(class_path: &str) -> Option<&'static str> {
    let name = class_path.strip_prefix("/Script/")?;
    let name = name.rsplit('.').next().unwrap_or(name);
    missing_native(classes::chain_of(classes::registered(name)))
}

/// The first class of a chain the port has no reader for.
pub fn missing_native(chain: &[&'static str]) -> Option<&'static str> {
    chain.iter().copied().find(|c| natives::native(c).is_none())
}
