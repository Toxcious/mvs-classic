//! What the menuscene bind's parts share: the build's objects as the object kind's
//! JSON gives them (a top-level object with every inner object below it, loaded once per entry), small helpers, and
//! the level and class lookups (a World's actors and components are its inner objects `PersistentLevel.<actor>.<comp>`,
//! a Blueprint class's templates and nodes the class export's inner objects).

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use mvs_material::menuscene::V3;
use mvs_math::js;

use crate::json::{Json, props_json, value_json};
use crate::kind::{Cx, KindError};
use crate::objview::{ObjView, split_inner};

/// An object's properties as the object kind's JSON holds them.
pub type Props = IndexMap<String, Json>;

/// A failure of the bind, its message prefixed with the kind.
pub fn fail(msg: impl std::fmt::Display) -> KindError {
    KindError::Failed(format!("menuscene: {msg}"))
}

/// One object as the object kind gives it: its path, class, export index, properties,
/// archetype and parent class, and for a top-level object every inner object by its name chain.
pub struct AObj<'a> {
    view: ObjView<'a>,
    pub props: Props,
    /// A top-level object's inner objects by chain, in export order (empty for an inner object).
    pub inner: IndexMap<String, Rc<AObj<'a>>>,
}

pub type Obj<'a> = Rc<AObj<'a>>;

impl<'a> AObj<'a> {
    fn of(view: ObjView<'a>) -> AObj<'a> {
        let props = props_json(view.cache(), view.props());
        AObj { view, props, inner: IndexMap::new() }
    }

    pub fn path(&self) -> &str {
        self.view.path()
    }

    pub fn class(&self) -> &str {
        self.view.class()
    }

    /// The export index (the build's order of a package's objects).
    pub fn index(&self) -> usize {
        self.view.index()
    }

    /// Its archetype's object path (`template`).
    pub fn template(&self) -> Result<Option<String>, KindError> {
        self.view.template()
    }

    /// A class's parent class (`super`).
    pub fn super_path(&self) -> Result<Option<String>, KindError> {
        self.view.super_path()
    }

    /// A member of what its class's native serializer read (`native.<name>`), as the object kind's JSON.
    pub fn native(&self, name: &str) -> Result<Option<Json>, KindError> {
        Ok(self.view.native()?.and_then(|n| n.get(name).map(|v| value_json(self.view.cache(), v))))
    }

    /// A property (`properties.<name>`).
    pub fn get(&self, name: &str) -> Option<&Json> {
        self.props.get(name)
    }
}

/// The objects one entry's bind loaded: each top-level object read once.
pub struct Scope<'c, 'a> {
    pub cx: &'c Cx<'a>,
    objects: RefCell<IndexMap<String, Obj<'a>>>,
}

impl<'c, 'a> Scope<'c, 'a> {
    pub fn new(cx: &'c Cx<'a>) -> Scope<'c, 'a> {
        Scope { cx, objects: RefCell::new(IndexMap::new()) }
    }

    /// `scope.load('object', path)`: a top-level object with its inner objects, or an inner object (`<obj>:<chain>`:
    /// the chain as keyed, else the first without regard to case).
    pub fn load(&self, path: &str) -> Result<Obj<'a>, KindError> {
        let (outer, chain) = split_inner(path);
        let top = self.top(outer)?;
        match chain {
            None => Ok(top),
            Some(c) => {
                let low = c.to_lowercase();
                top.inner
                    .get(c)
                    .or_else(|| top.inner.iter().find(|(k, _)| k.to_lowercase() == low).map(|(_, v)| v))
                    .cloned()
                    .ok_or_else(|| KindError::NotFound(format!("content: {outer} holds no inner object {c}")))
            }
        }
    }

    fn top(&self, path: &str) -> Result<Obj<'a>, KindError> {
        if let Some(o) = self.objects.borrow().get(path) {
            return Ok(o.clone());
        }
        let view = self.cx.object(path)?;
        let mut o = AObj::of(view.clone());
        for v in view.inner()? {
            let chain = v.chain().to_owned();
            o.inner.entry(chain).or_insert_with(|| Rc::new(AObj::of(v)));
        }
        let o = Rc::new(o);
        self.objects.borrow_mut().insert(path.to_owned(), o.clone());
        Ok(o)
    }

    /// `scope.exists`: the object kind has the object.
    pub fn exists(&self, path: &str) -> bool {
        self.objects.borrow().contains_key(path) || self.cx.exists(path)
    }
}

// ---------------------------------------------------------------- values

/// A JSON number, else None (`undefined`, or another type).
pub fn num(v: Option<&Json>) -> Option<f64> {
    v.and_then(Json::as_f64)
}

/// `v ?? d` of a number member.
pub fn num_or(v: Option<&Json>, d: f64) -> f64 {
    num(v).unwrap_or(d)
}

/// A string member.
pub fn str_of(v: Option<&Json>) -> Option<&str> {
    v.and_then(Json::as_str)
}

/// JavaScript's truth of a member (`!!v`; undefined false).
pub fn truthy(v: Option<&Json>) -> bool {
    match v {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Json::Str(s)) => !s.is_empty(),
        Some(Json::Arr(_) | Json::Obj(_)) => true,
    }
}

/// A member that is exactly `false` (`v === false`).
pub fn is_false(v: Option<&Json>) -> bool {
    v == Some(&Json::Bool(false))
}

/// An array member's elements (`?? []`).
pub fn arr(v: Option<&Json>) -> &[Json] {
    v.and_then(Json::as_array).unwrap_or(&[])
}

/// An object member (`?? {}`).
pub fn obj_of(v: Option<&Json>) -> Option<&Props> {
    v.and_then(Json::as_object)
}

/// A vector's three members (`d` for one the export leaves out; none: an error).
pub fn vec(v: Option<&Json>, d: Option<V3>) -> Result<V3, KindError> {
    match v {
        None | Some(Json::Null) => d.ok_or_else(|| fail("a vector the export leaves out")),
        Some(x) => Ok(["X", "Y", "Z"].map(|k| num_or(x.get(k), 0.0))),
    }
}

/// A rotator's Pitch, Yaw, Roll.
pub fn rot(r: Option<&Json>) -> V3 {
    match r {
        None | Some(Json::Null) => [0.0; 3],
        Some(x) => ["Pitch", "Yaw", "Roll"].map(|k| num_or(x.get(k), 0.0)),
    }
}

/// Rounded to 1e-6 (Math.round).
pub fn r6(v: f64) -> f64 {
    js::round(v * 1e6) / 1e6
}

/// [`vec`] rounded by [`r6`]; `d` as given for one the export leaves out.
pub fn vec6(v: Option<&Json>, d: Option<V3>) -> Result<V3, KindError> {
    match v {
        None | Some(Json::Null) => d.ok_or_else(|| fail("a vector the export leaves out")),
        Some(_) => Ok(vec(v, None)?.map(r6)),
    }
}

pub fn rot6(r: Option<&Json>) -> V3 {
    rot(r).map(r6)
}

/// The member at a chain of keys (`v.a.b.c`), None where one is missing.
pub fn dig<'v>(v: Option<&'v Json>, keys: &[&str]) -> Option<&'v Json> {
    keys.iter().try_fold(v?, |x, k| x.get(k))
}

/// The enum value's tail after its last `::`.
pub fn tail(v: &str) -> &str {
    v.rfind("::").map_or(v, |i| &v[i + 2..])
}

// ---------------------------------------------------------------- paths

/// An object path's package (`/Game/A/B.B:C` -> `/Game/A/B`).
pub fn pkg_of(path: &str) -> &str {
    let slash = path.rfind('/').unwrap_or(0);
    path[slash..].find('.').map_or(path, |d| &path[..slash + d])
}

/// An object path's last name: `/Game/A/B.B` -> `B`, `/Game/A/B.B_C` -> `B_C`, `/Game/A/B.B:X.Y` -> `Y`.
pub fn short_of(path: &str) -> &str {
    let slash = path.rfind('/').unwrap_or(0);
    if let Some(c) = path[slash..].find(':') {
        let chain = &path[slash + c + 1..];
        return &chain[chain.rfind('.').map_or(0, |i| i + 1)..];
    }
    &path[path.rfind('.').map_or(0, |i| i + 1)..]
}

/// A package's asset name.
pub fn asset_name(path: &str) -> &str {
    let p = pkg_of(path);
    &p[p.rfind('/').map_or(0, |i| i + 1)..]
}

/// A package's asset object (`/Game/A/B` -> `/Game/A/B.B`).
pub fn asset_of(pkg: &str) -> String {
    format!("{}.{}", pkg_of(pkg), asset_name(pkg))
}

/// A class's short name (`/Script/Engine.SceneComponent` -> `SceneComponent`).
pub fn class_of<'o>(o: &'o AObj<'_>) -> &'o str {
    let c = o.class();
    &c[c.rfind('.').map_or(0, |i| i + 1)..]
}

/// The name chain below its export of an inner object's path (`/Game/A/B.B:X.Y` -> `X.Y`).
pub fn chain_of(path: &str) -> &str {
    let slash = path.rfind('/').unwrap_or(0);
    path[slash..].find(':').map_or(path, |c| &path[slash + c + 1..])
}

/// A class path's package below Content: `/Game/A/B.B_C` -> `A/B`.
pub fn content_pkg(path: &str) -> String {
    let p = pkg_of(path);
    if let Some(r) = p.strip_prefix("/Game/") {
        r.to_owned()
    } else if let Some(r) = p.strip_prefix("/Engine/") {
        format!("Engine/Content/{r}")
    } else {
        p.to_owned()
    }
}

/// A class's default object (`<pkg>.Default__<Name>_C`).
pub fn cdo_of(cls: &str) -> String {
    format!("{}.Default__{}", pkg_of(cls), short_of(cls))
}

/// A class name without `_C`.
pub fn class_name(cls: &str) -> String {
    let s = short_of(cls);
    s.strip_suffix("_C").unwrap_or(s).to_owned()
}

// ---------------------------------------------------------------- objects

/// An instance's tagged properties over its archetype's (structs merge member by member at any depth; arrays and
/// scalars replace).
pub fn over_props(a: Option<&Props>, b: Option<&Props>) -> Props {
    let mut out = a.cloned().unwrap_or_default();
    for (k, v) in b.into_iter().flatten() {
        let merged = match (v, out.get(k)) {
            (Json::Obj(vb), Some(Json::Obj(va))) => Json::Obj(over_props(Some(va), Some(vb))),
            _ => v.clone(),
        };
        out.insert(k.clone(), merged);
    }
    out
}

/// An inner object of `o` by its path (it must be one of `o`'s).
pub fn inner_at<'a>(o: &AObj<'a>, path: &str, what: &str) -> Result<Obj<'a>, KindError> {
    if !path.starts_with(&format!("{}:", o.path())) {
        return Err(fail(format!("{what}: {path} is not inside {}", o.path())));
    }
    o.inner.get(chain_of(path)).cloned().ok_or_else(|| fail(format!("{what}: no {path}")))
}

/// The inner objects of `o` in the package's export order.
pub fn inner_list<'a>(o: &AObj<'a>) -> Vec<Obj<'a>> {
    let mut v: Vec<Obj<'a>> = o.inner.values().cloned().collect();
    v.sort_by_key(|x| x.index());
    v
}

/// A Blueprint class's template by its inner name.
pub fn class_inner<'a>(cls: &AObj<'a>, name: &str) -> Result<Obj<'a>, KindError> {
    cls.inner.get(name).cloned().ok_or_else(|| fail(format!("{}: no {name}", cls.path())))
}

/// The chain's parts below `PersistentLevel.`: `Some([actor])` or `Some([actor, comp])`.
fn level_parts(chain: &str) -> Option<Vec<&str>> {
    chain.strip_prefix("PersistentLevel.").map(|r| r.split('.').collect())
}

/// A World's level: its actors and their direct components.
pub struct Level<'a> {
    pub name: String,
    pub world: Obj<'a>,
}

impl<'a> Level<'a> {
    pub fn load(scope: &Scope<'_, 'a>, world: &str) -> Result<Level<'a>, KindError> {
        let world = scope.load(world)?;
        Ok(Level { name: asset_name(world.path()).to_owned(), world })
    }

    /// An actor (`PersistentLevel.<actor>`).
    pub fn actor(&self, name: &str) -> Option<Obj<'a>> {
        self.world.inner.get(&format!("PersistentLevel.{name}")).cloned()
    }

    /// The actors of a class (its short name), in export order.
    pub fn actors(&self, cls: &str) -> Vec<Obj<'a>> {
        inner_list(&self.world)
            .into_iter()
            .filter(|o| level_parts(chain_of(o.path())).is_some_and(|p| p.len() == 1) && class_of(o) == cls)
            .collect()
    }

    /// An actor's component by name.
    pub fn comp(&self, actor: &str, name: &str) -> Option<Obj<'a>> {
        self.world.inner.get(&format!("PersistentLevel.{actor}.{name}")).cloned()
    }

    /// An actor's direct components of a class (short name), in export order.
    pub fn comps(&self, actor: &str, cls: &str) -> Vec<Obj<'a>> {
        let pre = format!("PersistentLevel.{actor}.");
        inner_list(&self.world)
            .into_iter()
            .filter(|o| {
                let c = chain_of(o.path());
                c.strip_prefix(&pre).is_some_and(|r| !r.contains('.')) && class_of(o) == cls
            })
            .collect()
    }

    /// The one direct component of a class an actor has (the first): its properties.
    pub fn comp_of(&self, actor: &str, cls: &str) -> Result<Props, KindError> {
        self.comps(actor, cls)
            .first()
            .map(|c| c.props.clone())
            .ok_or_else(|| fail(format!("{}: {actor} has no {cls}", self.name)))
    }

    /// Every direct component of an actor of a class (short name) across the level, in export order.
    pub fn all_comps(&self, cls: &str) -> Vec<Obj<'a>> {
        inner_list(&self.world)
            .into_iter()
            .filter(|o| level_parts(chain_of(o.path())).is_some_and(|p| p.len() == 2) && class_of(o) == cls)
            .collect()
    }

    /// The actor a component's object path names (`PersistentLevel.<actor>.<comp>`).
    pub fn owner_of(r: &str) -> String {
        let p: Vec<&str> = chain_of(r).split('.').collect();
        p.get(p.len().wrapping_sub(2)).copied().unwrap_or_default().to_owned()
    }

    /// The component's actor (the level's inner key's second name).
    pub fn actor_of(o: &AObj<'_>) -> String {
        chain_of(o.path()).split('.').nth(1).unwrap_or_default().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(pkg_of("/Game/A/B.B:C"), "/Game/A/B");
        assert_eq!(short_of("/Game/A/B.B_C"), "B_C");
        assert_eq!(short_of("/Game/A/B.B:X.Y"), "Y");
        assert_eq!(asset_of("/Game/A/B"), "/Game/A/B.B");
        assert_eq!(chain_of("/Game/A/B.B:X.Y"), "X.Y");
        assert_eq!(content_pkg("/Game/A/B.B_C"), "A/B");
        assert_eq!(content_pkg("/Engine/A/B.B"), "Engine/Content/A/B");
        assert_eq!(cdo_of("/Game/A/BP.BP_C"), "/Game/A/BP.Default__BP_C");
        assert_eq!(Level::owner_of("/Game/M.M:PersistentLevel.Act.Root"), "Act");
        assert_eq!(r6(0.12345649), 0.123456);
    }

    #[test]
    fn merge() {
        let a = Json::parse(br#"{"A":{"X":1,"Y":2},"B":[1]}"#).unwrap();
        let b = Json::parse(br#"{"A":{"Y":3},"B":[2],"C":true}"#).unwrap();
        let m = over_props(a.as_object(), b.as_object());
        assert_eq!(Json::Obj(m), Json::parse(br#"{"A":{"X":1,"Y":3},"B":[2],"C":true}"#).unwrap());
    }
}
