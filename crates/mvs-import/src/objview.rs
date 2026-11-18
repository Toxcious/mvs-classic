//! An object of the build as the game kinds' binds read it: a view over the run's package cache (the export, its read
//! object, its path and class), what an earlier bind loaded as the `object` kind's AssetObject, read on demand and
//! never held past the bind. The value helpers give what an earlier bind saw of a value in the object kind's JSON.

use std::sync::Arc;

use mvs_uasset::natives;
use mvs_uasset::objects::{IssueKind, RF_CLASS_DEFAULT_OBJECT, UObj};
use mvs_uasset::typed::{class_defaults, f32_dotnet, member_name};
use mvs_uasset::value::{Props, Value};
use mvs_uasset::{Package, PackageCache, Resolved};

use crate::kind::{Cx, KindError};

/// Inner classes the object kind leaves out, with every export below them. A UFunction: a Blueprint class's
/// functions, which hold their parameter list only (the kind reads no bytecode). A MovieSceneCompiledData: a widget
/// animation's compiled evaluation templates, whose polymorphic template pointers (MovieSceneEvalTemplatePtr,
/// MovieSceneTrackImplementationPtr) and id structs the reader does not port; the animation itself (its MovieScene,
/// tracks and sections) stays. A ClothingAssetCommon (and its cloth config inners): a skeletal mesh's cloth
/// simulation data, whose LodData is a native struct the reader does not port (ClothLODDataCommon); the port
/// simulates no cloth.
pub const LEFT_OUT: &[&str] = &[
    "/Script/CoreUObject.Function",
    "/Script/MovieScene.MovieSceneCompiledData",
    "/Script/ClothingSystemRuntimeCommon.ClothingAssetCommon",
];

/// The exports inside top-level export `top` of `pkg` (their outer chain reaches it, no export on the way of a class
/// in [`LEFT_OUT`]), in export order, each with its name chain below `top` (`PersistentLevel.Floor.Box`): the
/// object kind's `inner`.
pub fn inner_exports(cache: &PackageCache, pkg: &Arc<Package>, top: usize) -> Vec<(usize, String)> {
    // per export: None not yet walked, Some(None) not inside, Some(Some(chain)) inside
    let mut chains: Vec<Option<Option<String>>> = vec![None; pkg.exports.len()];
    chains[top] = Some(Some(String::new()));
    let mut out = Vec::new();
    for i in 0..pkg.exports.len() {
        if i == top {
            continue;
        }
        if let Some(name) = chain_of(cache, pkg, i, &mut chains, 0) {
            out.push((i, name));
        }
    }
    out
}

fn chain_of(
    cache: &PackageCache,
    pkg: &Arc<Package>,
    i: usize,
    chains: &mut Vec<Option<Option<String>>>,
    depth: usize,
) -> Option<String> {
    if let Some(known) = &chains[i] {
        return known.clone();
    }
    let e = &pkg.exports[i];
    let mut c = None;
    let cls = cache.try_resolve(pkg, e.class_index).ok().flatten();
    let cp = cls.map(|c| c.object_path(cache)).unwrap_or_default();
    let outer = usize::try_from(e.outer).ok().filter(|&o| o > 0 && o <= pkg.exports.len());
    if let Some(o) = outer
        && !LEFT_OUT.contains(&cp.as_str())
        && depth < pkg.exports.len()
        && let Some(up) = chain_of(cache, pkg, o - 1, chains, depth + 1)
    {
        c = Some(if up.is_empty() { e.object_name.clone() } else { format!("{up}.{}", e.object_name) });
    }
    chains[i] = Some(c.clone());
    c
}

/// An object path split at its inner part: `/Game/A/B.B:C.D` -> (`/Game/A/B.B`,
/// `C.D`).
pub fn split_inner(path: &str) -> (&str, Option<&str>) {
    match path.find(':') {
        Some(at) => (&path[..at], Some(&path[at + 1..])),
        None => (path, None),
    }
}

/// One export as the object kind gives it: a top-level export or one of its inner objects.
#[derive(Clone)]
pub struct ObjView<'a> {
    cache: &'a PackageCache,
    pkg: Arc<Package>,
    top: usize,
    index: usize,
    chain: String,
    obj: Arc<UObj>,
    path: String,
    class: String,
}

impl<'a> ObjView<'a> {
    /// Export `index` of `pkg`, `chain` its name chain below the top-level export `top` (empty for `top` itself).
    /// An error when its read reported one (the object kind's entry would not be written).
    // Deviation: the object kind fails a whole entry when any of its inner objects fails, or a struct holds two
    // members of one name; a view checks the one object's read only. No export a game kind reads fails either way in
    // this build (the object kind's two failures are an animation and a Niagara system).
    pub fn new(
        cache: &'a PackageCache,
        pkg: Arc<Package>,
        top: usize,
        index: usize,
        chain: String,
    ) -> Result<ObjView<'a>, KindError> {
        let obj = cache.object(&pkg, index);
        let errors: Vec<String> = obj
            .issues
            .iter()
            .filter(|e| !(e.kind == IssueKind::Native && obj.partial.is_some()))
            .map(|i| {
                let kind = match i.kind {
                    IssueKind::Read => "read",
                    IssueKind::Native => "native",
                    IssueKind::NotPorted => "not-ported",
                };
                format!("{kind}: {}", i.message)
            })
            .collect();
        if !errors.is_empty() {
            return Err(KindError::Failed(format!(
                "{} {}: {}",
                pkg.path,
                pkg.exports[index].object_name,
                errors.join("; ")
            )));
        }
        let path = Resolved::Export { pkg: pkg.clone(), index }.object_path(cache);
        let class = obj.cls.as_ref().map_or_else(|| "/Script/CoreUObject.Object".to_owned(), |c| c.object_path(cache));
        Ok(ObjView { cache, pkg, top, index, chain, obj, path, class })
    }

    /// Its own object path: `/Game/A/B.B`, `/Game/A/B.B:PersistentLevel.Floor`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Its class as an object path: `/Script/Engine.StaticMeshComponent`, `/Game/A/BP.BP_C`.
    pub fn class(&self) -> &str {
        &self.class
    }

    /// Its index in its package's export table (the AssetObject's `index`: the build's order of a level's
    /// objects).
    pub fn index(&self) -> usize {
        self.index
    }

    /// Its name chain below its top-level export; empty for a top-level export.
    pub fn chain(&self) -> &str {
        &self.chain
    }

    pub fn package(&self) -> &Arc<Package> {
        &self.pkg
    }

    /// The object as read.
    pub fn uobj(&self) -> &Arc<UObj> {
        &self.obj
    }

    pub fn cache(&self) -> &'a PackageCache {
        self.cache
    }

    /// The tagged properties it holds, by stored name (look a member up with [`field`], which takes the declared
    /// name the object kind's JSON writes).
    pub fn props(&self) -> &Props {
        &self.obj.props
    }

    /// The tagged property of declared name `name` (`properties.<name>`).
    pub fn prop(&self, name: &str) -> Option<&Value> {
        field(&self.obj.props, name)
    }

    /// Its archetype (the export entry's TemplateIndex) as an object path.
    pub fn template(&self) -> Result<Option<String>, KindError> {
        Ok(self
            .cache
            .try_resolve(&self.pkg, self.pkg.exports[self.index].template_index)?
            .map(|t| t.object_path(self.cache)))
    }

    /// A class or struct export's parent as an object path (`super`).
    pub fn super_path(&self) -> Result<Option<String>, KindError> {
        Ok(self
            .cache
            .try_resolve(&self.pkg, self.pkg.exports[self.index].super_index)?
            .map(|t| t.object_path(self.cache)))
    }

    /// The C# class name of the most basic class whose native serializer the reader lacks (`partial`).
    pub fn partial(&self) -> Option<&'static str> {
        self.obj.partial
    }

    /// What its class's native serializer read beyond the tagged properties (`native`), `None` when nothing: the
    /// natives' writers run on each ask. A binary section stays `Value::Bytes`.
    pub fn native(&self) -> Result<Option<Props>, KindError> {
        let mut native = Props::new();
        // most-derived writer first in the chain: each calls its base first, so run from the base up
        for c in self.obj.chain.iter().rev() {
            if let Some(json) = natives::native(c).and_then(|n| n.json) {
                json(&self.obj, &mut native);
            }
        }
        let mut extra = Props::new();
        for c in self.obj.chain.iter().rev() {
            if let Some(f) = natives::native(c).and_then(|n| n.extra) {
                f(&self.obj, &mut extra);
            }
        }
        for (k, v) in extra {
            if native.contains_key(&k) {
                return Err(KindError::Failed(format!("typed: {}: native {k} written twice", self.path)));
            }
            native.insert(k, v);
        }
        Ok((!native.is_empty()).then_some(native))
    }

    /// A class default object only: its properties over its Blueprint parents' class defaults (`defaults`).
    pub fn defaults(&self) -> Result<Option<Props>, KindError> {
        if self.obj.flags & RF_CLASS_DEFAULT_OBJECT == 0 {
            return Ok(None);
        }
        let Some(cls) = &self.obj.cls else { return Ok(None) };
        Ok(Some(class_defaults(self.cache, cls, &self.obj.props)?))
    }

    /// The objects inside it, in export order (`Object.values(o.inner)` sorted by `index`): for a top-level export
    /// the object kind's `inner`, for an inner object the ones below it.
    pub fn inner(&self) -> Result<Vec<ObjView<'a>>, KindError> {
        let below = if self.chain.is_empty() { String::new() } else { format!("{}.", self.chain) };
        inner_exports(self.cache, &self.pkg, self.top)
            .into_iter()
            .filter(|(_, c)| c.starts_with(&below))
            .map(|(i, c)| ObjView::new(self.cache, self.pkg.clone(), self.top, i, c))
            .collect()
    }

    /// The inner object of name chain `chain` below its top-level export (the chain as
    /// the entry keys it, else the first without regard to case).
    pub fn inner_named(&self, chain: &str) -> Result<Option<ObjView<'a>>, KindError> {
        let all = inner_exports(self.cache, &self.pkg, self.top);
        let low = chain.to_lowercase();
        let found =
            all.iter().find(|(_, c)| c == chain).or_else(|| all.iter().find(|(_, c)| c.to_lowercase() == low)).cloned();
        found.map(|(i, c)| ObjView::new(self.cache, self.pkg.clone(), self.top, i, c)).transpose()
    }
}

impl<'a> Cx<'a> {
    /// The object a path names, as the object kind holds it: a
    /// top-level export (`<package>.<export>`, or a bare package path for the export named as the package) or an
    /// inner object (`<package>.<export>:<chain>`).
    pub fn object(&self, path: &str) -> Result<ObjView<'a>, KindError> {
        let (outer, chain) = split_inner(path);
        let (pkg, top) = self.read(outer)?;
        let view = ObjView::new(self.cache, pkg, top, top, String::new())?;
        match chain {
            None => Ok(view),
            Some("") => Err(KindError::NotFound(format!("content: {path}: an empty inner name"))),
            Some(c) => view
                .inner_named(c)?
                .ok_or_else(|| KindError::NotFound(format!("content: {outer} holds no inner object {c}"))),
        }
    }

    /// Whether the object kind has the object (`scope.exists`): its package, its export, read without error.
    pub fn exists(&self, path: &str) -> bool {
        self.object(path).is_ok()
    }
}

/// The member of declared name `name` of a struct or of tagged properties (the key the object kind's JSON writes:
/// a UserDefinedStruct member's `_<n>_<GUID>` suffix dropped).
pub fn field<'v>(p: &'v Props, name: &str) -> Option<&'v Value> {
    p.iter().find(|(k, _)| member_name(k) == name).map(|(_, v)| v)
}

/// A value as a JSON number of the object kind (`typeof v === 'number'`): a float as the double the object kind read
/// (`f32_dotnet`), an integer as a double. A non-finite float is a string there: `None`.
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) if f.is_finite() => Some(f32_dotnet(*f)),
        Value::Double(d) if d.is_finite() => Some(*d),
        Value::Int(i) => Some(*i as f64),
        Value::UInt(u) => Some(*u as f64),
        _ => None,
    }
}

/// A value as a JSON string of the object kind (`typeof v === 'string'`): a name or string, a reference's object
/// path, a soft path (`Asset` or `Asset:Sub`), a non-finite float's name.
pub fn string(cache: &PackageCache, v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.clone()),
        Value::Ref(Some(r)) => Some(r.object_path(cache)),
        Value::SoftPath { asset, sub } if !(asset == "None" || asset.is_empty()) => {
            Some(if sub.is_empty() { asset.clone() } else { format!("{asset}:{sub}") })
        }
        Value::Float(f) if !f.is_finite() => Some(non_finite(f64::from(*f)).into()),
        Value::Double(d) if !d.is_finite() => Some(non_finite(*d).into()),
        _ => None,
    }
}

fn non_finite(d: f64) -> &'static str {
    if d.is_nan() {
        "NaN"
    } else if d > 0.0 {
        "Infinity"
    } else {
        "-Infinity"
    }
}

/// Whether the JSON value is an object (`isStruct`: not null, not an array): a struct, a text, a binary section's
/// `{ offset, length }`.
pub fn is_struct(v: &Value) -> bool {
    matches!(v, Value::Struct(_) | Value::Text(_) | Value::Bytes(_))
}

/// The JSON value's truth as the stage binds take it: an array or an object when not
/// empty, else JavaScript's (null, false, 0 and "" false; a non-finite float is a non-empty string).
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null | Value::Ref(None) => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::UInt(u) => *u != 0,
        Value::Float(f) => !f.is_finite() || *f != 0.0,
        Value::Double(d) => !d.is_finite() || *d != 0.0,
        Value::Str(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Map(m) => !m.is_empty(),
        Value::Struct(p) => !p.is_empty(),
        Value::Ref(Some(_)) | Value::Text(_) | Value::Bytes(_) => true,
        Value::SoftPath { asset, .. } => !(asset == "None" || asset.is_empty()),
    }
}

/// A reference's last name component, as the tables name classes and assets:
/// `/Game/A/B.B_C` -> `B_C`, `/Game/A/B.B_C:Comp` -> `Comp`, `/Script/Engine.StaticMesh` -> `StaticMesh`.
pub fn short_name(path: &str) -> &str {
    path.rsplit(['.', ':']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_values() {
        assert_eq!(short_name("/Game/A/B.B_C"), "B_C");
        assert_eq!(short_name("/Game/A/B.B_C:Comp"), "Comp");
        assert_eq!(short_name("/Engine/BasicShapes/Plane.Plane"), "Plane");
        assert_eq!(short_name("Plain"), "Plain");
        assert_eq!(split_inner("/Game/A/B.B:C.D"), ("/Game/A/B.B", Some("C.D")));
        assert_eq!(split_inner("/Game/A/B.B"), ("/Game/A/B.B", None));
        assert_eq!(num(&Value::Float(0.1)), Some(0.1));
        assert_eq!(num(&Value::Int(-3)), Some(-3.0));
        assert_eq!(num(&Value::Float(f32::NAN)), None);
        assert!(truthy(&Value::Float(f32::NAN)) && !truthy(&Value::Float(-0.0)));
        assert!(!truthy(&Value::Array(vec![])) && !truthy(&Value::Struct(Props::new())));
        assert!(!truthy(&Value::SoftPath { asset: "None".into(), sub: String::new() }));
        let p = Props::from([("Speed_2_0123456789ABCDEF0123456789ABCDEF".to_owned(), Value::Int(4))]);
        assert_eq!(field(&p, "Speed").and_then(num), Some(4.0));
    }
}
