//! Build object paths and pak file paths, both ways, in one place.
//!
//! A build object path is the name the build gives an object: `/<mount>/<folders>/<package>`, then `.<object>` and
//! for a subobject `:<sub>` and `.<subsub>` (UE's FSoftObjectPath form: `/Game/A/B.B`, `/Game/A/B.B_C:Comp.Inner`).
//! A pak file path is where the paks hold the package: `MultiVersus/Content/A/B.uasset`.
//!
//! The mount table is the one UE 4.26 registers (FPackageName::RegisterMountPoint): `/Game/` on the project's Content
//! folder, `/Engine/` on Engine/Content, and `/<Plugin>/` on `<plugin folder>/Content/` for every plugin whose
//! descriptor has CanContainContent (the mount named by the .uplugin file's base name). `/Script/<Module>` names
//! native code: no file.

use std::collections::HashMap;

use crate::{PakError, PakSet, Result};

/// The extensions a package's header file has in the paks: a map's is .umap.
pub const PACKAGE_EXTENSIONS: [&str; 2] = [".uasset", ".umap"];

/// The file extensions [`BuildPaths::package_path`] drops (a package's files).
const PACKAGE_FILE_EXTENSIONS: [&str; 5] = [".uasset", ".umap", ".uexp", ".ubulk", ".ufont"];

/// One mount: `/Game/` on `MultiVersus/Content/`.
#[derive(Clone, Debug)]
pub struct Mount {
    /// With both slashes: `/Game/`.
    pub root: String,
    /// Its folder in the paks, with a trailing slash.
    pub dir: String,
}

/// The mount table of the paks.
#[derive(Clone, Debug)]
pub struct BuildPaths {
    pub project: String,
    /// Longest folder first, so a lookup takes the most specific mount.
    pub mounts: Vec<Mount>,
    /// Lower-cased root -> its place in `mounts`.
    by_root: HashMap<String, usize>,
}

fn paths_err(why: String) -> PakError {
    PakError::Format(std::path::PathBuf::from("paths"), why)
}

/// UE's JSON reader takes a trailing comma before `}` or `]`: drops each (outside strings).
fn drop_trailing_commas(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in text.char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == ',' {
            let next = b[i + 1..].iter().find(|x| !x.is_ascii_whitespace());
            if matches!(next, Some(b'}') | Some(b']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

impl BuildPaths {
    /// The table for `project` and its content plugins (`(name, folder)`).
    pub fn new(project: &str, plugins: &[(String, String)]) -> Result<BuildPaths> {
        if project.is_empty() || !project.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            return Err(paths_err(format!("project name \"{project}\" is not a plain name")));
        }
        let mut list = vec![
            Mount { root: "/Game/".into(), dir: format!("{project}/Content/") },
            Mount { root: "/Engine/".into(), dir: "Engine/Content/".into() },
        ];
        for (name, dir) in plugins {
            let dir = if dir.ends_with('/') { dir.clone() } else { format!("{dir}/") };
            list.push(Mount { root: format!("/{name}/"), dir: format!("{dir}Content/") });
        }
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (i, m) in list.iter().enumerate() {
            let k = m.root.to_lowercase();
            if let Some(&j) = seen.get(&k) {
                return Err(paths_err(format!("mount {} is registered twice ({}, {})", m.root, list[j].dir, m.dir)));
            }
            seen.insert(k, i);
        }
        // a stable sort by folder length in UTF-16 units, longest first (a stable sort: ties keep their order)
        list.sort_by_key(|m| std::cmp::Reverse(m.dir.encode_utf16().count()));
        let by_root = list.iter().enumerate().map(|(i, m)| (m.root.to_lowercase(), i)).collect();
        Ok(BuildPaths { project: project.to_string(), mounts: list, by_root })
    }

    /// The mount table of the paks: the project from its `<P>/<P>.uproject` entry, the plugins that can hold content
    /// from their `.uplugin` entries (each descriptor read; UE's reader takes a trailing comma).
    ///
    /// Unverified: UE mounts a plugin's content only when the plugin is enabled; every plugin whose descriptor says
    /// CanContainContent is mounted here, enabled or not.
    pub fn from_paks(paks: &PakSet) -> Result<BuildPaths> {
        let projects: Vec<&str> = paks
            .paths()
            .filter(|p| {
                let Some((dir, file)) = p.split_once('/') else { return false };
                !dir.is_empty()
                    && file.len() == dir.len() + ".uproject".len()
                    && file.is_char_boundary(dir.len())
                    && file[..dir.len()].eq_ignore_ascii_case(dir)
                    && file[dir.len()..].eq_ignore_ascii_case(".uproject")
            })
            .collect();
        if projects.len() != 1 {
            return Err(paths_err(format!("the paks hold {} <Project>/<Project>.uproject entries", projects.len())));
        }
        let project = projects[0].split('/').next().unwrap_or_default().to_string();
        let mut plugins = Vec::new();
        for p in paks.paths() {
            if !p.to_lowercase().ends_with(".uplugin") {
                continue;
            }
            let slash = p.rfind('/').map_or(0, |s| s + 1);
            let name = &p[slash..p.len() - ".uplugin".len()];
            let bytes = paks.read(p)?;
            let text = String::from_utf8_lossy(&bytes);
            let text = drop_trailing_commas(text.strip_prefix('\u{feff}').unwrap_or(&text));
            let desc: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| paths_err(format!("{p} is not JSON ({e})")))?;
            if desc.get("CanContainContent") == Some(&serde_json::Value::Bool(true)) {
                plugins.push((name.to_string(), p[..slash].to_string()));
            }
        }
        BuildPaths::new(&project, &plugins)
    }

    /// The pak path of an object path's package, without extension (`MultiVersus/Content/A/B`); `None` for a path
    /// under no mount (`/Script/Engine`). The object part after '.' is ignored.
    pub fn package_file(&self, object_path: &str) -> Option<String> {
        let (pkg, _) = split_object_path(object_path);
        if !pkg.starts_with('/') {
            return None;
        }
        let second = pkg[1..].find('/')? + 1;
        let m = &self.mounts[*self.by_root.get(&pkg[..=second].to_lowercase())?];
        let rest = &pkg[second + 1..];
        if rest.is_empty() { None } else { Some(format!("{}{rest}", m.dir)) }
    }

    /// The object path of the package a pak file holds (`MultiVersus/Content/A/B.uasset`, or without extension ->
    /// `/Game/A/B`); `None` for a file under no mount's folder.
    pub fn package_path(&self, file: &str) -> Option<String> {
        let base = PACKAGE_FILE_EXTENSIONS
            .iter()
            .find(|e| {
                file.len() >= e.len() && file.as_bytes()[file.len() - e.len()..].eq_ignore_ascii_case(e.as_bytes())
            })
            .map_or(file, |e| &file[..file.len() - e.len()]);
        let low = base.to_lowercase();
        for m in &self.mounts {
            if low.starts_with(&m.dir.to_lowercase()) && base.len() > m.dir.len() {
                return Some(format!("{}{}", m.root, &base[m.dir.len()..]));
            }
        }
        None
    }
}

/// Splits an object path into its package and the object part after the first '.' of its last segment:
/// `/Game/A/B.B_C:Comp` -> (`/Game/A/B`, Some(`B_C:Comp`)).
pub fn split_object_path(path: &str) -> (&str, Option<&str>) {
    let from = path.rfind('/').map_or(0, |s| s + 1);
    match path[from..].find('.') {
        Some(d) => (&path[..from + d], Some(&path[from + d + 1..])),
        None => (path, None),
    }
}

/// An object path from its package and its outer chain's names, outermost first:
/// (`/Game/A/B`, [`B_C`, `Comp`, `Inner`]) -> `/Game/A/B.B_C:Comp.Inner`.
pub fn join_object_path(pkg: &str, names: &[&str]) -> String {
    let mut s = pkg.to_string();
    for (i, n) in names.iter().enumerate() {
        s.push(if i == 1 { ':' } else { '.' });
        s.push_str(n);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_paths() {
        assert_eq!(split_object_path("/Game/A/B.B_C:Comp"), ("/Game/A/B", Some("B_C:Comp")));
        assert_eq!(split_object_path("/Game/A.x/B"), ("/Game/A.x/B", None));
        assert_eq!(split_object_path("/Game/A/B."), ("/Game/A/B", Some("")));
        assert_eq!(join_object_path("/Game/A/B", &["B_C", "Comp", "Inner"]), "/Game/A/B.B_C:Comp.Inner");
        assert_eq!(join_object_path("/Game/A/B", &[]), "/Game/A/B");
    }

    #[test]
    fn mounts() {
        let p = BuildPaths::new(
            "MultiVersus",
            &[
                ("Niagara".into(), "Engine/Plugins/FX/Niagara/".into()),
                ("Wbpc".into(), "MultiVersus/Plugins/wbpc".into()),
            ],
        )
        .unwrap();
        assert_eq!(p.mounts[0].root, "/Niagara/");
        assert_eq!(p.package_file("/Game/A/B.B").as_deref(), Some("MultiVersus/Content/A/B"));
        assert_eq!(p.package_file("/game/A/B").as_deref(), Some("MultiVersus/Content/A/B"));
        assert_eq!(p.package_file("/Script/Engine.Actor"), None);
        assert_eq!(p.package_file("/Game/"), None);
        assert_eq!(p.package_file("/Wbpc/X").as_deref(), Some("MultiVersus/Plugins/wbpc/Content/X"));
        assert_eq!(p.package_path("MultiVersus/Content/A/B.uasset").as_deref(), Some("/Game/A/B"));
        assert_eq!(p.package_path("multiversus/content/A/B.UMAP").as_deref(), Some("/Game/A/B"));
        assert_eq!(p.package_path("Engine/Plugins/FX/Niagara/Content/N.uexp").as_deref(), Some("/Niagara/N"));
        assert_eq!(p.package_path("MultiVersus/Content/"), None);
        assert_eq!(p.package_path("Other/X.uasset"), None);
        assert!(BuildPaths::new("Multi Versus", &[]).is_err());
        assert!(BuildPaths::new("G", &[("Game".into(), "x".into())]).is_err());
    }

    #[test]
    fn trailing_commas() {
        assert_eq!(drop_trailing_commas("{\"a\": [1, 2, ], \"b\": \",}\",\n}"), "{\"a\": [1, 2 ], \"b\": \",}\"\n}");
    }
}
