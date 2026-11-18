//! The `skeleton` kind: every Skeleton's reference skeleton as JSON: bone names,
//! parent indices and the stored reference pose (`mvs_uasset::natives::skeleton::read_skeleton`), written by
//! [`skeleton_entry`], the one function a skeletal mesh's header writes its own `skeleton` with. Virtual bones,
//! sockets, slot groups and the BoneTree stay in the object kind. Covers every Skeleton of the build.

use std::sync::Arc;

use mvs_pak::paths::split_object_path;
use mvs_uasset::natives::skeleton::{ReferenceSkeleton, read_skeleton};
use mvs_uasset::typed::f32_dotnet;
use serde::Serialize;

use crate::json;
use crate::kind::{Cx, Item, Kind, KindError, Memo};

/// The kind's version.
pub const SKELETON_KIND_VERSION: u64 = 1;
const SKELETON: &str = "/Script/Engine.Skeleton";

/// A reference skeleton as the skeleton kind's entry and a skeletal mesh header's `skeleton` hold it
#[derive(Clone, Debug, Serialize)]
pub struct SkeletonEntry {
    pub path: String,
    pub bones: Vec<String>,
    pub parents: Vec<i32>,
    /// The pose's float32 values as the object kind writes floats (the shortest decimal that reads back to the same
    /// float32, `f32_dotnet`).
    pub pose: Vec<f64>,
}

/// The entry of a reference skeleton named `path`. A pose value that is not finite
/// fails it.
pub fn skeleton_entry(path: &str, s: &ReferenceSkeleton) -> Result<SkeletonEntry, KindError> {
    let mut pose = Vec::with_capacity(s.pose.len());
    for &v in &s.pose {
        if !v.is_finite() {
            let shown = if v.is_nan() {
                "NaN"
            } else if v > 0.0 {
                "Infinity"
            } else {
                "-Infinity"
            };
            return Err(KindError::Failed(format!("skeleton: {path}: a reference pose value is {shown}")));
        }
        pose.push(f32_dotnet(v));
    }
    Ok(SkeletonEntry { path: path.into(), bones: s.bones.clone(), parents: s.parents.clone(), pose })
}

/// The entry of the Skeleton at `path` (the kind's build without its bytes).
fn read_entry(cx: &Cx<'_>, path: &str) -> Result<SkeletonEntry, KindError> {
    let (pkg, i) = cx.read(path)?;
    let name = cx.class_of(&pkg, i)?;
    if name != SKELETON {
        return Err(KindError::NotFound(format!("skeleton: {path} is a {name}, not a {SKELETON}")));
    }
    let s = read_skeleton(cx.cache, &pkg, i)?;
    // the entry names the export, also when asked by its bare package path
    let named = format!("{}.{}", split_object_path(path).0, pkg.exports[i].object_name);
    skeleton_entry(&named, &s)
}

/// The run's entries, by path.
#[derive(Default)]
struct Entries(Memo<Result<Arc<SkeletonEntry>, KindError>>);

/// The skeleton kind's entry of `path` (its bone names, parents and pose), read from the paks once per run: what a
/// game kind read of the kind (the Court's bone names).
pub fn entry(cx: &Cx<'_>, path: &str) -> Result<Arc<SkeletonEntry>, KindError> {
    cx.shared.get_or_init(Entries::default).0.get(path, || read_entry(cx, path).map(Arc::new))
}

/// The `skeleton` kind.
pub struct Skeleton;

impl Kind for Skeleton {
    fn id(&self) -> &'static str {
        "skeleton"
    }

    fn version(&self) -> u64 {
        SKELETON_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(cx.catalog.paths_of(&[SKELETON]).into_iter().map(Item::new).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        Ok(json::to_vec(&read_entry(cx, &item.path)?)?)
    }
}
