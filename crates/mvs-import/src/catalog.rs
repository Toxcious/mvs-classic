//! The catalog: every package of the paks (.uasset / .umap under a mount) and its top-level exports with their
//! classes, from the headers alone. Every kind's list reads it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use indexmap::IndexMap;
use mvs_pak::paths::join_object_path;
use mvs_pak::{BuildPaths, PakSet, cmp_utf16};
use mvs_uasset::Package;
use mvs_uasset::package::FileSource;
use rayon::prelude::*;
use serde::Serialize;

use crate::kind::{Failure, Phase, Progress};

/// Bumped by every change to what the catalog holds.
pub const CATALOG_VERSION: u32 = 1;

/// Every package of the build with its top-level exports: `[package path, [[export name, class index], ...]]` in
/// package path order, and the class paths the indexes name, numbered in package order. Serialized as the store's
/// catalog file is.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Catalog {
    pub classes: Vec<String>,
    pub packages: Vec<(String, Vec<(String, u32)>)>,
}

/// A top-level export of the build: its package's object path, its name and its class's object path.
#[derive(Clone, Copy, Debug)]
pub struct CatalogExport<'a> {
    pub package: &'a str,
    pub name: &'a str,
    pub class: &'a str,
}

impl CatalogExport<'_> {
    /// Its object path, `<package>.<export>`.
    pub fn object_path(&self) -> String {
        format!("{}.{}", self.package, self.name)
    }
}

impl Catalog {
    /// Every top-level export, in package order then export order.
    pub fn exports(&self) -> impl Iterator<Item = CatalogExport<'_>> {
        self.packages.iter().flat_map(move |(pkg, exports)| {
            exports.iter().map(move |(name, ci)| CatalogExport {
                package: pkg,
                name,
                class: &self.classes[*ci as usize],
            })
        })
    }

    /// The object paths (`<package>.<export>`) of the top-level exports whose class is one of `classes`, in catalog
    /// order: the list of most kinds.
    pub fn paths_of(&self, classes: &[&str]) -> Vec<String> {
        self.exports().filter(|e| classes.contains(&e.class)).map(|e| e.object_path()).collect()
    }

    /// The packages under `folder` (`/Game/Panda_Main/Cosmetics/RingOuts`) as bare object paths: every package there,
    /// `recursive` adding the subfolders', `class` keeping those whose asset (the export named as the package, exact
    /// name first, then without regard to case) is of that class exactly. Ordered by the path upper-cased as an object
    /// path (`<p>.<name>`, code units: the build's asset registry, PGI_GlobalData GetAllProfileIcons 141152590), ties
    /// by the path itself. An unknown folder lists nothing.
    pub fn list_packages(&self, folder: &str, recursive: bool, class: Option<&str>) -> Vec<String> {
        let prefix = folder.to_lowercase() + "/";
        let class_index = match class {
            Some(c) => match self.classes.iter().position(|k| k == c) {
                Some(i) => Some(i as u32),
                None => return Vec::new(),
            },
            None => None,
        };
        let mut out: Vec<(String, &str)> = Vec::new();
        for (p, exports) in &self.packages {
            let low = p.to_lowercase();
            if !low.starts_with(&prefix) || !recursive && low[prefix.len()..].contains('/') {
                continue;
            }
            if let Some(ci) = class_index {
                let name = &p[p.rfind('/').map_or(0, |s| s + 1)..];
                let lname = name.to_lowercase();
                let hit = exports
                    .iter()
                    .find(|e| e.0 == name)
                    .or_else(|| exports.iter().find(|e| e.0.to_lowercase() == lname));
                if hit.is_none_or(|e| e.1 != ci) {
                    continue;
                }
            }
            out.push(((p.clone() + "." + &p[p.rfind('/').map_or(0, |s| s + 1)..]).to_uppercase(), p));
        }
        out.sort_by(|a, b| cmp_utf16(&a.0, &b.0).then_with(|| cmp_utf16(a.1, b.1)));
        out.into_iter().map(|(_, p)| p.to_owned()).collect()
    }

    /// The top-level exports.
    pub fn len(&self) -> usize {
        self.packages.iter().map(|p| p.1.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.packages.iter().all(|p| p.1.is_empty())
    }
}

/// The class of an export as an object path from the header alone (for a package whose
/// imports resolve to nothing, as the catalog is read without a store): an export class by its outer chain in this
/// package, an import by the import chain (`/Game/A/BP.BP_C`, `/Script/Engine.FontFace`). `None` for index 0 or one
/// past the tables (`/Script/CoreUObject.Object` then).
pub fn header_object_path(pkg: &Package, index: i32) -> Option<String> {
    // (name, outer) of a package index
    let entry = |i: i32| -> Option<(&str, i32)> {
        if i > 0 {
            pkg.export_at(i).map(|j| (pkg.exports[j].object_name.as_str(), pkg.exports[j].outer))
        } else {
            pkg.import_at(i).map(|j| (pkg.imports[j].object_name.as_str(), pkg.imports[j].outer))
        }
    };
    let (mut name, mut outer) = entry(index)?;
    let mut cur = index;
    let mut names: Vec<&str> = Vec::new();
    // an export's chain ends at its package (outer 0 or one past the tables); an import's at its outermost import
    let limit = pkg.exports.len() + pkg.imports.len() + 1;
    let top = loop {
        if names.len() > limit {
            break name; // a loop in the outer chain (it would not end)
        }
        match entry(outer) {
            Some((n, o)) => {
                names.push(name);
                cur = outer;
                name = n;
                outer = o;
            }
            None if cur > 0 => {
                names.push(name);
                break pkg.object_path.as_str();
            }
            None => break name,
        }
    };
    names.reverse();
    Some(join_object_path(top, &names))
}

/// One package's top-level exports and their class object paths, from its header alone.
fn header_exports(pkg: &Package) -> Vec<(String, String)> {
    pkg.exports
        .iter()
        .filter(|e| e.outer == 0)
        .map(|e| {
            let class = header_object_path(pkg, e.class_index).unwrap_or_else(|| "/Script/CoreUObject.Object".into());
            (e.object_name.clone(), class)
        })
        .collect()
}

/// Scans every package header of the paks on the current rayon pool (the catalog is the same whatever the thread
/// order). A package whose header does not parse is a failure of kind `catalog`, its exports none.
pub fn scan_catalog(
    paks: &Arc<PakSet>,
    paths: &BuildPaths,
    progress: &(dyn Fn(Progress<'_>) + Sync),
) -> (Catalog, Vec<Failure>) {
    let mut files: Vec<(&str, String)> = paks
        .paths()
        .filter_map(|f| {
            let low = f.to_ascii_lowercase();
            if !low.ends_with(".uasset") && !low.ends_with(".umap") {
                return None;
            }
            Some((f, paths.package_path(f)?))
        })
        .collect();
    files.sort_by(|a, b| cmp_utf16(&a.1, &b.1));
    let total = files.len() as u64;
    let done = AtomicU64::new(0);
    progress(Progress { phase: Phase::Catalog, kind: None, done: 0, total, bytes: 0 });
    let source: Arc<dyn FileSource> = paks.clone();
    let got: Vec<Result<Vec<(String, String)>, String>> = files
        .par_iter()
        .map(|(file, object_path)| {
            let r = paks.read(file).map_err(|e| e.to_string()).and_then(|uasset| {
                let ext = if file.to_ascii_lowercase().ends_with(".umap") { 5 } else { 7 };
                let path = &file[..file.len() - ext];
                Package::parse(&uasset, path, object_path, source.clone())
                    .map(|p| header_exports(&p))
                    .map_err(|e| e.to_string())
            });
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(256) || n == total {
                progress(Progress { phase: Phase::Catalog, kind: None, done: n, total, bytes: 0 });
            }
            r
        })
        .collect();
    let mut classes: Vec<String> = Vec::new();
    let mut class_ix: IndexMap<String, u32> = IndexMap::new();
    let mut failures = Vec::new();
    let mut packages = Vec::with_capacity(files.len());
    for ((_, path), g) in files.iter().zip(got) {
        let mut exports = Vec::new();
        match g {
            Err(message) => failures.push(Failure { kind: "catalog".into(), path: path.clone(), message }),
            Ok(list) => {
                for (name, cls) in list {
                    let ci = match class_ix.get(&cls) {
                        Some(&ci) => ci,
                        None => {
                            let ci = u32::try_from(classes.len()).expect("classes under 2^32");
                            classes.push(cls.clone());
                            class_ix.insert(cls, ci);
                            ci
                        }
                    };
                    exports.push((name, ci));
                }
            }
        }
        packages.push((path.clone(), exports));
    }
    (Catalog { classes, packages }, failures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_packages() {
        let c = Catalog {
            classes: vec!["/Script/A.Tex".into(), "/Script/A.Mat".into()],
            packages: [
                ("/Game/R/BranchGroup_BatmanBase", vec![("BranchGroup_BatmanBase", 0)]),
                ("/Game/R/BranchGroup_BatmanBase+Neutral1", vec![("branchgroup_batmanbase+neutral1", 0)]),
                ("/Game/R/b", vec![("b", 1)]),
                ("/Game/R/Sub/C", vec![("C", 0)]),
                ("/Game/RX/D", vec![("D", 0)]),
                ("/Game/R/E", vec![("Other", 0)]),
            ]
            .into_iter()
            .map(|(p, e)| (p.to_owned(), e.into_iter().map(|(n, i)| (n.to_owned(), i)).collect()))
            .collect(),
        };
        let all = c.list_packages("/game/r", false, None);
        assert_eq!(
            all,
            ["/Game/R/b", "/Game/R/BranchGroup_BatmanBase+Neutral1", "/Game/R/BranchGroup_BatmanBase", "/Game/R/E"]
        );
        assert_eq!(c.list_packages("/Game/R", true, None).len(), 5);
        assert_eq!(
            c.list_packages("/Game/R", true, Some("/Script/A.Tex")),
            ["/Game/R/BranchGroup_BatmanBase+Neutral1", "/Game/R/BranchGroup_BatmanBase", "/Game/R/Sub/C"]
        );
        assert!(c.list_packages("/Game/R", true, Some("/Script/A.None")).is_empty());
    }
}
