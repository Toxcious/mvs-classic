//! The package reader: a UE 4.26 cooked package's summary, names, imports and exports, its indexes resolved (imports
//! through the packages they name, as CUE4Parse's legacy ResolvePackageIndex does), and the cache of packages read
//! from the paks. A read
//! here loads what it names when it names it.
//!
//! Ownership: a [`Package`] holds its header and the file source it reads its .uexp from, never its objects, so a value that
//! refers to an object of its own package ([`Resolved`] holds an `Arc<Package>`) makes no cycle. The objects read
//! ([`crate::objects::UObj`]) and the resolved imports are held by the [`PackageCache`], which drops everything it
//! holds once past its byte budget ([`PackageCache::trim`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock, Weak};
use std::thread::{self, ThreadId};

use mvs_pak::paths::{join_object_path, split_object_path};
use mvs_pak::{BuildPaths, PakSet};

use crate::archive::{Archive, Guid, ReadError, Result};
use crate::objects::{UObj, deserialize_object};
use crate::value::{ALLOC_OVERHEAD, alloc_bytes};

/// Where packages' files come from: the paks, or a folder of files in a test.
pub trait FileSource: Send + Sync {
    /// A file's bytes by its pak path (`MultiVersus/Content/A/B.uexp`, looked up without regard to case); `None`
    /// when there is no such file.
    fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>>;

    /// The path of a file `read_file` found, as the source spells it (the paks' own case: a package's path and object
    /// path are made from it). The default: as asked.
    fn spelling(&self, path: &str) -> String {
        path.to_owned()
    }
}

impl FileSource for PakSet {
    fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match self.entry(path) {
            None => Ok(None),
            Some(e) => Ok(Some(self.read(&e.path)?)),
        }
    }

    fn spelling(&self, path: &str) -> String {
        self.entry(path).map_or_else(|| path.to_owned(), |e| e.path.clone())
    }
}

/// The package summary (FPackageFileSummary) fields the reader uses.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub legacy_version: i32,
    /// 522 (UE 4.26) for the build's unversioned packages.
    pub file_version_ue4: i32,
    pub licensee: i32,
    /// The package's own custom versions (the build's packages list none: `versions` gives the 4.26 defaults).
    pub custom_versions: Vec<(Guid, i32)>,
    pub total_header_size: i32,
    pub package_flags: u32,
    pub name_count: i32,
    pub name_offset: i32,
    pub export_count: i32,
    pub export_offset: i32,
    pub import_count: i32,
    pub import_offset: i32,
    pub bulk_data_start_offset: i64,
    pub preload_dependency_count: i32,
    pub preload_dependency_offset: i32,
}

pub const PKG_FILTER_EDITOR_ONLY: u32 = 0x8000_0000;
pub const PKG_UNVERSIONED_PROPERTIES: u32 = 0x2000;

/// A count or an offset of the summary as a `usize`; a negative one is a parse error.
fn non_negative(v: i32, what: &str, name: &str) -> Result<usize> {
    usize::try_from(v).map_err(|_| ReadError::parse(format!("{name}: negative {what} {v}")))
}

/// FPackageFileSummary as CUE4Parse reads a UE 4 package's.
fn read_summary(ar: &mut Archive<'_>) -> Result<Summary> {
    let name = ar.name;
    let tag = ar.u32()?;
    if tag != 0x9E2A_83C1 {
        return Err(ReadError::parse(format!("{name}: invalid uasset magic 0x{tag:x}")));
    }
    let legacy = ar.i32()?;
    if !(-7..0).contains(&legacy) {
        return Err(ReadError::parse(format!("{name}: unsupported legacy file version {legacy}")));
    }
    if legacy != -4 {
        ar.i32()?; // UE3 version
    }
    let mut file_version_ue4 = ar.i32()?;
    let licensee = ar.i32()?;
    // an unversioned package (the build's are): CUE4Parse takes the game's version, UE4_26 = 522
    if file_version_ue4 == 0 && licensee == 0 {
        file_version_ue4 = 522;
    }
    let mut custom_versions = Vec::new();
    if legacy <= -2 {
        let n = ar.i32()?;
        for _ in 0..n {
            let g = ar.guid()?;
            let v = ar.i32()?;
            match custom_versions.iter_mut().find(|(k, _)| *k == g) {
                Some((_, old)) => *old = v,
                None => custom_versions.push((g, v)),
            }
        }
    }
    let total_header_size = ar.i32()?;
    ar.fstring()?; // folder name
    let package_flags = ar.u32()?;
    let name_count = ar.i32()?;
    let name_offset = ar.i32()?;
    if package_flags & PKG_FILTER_EDITOR_ONLY == 0 && file_version_ue4 >= 516 {
        ar.fstring()?; // localization id
    }
    if file_version_ue4 >= 459 {
        ar.i32()?; // gatherable text data
        ar.i32()?;
    }
    let export_count = ar.i32()?;
    let export_offset = ar.i32()?;
    let import_count = ar.i32()?;
    let import_offset = ar.i32()?;
    ar.i32()?; // depends
    if file_version_ue4 >= 384 {
        ar.i32()?; // soft package references
        ar.i32()?;
    }
    if file_version_ue4 >= 510 {
        ar.i32()?; // searchable names
    }
    ar.i32()?; // thumbnails
    ar.skip(16)?; // guid
    if package_flags & PKG_FILTER_EDITOR_ONLY == 0 {
        ar.skip(16)?;
        if file_version_ue4 < 520 {
            ar.skip(16)?;
        }
    }
    let gens = non_negative(ar.i32()?, "generation count", name)?;
    ar.skip(gens * 8)?;
    let engine_version = |ar: &mut Archive<'_>| -> Result<()> {
        ar.u16()?;
        ar.u16()?;
        ar.u16()?;
        ar.u32()?;
        ar.fstring()?;
        Ok(())
    };
    engine_version(ar)?;
    if file_version_ue4 >= 444 {
        engine_version(ar)?;
    }
    ar.u32()?; // compression flags
    if ar.i32()? != 0 {
        return Err(ReadError::parse(format!("{name}: compressed package chunks are not supported")));
    }
    ar.u32()?; // package source
    let extra = ar.i32()?;
    for _ in 0..extra {
        ar.fstring()?;
    }
    if legacy > -7 && ar.i32()? != 0 {
        return Err(ReadError::parse(format!("{name}: texture allocations are not supported")));
    }
    ar.i32()?; // asset registry data
    let bulk_data_start_offset = ar.i64()?;
    ar.i32()?; // world tile info
    let chunks = non_negative(ar.i32()?, "chunk id count", name)?;
    ar.skip(chunks * 4)?;
    let preload_dependency_count = ar.i32()?;
    let preload_dependency_offset = ar.i32()?;
    Ok(Summary {
        legacy_version: legacy,
        file_version_ue4,
        licensee,
        custom_versions,
        total_header_size,
        package_flags,
        name_count,
        name_offset,
        export_count,
        export_offset,
        import_count,
        import_offset,
        bulk_data_start_offset,
        preload_dependency_count,
        preload_dependency_offset,
    })
}

/// Name text of an FName (index, number): `Name` or `Name_<number - 1>`; `None` for an index outside `names`.
pub fn name_text(names: &[String], index: i32, number: i32) -> Option<String> {
    let base = usize::try_from(index).ok().and_then(|i| names.get(i))?;
    Some(if number == 0 { base.clone() } else { format!("{base}_{}", i64::from(number) - 1) })
}

/// An import: an object of another package (or of native code, `/Script/...`).
#[derive(Clone, Debug)]
pub struct Import {
    pub class_package: String,
    pub class_name: String,
    /// Package index of the outer, UE's sign convention (0 none, < 0 import, > 0 export).
    pub outer: i32,
    pub object_name: String,
}

/// An export entry.
#[derive(Clone, Debug)]
pub struct Export {
    /// Package indexes, UE's sign convention.
    pub class_index: i32,
    pub super_index: i32,
    pub template_index: i32,
    pub outer: i32,
    pub object_name: String,
    pub object_flags: u32,
    /// Absolute offset in the package (the .uasset's length is where the .uexp starts).
    pub serial_offset: i64,
    pub serial_size: i64,
}

/// One package's header.
pub struct Package {
    /// The pak path without extension: `MultiVersus/Content/A/B`.
    pub path: String,
    /// Its object path: `/Game/A/B`.
    pub object_path: String,
    pub summary: Summary,
    pub names: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub preload: Vec<i32>,
    /// The .uasset's length: the .uexp's bytes are read at `serial_offset - uasset_len`.
    pub uasset_len: usize,
    files: Arc<dyn FileSource>,
    /// The .uexp, read on the first ask.
    uexp: OnceLock<std::result::Result<Box<[u8]>, ReadError>>,
    /// The .uasset's bytes, kept only when the exports are in it (no .uexp).
    own_data: Option<Box<[u8]>>,
    /// What a cache holds of it (its resolved imports, its objects): reached from here without a lookup, never
    /// owned (the cache owns it; a trim drops it and a later ask adopts the package again).
    held: RwLock<Weak<Held>>,
}

impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Package({})", self.path)
    }
}

impl Package {
    /// Parses a package's header from its .uasset bytes; the .uexp is read from `files` on the first export read.
    /// `path`: the pak path without extension; `object_path`: its object path.
    pub fn parse(uasset: &[u8], path: &str, object_path: &str, files: Arc<dyn FileSource>) -> Result<Package> {
        let file = format!("{path}.uasset");
        let mut ar = Archive::new(uasset, &file);
        let summary = read_summary(&mut ar)?;
        let s = &summary;
        if s.package_flags & PKG_UNVERSIONED_PROPERTIES != 0 {
            return Err(ReadError::parse(format!("{file}: unversioned properties are not supported")));
        }
        let fname = |ar: &mut Archive<'_>, names: &[String]| -> Result<String> {
            let i = ar.i32()?;
            let n = ar.i32()?;
            name_text(names, i, n)
                .ok_or_else(|| ReadError::parse(format!("{file}: FName index {i} out of range {}", names.len())))
        };
        let mut names = Vec::with_capacity(s.name_count.max(0) as usize);
        ar.pos = non_negative(s.name_offset, "name offset", &file)?;
        for _ in 0..s.name_count {
            names.push(ar.fstring()?);
            if s.file_version_ue4 >= 504 {
                ar.skip(4)?; // hashes
            }
        }
        let mut imports = Vec::with_capacity(s.import_count.max(0) as usize);
        ar.pos = non_negative(s.import_offset, "import offset", &file)?;
        for _ in 0..s.import_count {
            let class_package = fname(&mut ar, &names)?;
            let class_name = fname(&mut ar, &names)?;
            let outer = ar.i32()?;
            let object_name = fname(&mut ar, &names)?;
            imports.push(Import { class_package, class_name, outer, object_name });
        }
        let mut exports = Vec::with_capacity(s.export_count.max(0) as usize);
        ar.pos = non_negative(s.export_offset, "export offset", &file)?;
        for _ in 0..s.export_count {
            let class_index = ar.i32()?;
            let super_index = ar.i32()?;
            let template_index = if s.file_version_ue4 >= 508 { ar.i32()? } else { 0 };
            let outer = ar.i32()?;
            let object_name = fname(&mut ar, &names)?;
            let object_flags = ar.u32()?;
            let serial_size = ar.i64()?;
            let serial_offset = ar.i64()?;
            ar.skip(12)?; // forced export, not for client, not for server
            ar.skip(16 + 4)?; // package guid, package flags
            if s.file_version_ue4 >= 365 {
                ar.skip(4)?;
            }
            if s.file_version_ue4 >= 485 {
                ar.skip(4)?;
            }
            if s.file_version_ue4 >= 507 {
                ar.skip(20)?;
            }
            exports.push(Export {
                class_index,
                super_index,
                template_index,
                outer,
                object_name,
                object_flags,
                serial_offset,
                serial_size,
            });
        }
        let mut preload = Vec::new();
        if s.preload_dependency_count > 0 && s.preload_dependency_offset > 0 {
            ar.pos = non_negative(s.preload_dependency_offset, "preload offset", &file)?;
            for _ in 0..s.preload_dependency_count {
                preload.push(ar.i32()?);
            }
        }
        // The export offsets tell whether a .uexp is beside the .uasset: a
        // cooked package split in two has its exports past the .uasset's end.
        let split = exports.iter().all(|e| e.serial_size == 0 || e.serial_offset >= uasset.len() as i64);
        let (uasset_len, own_data) = if split { (uasset.len(), None) } else { (0, Some(uasset.into())) };
        Ok(Package {
            path: path.to_owned(),
            object_path: object_path.to_owned(),
            summary,
            names,
            imports,
            exports,
            preload,
            uasset_len,
            files,
            uexp: OnceLock::new(),
            own_data,
            held: RwLock::new(Weak::new()),
        })
    }

    /// The export data (the .uexp's bytes, read once; the .uasset's when the package has none).
    pub fn uexp(&self) -> Result<&[u8]> {
        if let Some(b) = &self.own_data {
            return Ok(b);
        }
        let r = self.uexp.get_or_init(|| {
            let file = format!("{}.uexp", self.path);
            match self.files.read_file(&file) {
                Ok(Some(b)) => Ok(b.into()),
                Ok(None) => Err(ReadError::parse(format!("{file}: no such file"))),
                Err(e) => Err(e),
            }
        });
        r.as_deref().map_err(Clone::clone)
    }

    /// The heap bytes its header holds (its names, imports, exports and preload list, the .uasset's bytes when it keeps
    /// them), itself included: what the package cache counts against its budget for it.
    fn header_heap_bytes(&self) -> usize {
        let names: usize = self.names.iter().map(|n| alloc_bytes(n.capacity())).sum();
        let imports: usize = self
            .imports
            .iter()
            .map(|i| {
                alloc_bytes(i.class_package.capacity())
                    + alloc_bytes(i.class_name.capacity())
                    + alloc_bytes(i.object_name.capacity())
            })
            .sum();
        let exports: usize = self.exports.iter().map(|e| alloc_bytes(e.object_name.capacity())).sum();
        size_of::<Package>()
            + 2 * ALLOC_OVERHEAD
            + alloc_bytes(self.path.capacity())
            + alloc_bytes(self.object_path.capacity())
            + alloc_bytes(self.summary.custom_versions.capacity() * size_of::<(Guid, i32)>())
            + alloc_bytes(self.names.capacity() * size_of::<String>())
            + names
            + alloc_bytes(self.imports.capacity() * size_of::<Import>())
            + imports
            + alloc_bytes(self.exports.capacity() * size_of::<Export>())
            + exports
            + alloc_bytes(self.preload.capacity() * size_of::<i32>())
            + self.own_data.as_ref().map_or(0, |b| alloc_bytes(b.len()))
    }

    /// The .uexp's length when it has been read (the cache's budget).
    fn uexp_len_read(&self) -> usize {
        match self.uexp.get() {
            Some(Ok(b)) => b.len(),
            _ => 0,
        }
    }

    /// FName text: `names[index]`, or `names[index]_<number - 1>` for a number above 0.
    pub fn name_text(&self, index: i32, number: i32) -> Result<String> {
        name_text(&self.names, index, number).ok_or_else(|| {
            ReadError::parse(format!(
                "{}: FName could not be read, requested index {index}, name map size {}",
                self.path,
                self.names.len()
            ))
        })
    }

    /// The top-level export a name names: the last exact match, else the first without regard to case; `None` when
    /// there is none.
    pub fn top_level_export(&self, name: &str) -> Option<usize> {
        self.child_export(0, name)
    }

    /// The export whose outer is package index `outer` (0: a top-level export) and whose name is `name`: the last
    /// exact match, else the first without regard to case.
    pub fn child_export(&self, outer: i32, name: &str) -> Option<usize> {
        let mut it = self.exports.iter().enumerate().filter(|(_, e)| e.outer == outer);
        it.clone()
            .rev()
            .find(|(_, e)| e.object_name == name)
            .or_else(|| it.find(|(_, e)| e.object_name.to_lowercase() == name.to_lowercase()))
            .map(|(i, _)| i)
    }

    /// The class default object of a class export of this package: the top-level export
    /// named `Default__<class name>`, exact.
    pub fn class_default_of(&self, class_export: usize) -> Option<usize> {
        let want = format!("Default__{}", self.exports.get(class_export)?.object_name);
        self.exports.iter().position(|e| e.outer == 0 && e.object_name == want)
    }

    /// An import's index from a package index (< 0), an export's from one > 0; `None` for 0 or past the tables.
    pub fn import_at(&self, index: i32) -> Option<usize> {
        let i = usize::try_from(-i64::from(index) - 1).ok()?;
        (i < self.imports.len()).then_some(i)
    }

    pub fn export_at(&self, index: i32) -> Option<usize> {
        let i = usize::try_from(i64::from(index) - 1).ok()?;
        (i < self.exports.len()).then_some(i)
    }
}

/// A package index resolved (CUE4Parse's ResolvedObject).
#[derive(Clone)]
pub enum Resolved {
    /// An export of `pkg` (an import that resolved through its package is the export there).
    Export { pkg: Arc<Package>, index: usize },
    /// An import of `pkg` that did not resolve (native code, a package the paks do not hold, a name not found).
    Import { pkg: Arc<Package>, index: usize },
    /// A package itself: the outer of a top-level export.
    Package(Arc<Package>),
    /// A script class known by name alone.
    Script(String),
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Resolved::Export { pkg, index } => write!(f, "Export({} #{index})", pkg.path),
            Resolved::Import { pkg, index } => write!(f, "Import({} #{index})", pkg.path),
            Resolved::Package(pkg) => write!(f, "Package({})", pkg.path),
            Resolved::Script(n) => write!(f, "Script({n})"),
        }
    }
}

impl Resolved {
    /// Its object name.
    pub fn name(&self) -> &str {
        match self {
            Resolved::Export { pkg, index } => &pkg.exports[*index].object_name,
            Resolved::Import { pkg, index } => &pkg.imports[*index].object_name,
            Resolved::Package(pkg) => &pkg.path,
            Resolved::Script(n) => n,
        }
    }

    /// Its outer, resolved; `None` at the top (a package, the outermost import, a script class).
    pub fn outer(&self, cache: &PackageCache) -> Option<Resolved> {
        match self {
            Resolved::Export { pkg, index } => {
                Some(cache.resolve(pkg, pkg.exports[*index].outer).unwrap_or_else(|| Resolved::Package(pkg.clone())))
            }
            Resolved::Import { pkg, index } => cache.resolve(pkg, pkg.imports[*index].outer),
            _ => None,
        }
    }

    /// Its class, resolved (an export's class index; an import's class by its ClassPackage and ClassName).
    pub fn class(&self, cache: &PackageCache) -> Option<Resolved> {
        match self {
            Resolved::Export { pkg, index } => cache.resolve(pkg, pkg.exports[*index].class_index),
            Resolved::Import { pkg, index } => Some(Resolved::Script(pkg.imports[*index].class_name.clone())),
            _ => None,
        }
    }

    /// A class or struct export's parent (its super index), resolved; `None` for an import.
    pub fn super_struct(&self, cache: &PackageCache) -> Option<Resolved> {
        match self {
            Resolved::Export { pkg, index } => cache.resolve(pkg, pkg.exports[*index].super_index),
            _ => None,
        }
    }

    /// The object path the object kind writes for it: its package's object path and its
    /// outer chain's names (`/Game/A/B.B_C:Comp`); a script class its name alone.
    pub fn object_path(&self, cache: &PackageCache) -> String {
        if let Resolved::Script(n) = self {
            return n.clone();
        }
        let mut names: Vec<Resolved> = Vec::new();
        let mut top = self.clone();
        while let Some(o) = top.outer(cache) {
            names.push(top);
            top = o;
        }
        let pkg = match &top {
            // an export's outer chain ends at its package (`outer` gives the package for outer 0)
            Resolved::Package(p) => {
                assert!(!p.object_path.is_empty(), "typed: {} is under no mount", p.path);
                p.object_path.as_str()
            }
            // the outermost import: the package's own name, "/Game/..." or "/Script/Engine"
            r => r.name(),
        };
        let names: Vec<&str> = names.iter().rev().map(Resolved::name).collect();
        join_object_path(pkg, &names)
    }

    /// CUE4Parse's GetPathName(includeOuterMost) of it.
    pub fn path_name(&self, cache: &PackageCache, include_outermost: bool) -> String {
        let mut s = String::new();
        if let Some(outer) = self.outer(cache) {
            let outer2 = outer.outer(cache);
            if outer2.is_some() || include_outermost {
                s = outer.path_name(cache, include_outermost);
                let colon = outer2.is_some_and(|o2| o2.outer(cache).is_none());
                s.push(if colon { ':' } else { '.' });
            }
        }
        s.push_str(self.name());
        s
    }

    /// CUE4Parse's GetFullName(includeOuterMost): `Class'Path'` (FPackageIndex.ToString of a map key).
    pub fn full_name(&self, cache: &PackageCache, include_outermost: bool) -> String {
        let c = self.class(cache);
        format!("{}'{}'", c.as_ref().map_or("", Resolved::name), self.path_name(cache, include_outermost))
    }

    /// The export it is, if it is one: its package and index.
    pub fn as_export(&self) -> Option<(&Arc<Package>, usize)> {
        match self {
            Resolved::Export { pkg, index } => Some((pkg, *index)),
            _ => None,
        }
    }

    /// The export entry it is, if it is one.
    pub fn export(&self) -> Option<&Export> {
        self.as_export().map(|(p, i)| &p.exports[i])
    }

    /// The same object (the same package by pointer and the same index, or the same script name).
    pub fn same(&self, other: &Resolved) -> bool {
        match (self, other) {
            (Resolved::Export { pkg: a, index: i }, Resolved::Export { pkg: b, index: j })
            | (Resolved::Import { pkg: a, index: i }, Resolved::Import { pkg: b, index: j }) => {
                Arc::ptr_eq(a, b) && i == j
            }
            (Resolved::Package(a), Resolved::Package(b)) => Arc::ptr_eq(a, b),
            (Resolved::Script(a), Resolved::Script(b)) => a == b,
            _ => false,
        }
    }
}

/// What the cache holds of one package: its resolved imports and its objects, each set once.
struct Held {
    pkg: Arc<Package>,
    imports: Box<[OnceLock<Option<Resolved>>]>,
    objects: Box<[OnceLock<Arc<UObj>>]>,
    /// Whether its .uexp's bytes count in the budget yet.
    uexp_counted: AtomicBool,
}

impl Held {
    fn new(pkg: Arc<Package>) -> Arc<Held> {
        let held = Arc::new(Held {
            imports: (0..pkg.imports.len()).map(|_| OnceLock::new()).collect(),
            objects: (0..pkg.exports.len()).map(|_| OnceLock::new()).collect(),
            uexp_counted: AtomicBool::new(false),
            pkg,
        });
        *held.pkg.held.write().expect("package's held lock") = Arc::downgrade(&held);
        held
    }
}

/// One path of the cache: the package there, parsed once (`None`: the paks have none; an error: it is there and does
/// not parse, given to every asker).
#[derive(Default)]
struct Slot {
    cell: OnceLock<std::result::Result<Option<Arc<Held>>, ReadError>>,
}

/// An export being read: by its package (pointer) and index.
type ReadKey = (usize, usize);

/// The objects being read now, and who waits for which (a wait that would close a cycle reads the object itself).
#[derive(Default)]
struct InFlight {
    owner: HashMap<ReadKey, ThreadId>,
    waits: HashMap<ThreadId, ReadKey>,
}

thread_local! {
    /// The exports this thread is reading, innermost last.
    static READING: RefCell<Vec<ReadKey>> = const { RefCell::new(Vec::new()) };
}

/// Pops this thread's reading entry and frees an owned key, also when the read panics.
struct ReadGuard<'a> {
    cache: &'a PackageCache,
    key: ReadKey,
    owned: bool,
}

impl ReadGuard<'_> {
    fn enter(cache: &PackageCache, key: ReadKey, owned: bool) -> ReadGuard<'_> {
        READING.with(|r| r.borrow_mut().push(key));
        ReadGuard { cache, key, owned }
    }
}

impl Drop for ReadGuard<'_> {
    fn drop(&mut self) {
        READING.with(|r| r.borrow_mut().pop());
        if self.owned {
            let mut fl = self.cache.flight.lock().unwrap_or_else(|e| e.into_inner());
            fl.owner.remove(&self.key);
            drop(fl);
            self.cache.wake.notify_all();
        }
    }
}

/// Packages read from the paks, shared by every thread of a run. A package is parsed once; an export's object is
/// read once ([`PackageCache::object`]); an import is resolved once per package. Once the bytes held pass the
/// budget, [`PackageCache::trim`] drops everything (what a caller holds stays alive through its `Arc`s).
///
/// Reentrancy: reading an object may read others (a struct's fields, a parent class's defaults), on the same thread,
/// with no lock held. A thread asking again for an object it is reading reads it again, nested, and keeps that read
/// to itself: the outer read stores its result (its nested read is overwritten by
/// the outer one). A thread asking for an object another thread is reading waits for it, unless that thread waits
/// (through others) on one this thread reads: then it reads the object nested in the same way.
// Deviation: this cache drops everything at once when its budget is passed, where a cache could let go of the packages
// used longest ago until half its budget is held. What is read is the same either way.
pub struct PackageCache {
    files: Arc<dyn FileSource>,
    paths: Arc<BuildPaths>,
    budget: usize,
    /// Lower-cased pak path without extension -> its slot.
    map: Mutex<HashMap<String, Arc<Slot>>>,
    held: AtomicUsize,
    flight: Mutex<InFlight>,
    wake: Condvar,
}

impl PackageCache {
    /// `budget`: the bytes of package files and objects held before [`PackageCache::trim`] drops them.
    pub fn new(files: Arc<dyn FileSource>, paths: Arc<BuildPaths>, budget: usize) -> PackageCache {
        PackageCache {
            files,
            paths,
            budget,
            map: Mutex::new(HashMap::new()),
            held: AtomicUsize::new(0),
            flight: Mutex::new(InFlight::default()),
            wake: Condvar::new(),
        }
    }

    pub fn files(&self) -> &Arc<dyn FileSource> {
        &self.files
    }

    pub fn paths(&self) -> &BuildPaths {
        &self.paths
    }

    fn slot(&self, key: String) -> Arc<Slot> {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).entry(key).or_default().clone()
    }

    /// The package at a pak path without extension (`MultiVersus/Content/A/B`, looked up without regard to case;
    /// its .uasset or .umap), `None` when the paks hold none. A package that is there and does not parse is an
    /// error, to every asker.
    pub fn get(&self, pak_path: &str) -> Result<Option<Arc<Package>>> {
        let slot = self.slot(pak_path.to_lowercase());
        let r = slot.cell.get_or_init(|| self.load(pak_path));
        match r {
            Ok(h) => Ok(h.as_ref().map(|h| h.pkg.clone())),
            Err(e) => Err(e.clone()),
        }
    }

    fn load(&self, pak_path: &str) -> std::result::Result<Option<Arc<Held>>, ReadError> {
        for ext in mvs_pak::paths::PACKAGE_EXTENSIONS {
            let file = format!("{pak_path}{ext}");
            let Some(uasset) = self.files.read_file(&file)? else { continue };
            let spelled = self.files.spelling(&file);
            let path = spelled.get(..spelled.len().saturating_sub(ext.len())).unwrap_or(&spelled);
            let object_path = self.paths.package_path(path).unwrap_or_default();
            let pkg = Package::parse(&uasset, path, &object_path, self.files.clone())?;
            // the header, what the cache keeps of it (the slot, its key, the cells) and the map's entry
            let bytes = pkg.header_heap_bytes()
                + size_of::<Slot>()
                + size_of::<Held>()
                + 3 * ALLOC_OVERHEAD
                + alloc_bytes(pak_path.len())
                + alloc_bytes(pkg.imports.len() * size_of::<OnceLock<Option<Resolved>>>())
                + alloc_bytes(pkg.exports.len() * size_of::<OnceLock<Arc<UObj>>>())
                + size_of::<String>()
                + size_of::<Arc<Slot>>();
            self.held.fetch_add(bytes, Ordering::Relaxed);
            return Ok(Some(Held::new(Arc::new(pkg))));
        }
        Ok(None)
    }

    /// The package of an object path (`/Game/A/B`, `/Game/A/B.B`), through the mount table.
    pub fn package(&self, object_path: &str) -> Result<Option<Arc<Package>>> {
        match self.paths.package_file(object_path) {
            Some(f) => self.get(&f),
            None => Ok(None),
        }
    }

    /// What the cache holds of `pkg`: its own entry, or, once a trim dropped that, a new one adopting the package
    /// (`None` when another parse of the same path took its place).
    fn held_of(&self, pkg: &Arc<Package>) -> Option<Arc<Held>> {
        if let Some(h) = pkg.held.read().expect("package's held lock").upgrade() {
            return Some(h);
        }
        let slot = self.slot(pkg.path.to_lowercase());
        match slot.cell.get_or_init(|| Ok(Some(Held::new(pkg.clone())))) {
            Ok(Some(h)) if Arc::ptr_eq(&h.pkg, pkg) => Some(h.clone()),
            _ => None,
        }
    }

    /// Resolves a package index of `pkg` (UE's sign convention; 0 is `None`). An import whose package is there and
    /// does not parse panics here, naming both; [`PackageCache::try_resolve`] gives the error.
    pub fn resolve(&self, pkg: &Arc<Package>, index: i32) -> Option<Resolved> {
        self.try_resolve(pkg, index).unwrap_or_else(|e| panic!("{}: resolving package index {index}: {e}", pkg.path))
    }

    /// CUE4Parse's ResolvePackageIndex on a legacy package: an export of `pkg`, or an import resolved through the
    /// package it names (the export there of its name and outer path), else the import itself. Errors: a package an
    /// import names that is there and does not parse.
    pub fn try_resolve(&self, pkg: &Arc<Package>, index: i32) -> Result<Option<Resolved>> {
        if index > 0 {
            return Ok(pkg.export_at(index).map(|i| Resolved::Export { pkg: pkg.clone(), index: i }));
        }
        let Some(i) = pkg.import_at(index) else { return Ok(None) };
        let held = self.held_of(pkg);
        if let Some(r) = held.as_ref().and_then(|h| h.imports[i].get()) {
            return Ok(r.clone());
        }
        let r = self.resolve_import(pkg, i)?;
        Ok(match held {
            Some(h) => h.imports[i].get_or_init(|| r).clone(),
            None => r,
        })
    }

    fn resolve_import(&self, pkg: &Arc<Package>, i: usize) -> Result<Option<Resolved>> {
        let imp = &pkg.imports[i];
        let unresolved = Some(Resolved::Import { pkg: pkg.clone(), index: i });
        // the outermost import: the package
        let mut top = i;
        let mut steps = 0;
        loop {
            let o = pkg.imports[top].outer;
            if o == 0 {
                break;
            }
            steps += 1;
            match pkg.import_at(o) {
                Some(t) if o < 0 && steps <= pkg.imports.len() => top = t,
                _ => return Ok(unresolved),
            }
        }
        let pkg_name = &pkg.imports[top].object_name;
        if pkg_name.starts_with("/Script/") {
            return Ok(unresolved);
        }
        let mut other = match self.paths.package_file(pkg_name) {
            Some(f) => self.get(&f)?,
            None => None,
        };
        if other.is_none() {
            let cp = &pkg.imports[top].class_package;
            if let Some(f) = (!cp.is_empty()).then(|| self.paths.package_file(cp)).flatten() {
                other = self.get(&f)?;
            }
        }
        let Some(other) = other else { return Ok(unresolved) };
        let mut outer_path = None;
        let top_index = -i64::try_from(top).expect("import index") - 1;
        if top_index != i64::from(imp.outer) && imp.outer < 0 {
            let Some(ro) = self.try_resolve(pkg, imp.outer)? else { return Ok(unresolved) };
            outer_path = Some(ro.path_name(self, true));
        }
        for (j, e) in other.exports.iter().enumerate() {
            if e.object_name != imp.object_name {
                continue;
            }
            let op = self.try_resolve(&other, e.outer)?.map(|o| o.path_name(self, true));
            if op == outer_path {
                return Ok(Some(Resolved::Export { pkg: other.clone(), index: j }));
            }
        }
        Ok(unresolved)
    }

    /// Export `index` of `pkg`, read once (`objects::deserialize_object`). A read that failed is an object with
    /// what it read and its issues listed (CUE4Parse keeps what it read).
    pub fn object(&self, pkg: &Arc<Package>, index: usize) -> Arc<UObj> {
        assert!(
            index < pkg.exports.len(),
            "{}: export {index} past the export table ({})",
            pkg.path,
            pkg.exports.len()
        );
        let key: ReadKey = (Arc::as_ptr(pkg) as usize, index);
        let Some(held) = self.held_of(pkg) else { return self.read_nested(pkg, index) };
        let cell = &held.objects[index];
        if let Some(o) = cell.get() {
            return o.clone();
        }
        if self.is_reading(pkg, index) {
            return self.read_nested(pkg, index);
        }
        let me = thread::current().id();
        loop {
            if let Some(o) = cell.get() {
                return o.clone();
            }
            let mut fl = self.flight.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(o) = cell.get() {
                return o.clone();
            }
            let own = match fl.owner.get(&key) {
                None => {
                    fl.owner.insert(key, me);
                    true
                }
                Some(&t) => {
                    if !Self::waits_on(&fl, t, me) {
                        fl.waits.insert(me, key);
                        let mut fl = self.wake.wait(fl).unwrap_or_else(|e| e.into_inner());
                        fl.waits.remove(&me);
                        continue;
                    }
                    // the thread reading it waits, through others, on this one: read it here, nested
                    false
                }
            };
            drop(fl);
            if !own {
                return self.read_nested(pkg, index);
            }
            // the object is in its cell before the guard lets a waiter look again
            let guard = ReadGuard::enter(self, key, true);
            let o = Arc::new(deserialize_object(self, pkg, index));
            let o = cell.get_or_init(|| o).clone();
            drop(guard);
            let mut bytes = o.heap_bytes();
            if pkg.uexp_len_read() > 0 && !held.uexp_counted.swap(true, Ordering::Relaxed) {
                bytes += alloc_bytes(pkg.uexp_len_read());
            }
            self.held.fetch_add(bytes, Ordering::Relaxed);
            // a job that reads a great deal within itself (a character's entries) stays within the budget too
            self.trim();
            return o;
        }
    }

    /// Whether this thread is reading export `index` of `pkg` now (an outer read of it is on its stack): what a
    /// guard against reading a struct's fields inside its own read keys on.
    pub fn is_reading(&self, pkg: &Arc<Package>, index: usize) -> bool {
        let key: ReadKey = (Arc::as_ptr(pkg) as usize, index);
        READING.with(|r| r.borrow().contains(&key))
    }

    /// A read of an export that is not stored: one nested in a read of the same export, or one the cache cannot hold.
    fn read_nested(&self, pkg: &Arc<Package>, index: usize) -> Arc<UObj> {
        let _guard = ReadGuard::enter(self, (Arc::as_ptr(pkg) as usize, index), false);
        Arc::new(deserialize_object(self, pkg, index))
    }

    /// Whether thread `t` waits, through the threads reading what it waits on, on thread `me`.
    fn waits_on(fl: &InFlight, mut t: ThreadId, me: ThreadId) -> bool {
        for _ in 0..=fl.waits.len() {
            if t == me {
                return true;
            }
            match fl.waits.get(&t).and_then(|k| fl.owner.get(k)) {
                Some(&o) => t = o,
                None => return false,
            }
        }
        false
    }

    /// The export an object path names: `/Game/A/B.B` (a top-level export), `/Game/A/B.B_C:Comp` (an inner one).
    /// A bare package path names no export.
    pub fn find(&self, object_path: &str) -> Result<Option<Resolved>> {
        let (pkg_path, object) = split_object_path(object_path);
        let Some(object) = object else { return Ok(None) };
        let Some(pkg) = self.package(pkg_path)? else { return Ok(None) };
        let mut at: Option<usize> = None;
        for name in object.split([':', '.']) {
            let outer = at.map_or(0, |i| i32::try_from(i + 1).expect("export index"));
            match pkg.child_export(outer, name) {
                Some(i) => at = Some(i),
                None => return Ok(None),
            }
        }
        Ok(at.map(|index| Resolved::Export { pkg, index }))
    }

    /// A string table's entries by key (UStringTable), for an FText StringTableEntry history: the build's texts
    /// name none the reader can load, so always `None`.
    pub fn string_table(&self, table_id: &str) -> Option<&StringTable> {
        let _ = table_id;
        None
    }

    /// Drops what the cache holds when past its budget.
    pub fn trim(&self) {
        if self.held() > self.budget {
            let dropped = std::mem::take(&mut *self.map.lock().unwrap_or_else(|e| e.into_inner()));
            self.held.store(0, Ordering::Relaxed);
            drop(dropped);
        }
    }

    /// The bytes held now: the packages' headers and .uexp files and the objects read, as their heap allocations
    /// count them (`Package::header_heap_bytes`, `UObj::heap_bytes`; a resolved import is not counted).
    pub fn held(&self) -> usize {
        self.held.load(Ordering::Relaxed)
    }
}

/// A string table's entries: key -> source string.
pub type StringTable = indexmap::IndexMap<String, String>;
