//! The package reader against the oracle's reader's answers for the same packages: a folder of package files and,
//! beside each package the test checks, a `<package>.json` the oracle's reader wrote (its header, every import and
//! export index resolved: kind, package, index, object path, path names, class; each export's constructed type and
//! native chain), and `mounts.json` (the mount table). The folder holds the
//! build's data, so it is never committed: the test reads `MVS_PACKAGE_FIXTURES`, else the tree's
//! `.scratch/fixtures`, and passes with a line when there is none.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mvs_pak::BuildPaths;
use mvs_uasset::archive::Result;
use mvs_uasset::objects::construct_type;
use mvs_uasset::package::FileSource;
use mvs_uasset::{PackageCache, Resolved, classes};
use serde_json::Value as J;

/// Package files in a folder, by their pak path.
struct Folder(PathBuf);

impl FileSource for Folder {
    fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.0.join(path)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(mvs_uasset::ReadError::Pak(format!("{path}: {e}"))),
        }
    }
}

fn fixtures() -> Option<PathBuf> {
    let dir = std::env::var_os("MVS_PACKAGE_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/fixtures"));
    dir.join("mounts.json").is_file().then_some(dir)
}

fn jsons(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            jsons(&p, out);
        } else if p.extension().is_some_and(|x| x == "json") && p.file_name().is_some_and(|n| n != "mounts.json") {
            out.push(p);
        }
    }
}

/// Collects the differences of one value.
struct Diffs(Vec<String>);

impl Diffs {
    fn eq(&mut self, at: &str, rust: J, ts: &J) {
        if rust != *ts {
            self.0.push(format!("{at}: rust {rust} ts {ts}"));
        }
    }
}

fn res_json(cache: &PackageCache, r: Option<Resolved>) -> J {
    let Some(r) = r else { return J::Null };
    let (k, pkg, i) = match &r {
        Resolved::Export { pkg, index } => ("export", J::from(pkg.path.clone()), J::from(*index)),
        Resolved::Import { pkg, index } => ("import", J::from(pkg.path.clone()), J::from(*index)),
        Resolved::Package(pkg) => ("package", J::from(pkg.path.clone()), J::Null),
        Resolved::Script(_) => ("script", J::Null, J::Null),
    };
    serde_json::json!({
        "k": k,
        "pkg": pkg,
        "i": i,
        "name": r.name(),
        "path": r.object_path(cache),
        "pathName": r.path_name(cache, true),
        "pathNameShort": r.path_name(cache, false),
        "cls": r.class(cache).map(|c| c.object_path(cache)),
    })
}

#[test]
fn packages_match_the_ts_reader() {
    let Some(dir) = fixtures() else {
        println!("package fixtures: none (set MVS_PACKAGE_FIXTURES or write .scratch/fixtures); not checked");
        return;
    };
    let m: J = serde_json::from_slice(&std::fs::read(dir.join("mounts.json")).unwrap()).unwrap();
    // the plugin mounts (the project's two come from its name)
    let plugins: Vec<(String, String)> = m["mounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x["root"].as_str().unwrap(), x["dir"].as_str().unwrap()))
        .filter(|(root, _)| *root != "/Game/" && *root != "/Engine/")
        .map(|(root, dir)| (root.trim_matches('/').to_string(), dir.trim_end_matches("Content/").to_string()))
        .collect();
    let paths = Arc::new(BuildPaths::new(m["project"].as_str().unwrap(), &plugins).unwrap());
    let cache = PackageCache::new(Arc::new(Folder(dir.clone())), paths, usize::MAX);
    let mut files = Vec::new();
    jsons(&dir, &mut files);
    files.sort();
    let (mut n_imp, mut n_exp, mut n_found) = (0, 0, 0);
    let mut d = Diffs(Vec::new());
    for f in &files {
        let j: J = serde_json::from_slice(&std::fs::read(f).unwrap()).unwrap();
        let path = j["path"].as_str().unwrap();
        let pkg = cache.get(path).unwrap().unwrap_or_else(|| panic!("{path}: not found"));
        d.eq(&format!("{path} path"), J::from(pkg.path.clone()), &j["path"]);
        let s = &pkg.summary;
        let js = &j["summary"];
        for (k, v) in [
            ("legacyVersion", i64::from(s.legacy_version)),
            ("fileVersionUE4", i64::from(s.file_version_ue4)),
            ("licensee", i64::from(s.licensee)),
            ("totalHeaderSize", i64::from(s.total_header_size)),
            ("packageFlags", i64::from(s.package_flags)),
            ("nameCount", i64::from(s.name_count)),
            ("nameOffset", i64::from(s.name_offset)),
            ("exportCount", i64::from(s.export_count)),
            ("exportOffset", i64::from(s.export_offset)),
            ("importCount", i64::from(s.import_count)),
            ("importOffset", i64::from(s.import_offset)),
            ("bulkDataStartOffset", s.bulk_data_start_offset),
            ("preloadDependencyCount", i64::from(s.preload_dependency_count)),
            ("preloadDependencyOffset", i64::from(s.preload_dependency_offset)),
        ] {
            d.eq(&format!("{path} summary {k}"), J::from(v), &js[k]);
        }
        d.eq(
            &format!("{path} customVersions"),
            J::from(s.custom_versions.len()),
            &J::from(js["customVersions"].as_array().unwrap().len()),
        );
        d.eq(&format!("{path} names"), J::from(pkg.names.clone()), &j["names"]);
        d.eq(&format!("{path} preload"), J::from(pkg.preload.clone()), &j["preload"]);
        d.eq(&format!("{path} uassetLength"), J::from(pkg.uasset_len), &j["uassetLength"]);
        for (i, (im, ji)) in pkg.imports.iter().zip(j["imports"].as_array().unwrap()).enumerate() {
            n_imp += 1;
            let at = format!("{path} import {i}");
            d.eq(&format!("{at} classPackage"), J::from(im.class_package.clone()), &ji["classPackage"]);
            d.eq(&format!("{at} className"), J::from(im.class_name.clone()), &ji["className"]);
            d.eq(&format!("{at} outer"), J::from(im.outer), &ji["outer"]);
            d.eq(&format!("{at} objectName"), J::from(im.object_name.clone()), &ji["objectName"]);
            let idx = -i32::try_from(i).unwrap() - 1;
            d.eq(&format!("{at} res"), res_json(&cache, cache.resolve(&pkg, idx)), &ji["res"]);
        }
        d.eq(&format!("{path} imports"), J::from(pkg.imports.len()), &J::from(j["imports"].as_array().unwrap().len()));
        for (i, (e, je)) in pkg.exports.iter().zip(j["exports"].as_array().unwrap()).enumerate() {
            n_exp += 1;
            let at = format!("{path} export {i}");
            for (k, v) in [
                ("classIndex", i64::from(e.class_index)),
                ("superIndex", i64::from(e.super_index)),
                ("templateIndex", i64::from(e.template_index)),
                ("outer", i64::from(e.outer)),
                ("objectFlags", i64::from(e.object_flags)),
                ("serialOffset", e.serial_offset),
                ("serialSize", e.serial_size),
            ] {
                d.eq(&format!("{at} {k}"), J::from(v), &je[k]);
            }
            d.eq(&format!("{at} objectName"), J::from(e.object_name.clone()), &je["objectName"]);
            let idx = i32::try_from(i).unwrap() + 1;
            let r = cache.resolve(&pkg, idx);
            let op = r.as_ref().unwrap().object_path(&cache);
            d.eq(&format!("{at} res"), res_json(&cache, r), &je["res"]);
            let reg = construct_type(&cache, cache.resolve(&pkg, e.class_index).as_ref(), e.object_flags).unwrap();
            d.eq(&format!("{at} reg"), reg.map_or(J::Null, J::from), &je["reg"]);
            d.eq(&format!("{at} full"), J::from(classes::chain_of(reg).to_vec()), &je["full"]);
            // an object path names the export it was made from
            match cache.find(&op).unwrap() {
                Some(Resolved::Export { pkg: p, index }) if std::sync::Arc::ptr_eq(&p, &pkg) => {
                    n_found += 1;
                    let found = p.exports[index].object_name.clone();
                    d.eq(&format!("{at} find {op}"), J::from(found), &J::from(e.object_name.clone()));
                }
                other => d.0.push(format!("{at} find {op}: {other:?}")),
            }
        }
        d.eq(&format!("{path} exports"), J::from(pkg.exports.len()), &J::from(j["exports"].as_array().unwrap().len()));
    }
    println!(
        "package fixtures: {} packages, {n_imp} imports, {n_exp} exports compared, {n_found} found by path; {} differences",
        files.len(),
        d.0.len()
    );
    for x in d.0.iter().take(40) {
        println!("  {x}");
    }
    assert!(d.0.is_empty(), "{} differences from the oracle's reader", d.0.len());

    // Every import resolved from 8 threads at once over a fresh cache, each in its own order, gives the same paths.
    let pkg_paths: Vec<String> = files
        .iter()
        .map(|f| {
            let j: J = serde_json::from_slice(&std::fs::read(f).unwrap()).unwrap();
            j["path"].as_str().unwrap().to_owned()
        })
        .collect();
    let resolve_all = |cache: &PackageCache, order: &[String]| -> Vec<(String, String)> {
        let mut out = Vec::new();
        for p in order {
            let pkg = cache.get(p).unwrap().unwrap();
            for i in 0..pkg.imports.len() {
                let r = cache.resolve(&pkg, -i32::try_from(i).unwrap() - 1).unwrap();
                out.push((format!("{p} {i}"), r.object_path(cache)));
            }
        }
        out.sort();
        out
    };
    let want = resolve_all(&cache, &pkg_paths);
    let shared = PackageCache::new(cache.files().clone(), Arc::new(cache.paths().clone()), usize::MAX);
    let got: Vec<_> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..8)
            .map(|t| {
                let mut order = pkg_paths.clone();
                let n = order.len();
                order.rotate_left(t * n / 8);
                if t % 2 == 1 {
                    order.reverse();
                }
                let shared = &shared;
                s.spawn(move || resolve_all(shared, &order))
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(got.iter().all(|g| *g == want), "threads resolved differently");

    // Past its budget a trim drops everything; a package held from before resolves through it again.
    let small = PackageCache::new(cache.files().clone(), Arc::new(cache.paths().clone()), 1);
    let before = small.get(&pkg_paths[0]).unwrap().unwrap();
    assert!(small.held() > 1);
    small.trim();
    assert_eq!(small.held(), 0);
    let mut old = Vec::new();
    for i in 0..before.imports.len() {
        let r = small.resolve(&before, -i32::try_from(i).unwrap() - 1).unwrap();
        old.push((format!("{} {i}", pkg_paths[0]), r.object_path(&small)));
    }
    let after = small.get(&pkg_paths[0]).unwrap().unwrap();
    assert!(Arc::ptr_eq(&before, &after), "the package held from before is adopted again");
    old.sort();
    let first: Vec<_> = want.iter().filter(|(k, _)| k.starts_with(&format!("{} ", pkg_paths[0]))).cloned().collect();
    assert_eq!(first, old);
}
