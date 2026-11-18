//! The widget part of the `ui` kind's bind: a widget Blueprint class as the HUD runtime reads it (`hud/slate`
//! ClassData: its tree, its class defaults, its WidgetAnimations), read from the build's objects in the form the
//! runtime reads ([`UiShape`]).
//!
//! Objects are held as the object kind's JSON (`Json`, `props_json`), as an earlier bind read them: a widget's values
//! are reshaped from that JSON, not from the typed values.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;
use mvs_uasset::typed::member_name;

use crate::json::{Json, props_json, value_json};
use crate::kind::{Cx, KindError};
use crate::objview::{ObjView, short_name};

type Map = IndexMap<String, Json>;

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(why))
}

/// Properties left out of a widget's values (the tree's links and editor state).
const SKIP: [&str; 7] = ["Slot", "Slots", "Content", "Parent", "bIsVariable", "Navigation", "UberGraphFrame"];
const TEXT_KEYS: [&str; 4] = ["source", "namespace", "key", "table"];
const WIDGET_CLASS: &str = "/Script/UMG.WidgetBlueprintGeneratedClass";

// JS semantics of the values a bind tests

/// JS truthiness of a JSON value (an array and an object are truthy even when empty).
pub fn truthy(v: Option<&Json>) -> bool {
    match v {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Json::Str(s)) => !s.is_empty(),
        Some(Json::Arr(_) | Json::Obj(_)) => true,
    }
}

/// `Number(v)` for what a widget's values hold.
fn js_number(v: &Json) -> f64 {
    match v {
        Json::Num(n) => *n,
        Json::Null | Json::Bool(false) => 0.0,
        Json::Bool(true) => 1.0,
        Json::Str(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse().unwrap_or(f64::NAN) }
        }
        Json::Arr(_) | Json::Obj(_) => f64::NAN,
    }
}

/// A member of an object value (`undefined` for a member it lacks or a value that is no object).
fn get<'j>(v: &'j Json, k: &str) -> Option<&'j Json> {
    v.get(k)
}

/// A string member, or `None`.
fn gets<'j>(v: &'j Json, k: &str) -> Option<&'j str> {
    get(v, k).and_then(Json::as_str)
}

/// A member of a map of properties.
fn mget<'j>(m: &'j Map, k: &str) -> Option<&'j Json> {
    m.get(k)
}

fn is_obj(v: &Json) -> bool {
    matches!(v, Json::Obj(_))
}

/// An object's values in the order `Object.values` gives (integer-like keys first, ascending, the rest as set).
fn values_in_order(m: &Map) -> Vec<&Json> {
    let index = |k: &str| {
        (!k.is_empty() && (k.len() == 1 || !k.starts_with('0')) && k.bytes().all(|c| c.is_ascii_digit()))
            .then(|| k.parse::<u32>().ok().filter(|&n| n != u32::MAX))
            .flatten()
    };
    let mut ordered: Vec<(&String, &Json)> = m.iter().collect();
    if ordered.iter().any(|(k, _)| index(k).is_some()) {
        ordered.sort_by_key(|(k, _)| index(k).map_or((1, 0), |n| (0, n)));
    }
    ordered.into_iter().map(|(_, v)| v).collect()
}

/// `/^\/(?:Game|Engine|Script)\/[^.\s]*\.[^/\s]+$/`: a reference as the object kind writes one.
pub fn is_object_path(s: &str) -> bool {
    let Some(rest) = ["/Game/", "/Engine/", "/Script/"].iter().find_map(|p| s.strip_prefix(p)) else {
        return false;
    };
    let space = |c: char| c.is_whitespace() || c == '\u{feff}';
    let Some(dot) = rest.find('.') else { return false };
    let (head, tail) = (&rest[..dot], &rest[dot + 1..]);
    !head.chars().any(space) && !tail.is_empty() && !tail.chars().any(|c| space(c) || c == '/')
}

/// The name a path's `memberName` gives (`^(.+)_\d+_[0-9A-F]{32}$` dropped).
fn member(stored: &str) -> &str {
    member_name(stored)
}

/// A text value as the object kind has it: a Base history `{ Namespace, Key, SourceString, LocalizedString }` (the
/// English string is the source string in this build), a culture-invariant `{ CultureInvariantString }` (`textOf`).
fn text_of(t: &Json) -> Json {
    let mut out = Map::new();
    let mut put = |k: &str, v: Option<&Json>| {
        if let Some(v) = v {
            out.insert(k.to_owned(), v.clone());
        }
    };
    let null = Json::Null;
    if get(t, "table").is_some() {
        put("TableId", get(t, "table"));
        put("Key", get(t, "key"));
        put("SourceString", Some(get(t, "source").unwrap_or(&null)));
    } else if get(t, "namespace").is_some() {
        put("Namespace", get(t, "namespace"));
        put("Key", get(t, "key"));
        put("SourceString", get(t, "source"));
        put("LocalizedString", get(t, "source"));
    } else {
        put("CultureInvariantString", Some(get(t, "source").unwrap_or(&null)));
    }
    Json::Obj(out)
}

fn is_text(v: &Json) -> bool {
    match v {
        Json::Obj(m) => m.contains_key("source") && m.keys().all(|k| TEXT_KEYS.contains(&k.as_str())),
        _ => false,
    }
}

/// One object as the object kind gives it: its class, path, index and properties as JSON.
#[derive(Clone)]
pub struct Obj<'a> {
    pub view: ObjView<'a>,
    pub props: Map,
}

impl<'a> Obj<'a> {
    fn new(view: ObjView<'a>) -> Obj<'a> {
        let props = props_json(view.cache(), view.props());
        Obj { view, props }
    }

    /// The object a path names.
    pub fn load(cx: &Cx<'a>, path: &str) -> Result<Obj<'a>, KindError> {
        Ok(Obj::new(cx.object(path)?))
    }

    pub fn path(&self) -> &str {
        self.view.path()
    }

    pub fn class(&self) -> &str {
        self.view.class()
    }

    /// The `ChildProperties` its native reader gives (a class's or a struct's property definitions), as JSON.
    fn child_properties(&self) -> Result<Vec<Json>, KindError> {
        let native = self.view.native()?;
        Ok(match native.as_ref().and_then(|n| n.get("ChildProperties")) {
            Some(v) => match value_json(self.view.cache(), v) {
                Json::Arr(a) => a,
                _ => Vec::new(),
            },
            None => Vec::new(),
        })
    }
}

/// The object kind's values in the form the runtime reads: references as their short names, soft references as
/// `{ AssetPathName, SubPathString }`, texts as above, maps as `{ Key, Value }` entries, colours without their Hex, and
/// a UserDefinedStruct's members under their stored names (`Label_2_<GUID>`: the object kind drops the suffix, the
/// struct's ChildProperties give it back) (`UiShape`).
pub struct UiShape<'c, 'a> {
    cx: &'c Cx<'a>,
    structs: RefCell<HashMap<String, Option<Rc<Vec<Json>>>>>,
    classes: RefCell<HashMap<String, Rc<Vec<Json>>>>,
}

impl<'c, 'a> UiShape<'c, 'a> {
    pub fn new(cx: &'c Cx<'a>) -> UiShape<'c, 'a> {
        UiShape { cx, structs: RefCell::default(), classes: RefCell::default() }
    }

    /// A UserDefinedStruct's members (`None` for a native struct).
    fn members(&self, strukt: &str) -> Result<Option<Rc<Vec<Json>>>, KindError> {
        if let Some(m) = self.structs.borrow().get(strukt) {
            return Ok(m.clone());
        }
        let m = if strukt.starts_with("/Script/") {
            None
        } else {
            Some(Rc::new(Obj::load(self.cx, strukt)?.child_properties()?))
        };
        self.structs.borrow_mut().insert(strukt.to_owned(), m.clone());
        Ok(m)
    }

    /// A Blueprint class's properties, its parents' after its own (a native class: none).
    fn class_props(&self, cls: &str) -> Result<Rc<Vec<Json>>, KindError> {
        if let Some(p) = self.classes.borrow().get(cls) {
            return Ok(p.clone());
        }
        let p = if cls.starts_with("/Script/") {
            Vec::new()
        } else {
            let o = Obj::load(self.cx, cls)?;
            let mut all = o.child_properties()?;
            if let Some(sup) = o.view.super_path()? {
                all.extend(self.class_props(&sup)?.iter().cloned());
            }
            all
        };
        let p = Rc::new(p);
        self.classes.borrow_mut().insert(cls.to_owned(), p.clone());
        Ok(p)
    }

    /// Properties of an object of class `cls`, without [`SKIP`]'s.
    pub fn props(&self, p: &Map, cls: &str) -> Result<Map, KindError> {
        let defs = self.class_props(cls)?;
        let mut out = Map::new();
        for (k, v) in p {
            if SKIP.contains(&k.as_str()) {
                continue;
            }
            let def = defs.iter().find(|d| gets(d, "Name") == Some(k));
            out.insert(k.clone(), self.value(v, def)?);
        }
        Ok(out)
    }

    fn value(&self, v: &Json, def: Option<&Json>) -> Result<Json, KindError> {
        let ty = def.and_then(|d| gets(d, "Type"));
        let d = def.unwrap_or(&Json::Null);
        match ty {
            Some("TextProperty") => return Ok(if is_obj(v) { text_of(v) } else { v.clone() }),
            Some("SoftObjectProperty" | "SoftClassProperty") => {
                return Ok(Json::object([
                    ("AssetPathName", Json::from(v.as_str().unwrap_or("None"))),
                    ("SubPathString", Json::from("")),
                ]));
            }
            Some("ArrayProperty") => {
                if let Json::Arr(a) = v {
                    return Ok(Json::Arr(a.iter().map(|x| self.value(x, get(d, "Inner"))).collect::<Result<_, _>>()?));
                }
                return Ok(v.clone());
            }
            Some("SetProperty") => {
                if let Json::Arr(a) = v {
                    return Ok(Json::Arr(
                        a.iter().map(|x| self.value(x, get(d, "ElementProp"))).collect::<Result<_, _>>()?,
                    ));
                }
                return Ok(v.clone());
            }
            Some("MapProperty") => {
                if let Json::Arr(a) = v {
                    let entry = |e: &Json| -> Result<Json, KindError> {
                        let pair = e.as_array().unwrap_or(&[]);
                        let at = |i: usize| pair.get(i).unwrap_or(&Json::Null);
                        Ok(Json::object([
                            ("Key", self.value(at(0), get(d, "KeyProp"))?),
                            ("Value", self.value(at(1), get(d, "ValueProp"))?),
                        ]))
                    };
                    return Ok(Json::Arr(a.iter().map(entry).collect::<Result<_, _>>()?));
                }
                return Ok(v.clone());
            }
            Some("StructProperty") => {
                let members = match gets(d, "Struct").filter(|s| !s.is_empty()) {
                    Some(s) => self.members(s)?,
                    None => None,
                };
                if let (Some(members), Json::Obj(fields)) = (members, v) {
                    let mut out = Map::new();
                    for (k, x) in fields {
                        let m = members.iter().find(|m| gets(m, "Name").is_some_and(|n| member(n) == k));
                        let name = m.and_then(|m| gets(m, "Name")).unwrap_or(k);
                        out.insert(name.to_owned(), self.value(x, m)?);
                    }
                    return Ok(Json::Obj(out));
                }
            }
            _ => {}
        }
        Ok(self.clean(v))
    }

    /// A value with no definition to go by (a native class's or native struct's).
    pub fn clean(&self, v: &Json) -> Json {
        match v {
            Json::Arr(a) => Json::Arr(a.iter().map(|x| self.clean(x)).collect()),
            Json::Str(s) if is_object_path(s) => Json::Str(short_name(s).to_owned()),
            Json::Obj(_) if is_text(v) => text_of(v),
            Json::Obj(m) => {
                Json::Obj(m.iter().filter(|(k, _)| *k != "Hex").map(|(k, x)| (k.clone(), self.clean(x))).collect())
            }
            _ => v.clone(),
        }
    }
}

/// A widget package's objects by their object path: its class and class default object and what is inside them.
pub struct WidgetPackage<'a> {
    pub cls: Obj<'a>,
    pub cdo: Obj<'a>,
    /// Every object of the package, in its export order (the dump's order).
    pub objects: Vec<Obj<'a>>,
    by_path: HashMap<String, usize>,
}

impl<'a> WidgetPackage<'a> {
    /// The object a reference value names.
    fn at(&self, r: Option<&Json>) -> Option<&Obj<'a>> {
        let path = r.and_then(Json::as_str)?;
        self.by_path.get(path).map(|&i| &self.objects[i])
    }

    /// The tree's WidgetTree object, when the class has a tree of its own.
    pub fn tree(&self) -> Option<&Obj<'a>> {
        self.objects.iter().find(|o| {
            o.class() == "/Script/UMG.WidgetTree" && matches!(mget(&o.props, "RootWidget"), Some(Json::Str(_)))
        })
    }
}

pub struct WidgetReader<'c, 'a> {
    cx: &'c Cx<'a>,
    pub shape: UiShape<'c, 'a>,
    pkgs: RefCell<HashMap<String, Rc<WidgetPackage<'a>>>>,
}

impl<'c, 'a> WidgetReader<'c, 'a> {
    pub fn new(cx: &'c Cx<'a>) -> WidgetReader<'c, 'a> {
        WidgetReader { cx, shape: UiShape::new(cx), pkgs: RefCell::default() }
    }

    /// A widget Blueprint by its package path (`/Game/Panda_Main/UI/X`).
    pub fn load(&self, pkg: &str) -> Result<Rc<WidgetPackage<'a>>, KindError> {
        if let Some(p) = self.pkgs.borrow().get(pkg) {
            return Ok(p.clone());
        }
        let name = &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..];
        let cls_path = format!("{pkg}.{name}_C");
        let dot = cls_path.rfind('.').unwrap_or(0);
        let cdo_path = format!("{}.Default__{}", &cls_path[..dot], &cls_path[dot + 1..]);
        let (cls, cdo) = (self.cx.object(&cls_path)?, self.cx.object(&cdo_path)?);
        let mut objects = Vec::new();
        let mut tops = Vec::new();
        for top in [cls, cdo] {
            let inner = top.inner()?;
            let o = Obj::new(top);
            tops.push(o.clone());
            objects.push(o);
            objects.extend(inner.into_iter().map(Obj::new));
        }
        objects.sort_by_key(|o| o.view.index());
        let by_path = objects.iter().enumerate().map(|(i, o)| (o.path().to_owned(), i)).collect();
        let cdo = tops.pop().unwrap_or_else(|| unreachable!("two tops"));
        let cls = tops.pop().unwrap_or_else(|| unreachable!("two tops"));
        let p = Rc::new(WidgetPackage { cls, cdo, objects, by_path });
        self.pkgs.borrow_mut().insert(pkg.to_owned(), p.clone());
        Ok(p)
    }

    /// A child Blueprint without a tree of its own: its parent widget Blueprint's package (`None`: one with a tree, or a
    /// native parent).
    pub fn parent(&self, w: &WidgetPackage<'a>) -> Result<Option<String>, KindError> {
        if w.tree().is_some() {
            return Ok(None);
        }
        let Some(sup) = w.cls.view.super_path()? else { return Ok(None) };
        if sup.starts_with("/Script/") {
            return Ok(None);
        }
        if self.cx.object(&sup)?.class() != WIDGET_CLASS {
            return Ok(None);
        }
        Ok(Some(sup[..sup.rfind('.').unwrap_or(sup.len())].to_owned()))
    }

    /// The roots and every user widget in their trees (a child Blueprint's parent's tree), by package path, skipping the
    /// classes named in `skip`.
    pub fn closure(&self, roots: &[String], skip: &[&str]) -> Result<Vec<String>, KindError> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for r in roots {
            self.visit(r, skip, &mut out, &mut seen)?;
        }
        Ok(out)
    }

    fn visit(
        &self,
        pkg: &str,
        skip: &[&str],
        out: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) -> Result<(), KindError> {
        let name = &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..];
        if !seen.insert(name.to_owned()) {
            return Ok(());
        }
        let mut w = self.load(pkg)?;
        if w.tree().is_none() {
            let Some(parent) = self.parent(&w)? else { return Ok(()) };
            w = self.load(&parent)?;
        }
        out.push(pkg.to_owned());
        let Some(tree) = w.tree() else { return Ok(()) };
        let tree_path = tree.path().to_owned();
        for o in &w.objects {
            let own_tree = o.path().rfind('.').is_some_and(|d| o.path()[..d] == tree_path);
            if !o.class().ends_with("_C") || !own_tree {
                continue;
            }
            let n = short_name(o.class());
            let n = n.strip_suffix("_C").unwrap_or(n);
            if skip.contains(&n) {
                continue;
            }
            let cls = o.class();
            self.visit(&cls[..cls.rfind('.').unwrap_or(cls.len())], skip, out, seen)?;
        }
        Ok(())
    }

    /// The references a widget package's objects hold, in the package's order.
    pub fn refs(&self, w: &WidgetPackage<'a>) -> Vec<String> {
        fn walk(v: &Json, out: &mut Vec<String>) {
            match v {
                Json::Arr(a) => a.iter().for_each(|x| walk(x, out)),
                Json::Str(s) => {
                    if is_object_path(s) && !s.starts_with("/Script/") {
                        out.push(s.clone());
                    }
                }
                Json::Obj(m) => values_in_order(m).into_iter().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for o in &w.objects {
            for v in values_in_order(&o.props) {
                walk(v, &mut out);
            }
        }
        out
    }

    /// A widget package's class data (`ClassData`: `{ root, defaults, anims }`).
    pub fn build(&self, pkg: &str) -> Result<Json, KindError> {
        let w = self.load(pkg)?;
        let defaults_of = |cdo: &Obj<'a>| self.shape.props(&cdo.props, w.cls.path());
        if let Some(parent) = self.parent(&w)? {
            // a child Blueprint: its parent's tree and animations, the parent's class defaults under its own
            let p = self.build(&parent)?;
            let mut defaults = match p.get("defaults") {
                Some(Json::Obj(d)) => d.clone(),
                _ => Map::new(),
            };
            defaults.extend(defaults_of(&w.cdo)?);
            return Ok(Json::object([
                ("root", p.get("root").cloned().unwrap_or(Json::Null)),
                ("defaults", Json::Obj(defaults)),
                ("anims", p.get("anims").cloned().unwrap_or(Json::Null)),
            ]));
        }
        let Some(tree) = w.tree() else { return failed(format!("ui: {pkg}: no widget tree")) };
        let root_ref = mget(&tree.props, "RootWidget");
        let Some(root) = w.at(root_ref) else {
            return failed(format!(
                "ui: {pkg}: the root widget {} is not in the package",
                root_ref.map_or_else(|| "undefined".to_owned(), |r| r.as_str().unwrap_or("[object]").to_owned())
            ));
        };
        let mut anims = Map::new();
        for a in w.objects.iter().filter(|o| o.class() == "/Script/UMG.WidgetAnimation") {
            let key = match mget(&a.props, "DisplayLabel").and_then(Json::as_str) {
                Some(l) => l.to_owned(),
                None => {
                    let n = short_name(a.path());
                    n.strip_suffix("_INST").unwrap_or(n).to_owned()
                }
            };
            anims.insert(key, animation(a, &w)?);
        }
        Ok(Json::object([
            ("root", self.node(&w, root)?),
            ("defaults", Json::Obj(defaults_of(&w.cdo)?)),
            ("anims", Json::Obj(anims)),
        ]))
    }

    /// One widget of a tree (`WData`: `{ t, n, p, st?, s?, c?, ns? }`).
    fn node(&self, w: &WidgetPackage<'a>, e: &Obj<'a>) -> Result<Json, KindError> {
        let p = &e.props;
        let mut props = self.shape.props(p, e.class())?;
        props.shift_remove("NamedSlotBindings");
        let t = short_name(e.class());
        let mut n = Map::new();
        n.insert("t".into(), Json::from(t.strip_suffix("_C").unwrap_or(t)));
        n.insert("n".into(), Json::from(short_name(e.path())));
        n.insert("p".into(), Json::Obj(props));
        if let Some(slot) = w.at(mget(p, "Slot")) {
            n.insert("st".into(), Json::from(short_name(slot.class())));
            n.insert("s".into(), Json::Obj(self.shape.props(&slot.props, slot.class())?));
        }
        // the content a slot holds that the tree has
        let held = |slot: &Obj<'a>| -> Option<&Obj<'a>> {
            match mget(&slot.props, "Content") {
                Some(Json::Str(_)) => w.at(mget(&slot.props, "Content")),
                _ => None,
            }
        };
        let mut kids = Vec::new();
        if let Some(Json::Arr(slots)) = mget(p, "Slots") {
            for s in slots {
                if let Some(c) = w.at(Some(s)).and_then(held) {
                    kids.push(self.node(w, c)?);
                }
            }
        }
        let boxy = {
            let c = short_name(e.class());
            c.ends_with("Box") || c.ends_with("Border")
        };
        if matches!(mget(p, "Content"), Some(Json::Str(_)))
            && boxy
            && let Some(c) = w.at(mget(p, "Content")).and_then(held)
        {
            kids.push(self.node(w, c)?);
        }
        if !kids.is_empty() {
            n.insert("c".into(), Json::Arr(kids));
        }
        // a user widget instance's named slot contents (widgets of this tree placed in the instance's NamedSlots)
        if let Some(Json::Arr(bindings)) = mget(p, "NamedSlotBindings") {
            let mut ns = Map::new();
            for b in bindings {
                if let Some(c) = w.at(get(b, "Content")) {
                    let name = gets(b, "Name").unwrap_or("undefined").to_owned();
                    ns.insert(name, self.node(w, c)?);
                }
            }
            if !ns.is_empty() {
                n.insert("ns".into(), Json::Obj(ns));
            }
        }
        Ok(Json::Obj(n))
    }
}

// ERangeBoundTypes: Exclusive 0, Inclusive 1, Open 2
fn bound_type(b: &Json) -> Result<f64, KindError> {
    match get(b, "Type") {
        Some(Json::Num(n)) => Ok(*n),
        t => match t.and_then(Json::as_str) {
            Some("ERangeBoundTypes::Exclusive") => Ok(0.0),
            Some("ERangeBoundTypes::Inclusive") => Ok(1.0),
            Some("ERangeBoundTypes::Open") => Ok(2.0),
            other => failed(format!("ui: range bound type {}", other.unwrap_or("undefined"))),
        },
    }
}

/// A frame number of a bound (`b.Value.Value ?? 0`).
fn frame(b: &Json) -> Result<Json, KindError> {
    let Some(v) = get(b, "Value") else { return failed("ui: a range bound without a Value".into()) };
    Ok(match get(v, "Value") {
        Some(Json::Null) | None => Json::Num(0.0),
        Some(x) => x.clone(),
    })
}

/// A weighted key keeps its two weights and the mode (`ERichCurveTangentWeightMode`).
fn weight_mode(name: &str) -> Option<f64> {
    match name {
        "RCTWM_WeightedNone" => Some(0.0),
        "RCTWM_WeightedArrive" => Some(1.0),
        "RCTWM_WeightedLeave" => Some(2.0),
        "RCTWM_WeightedBoth" => Some(3.0),
        _ => None,
    }
}

fn interp_mode(name: &str) -> Option<f64> {
    match name {
        "RCIM_Linear" => Some(0.0),
        "RCIM_Constant" => Some(1.0),
        "RCIM_Cubic" => Some(2.0),
        "RCIM_None" => Some(3.0),
        _ => None,
    }
}

/// `undefined` in an array is `null` in JSON.
fn or_null(v: Option<&Json>) -> Json {
    v.cloned().unwrap_or(Json::Null)
}

/// FMovieSceneFloatValue: [value, interp mode, arrive tangent, leave tangent] and, for a weighted key, the two weights
/// and the mode (`floatKey`).
fn float_key(x: &Json) -> Result<Json, KindError> {
    let t = get(x, "Tangent").unwrap_or(&Json::Null);
    let interp = match get(x, "InterpMode") {
        Some(Json::Str(s)) => match interp_mode(s) {
            Some(n) => Json::Num(n),
            None => return failed(format!("ui: interp mode {s}")),
        },
        Some(m) => m.clone(),
        None => return failed("ui: interp mode undefined".into()),
    };
    let mut k =
        vec![or_null(get(x, "Value")), interp, or_null(get(t, "ArriveTangent")), or_null(get(t, "LeaveTangent"))];
    let wm = match get(t, "TangentWeightMode") {
        Some(Json::Str(s)) => match weight_mode(s) {
            Some(n) => Json::Num(n),
            None => return failed(format!("ui: tangent weight mode {s}")),
        },
        Some(Json::Null) | None => Json::Num(0.0),
        Some(m) => m.clone(),
    };
    if truthy(Some(&wm)) {
        k.push(or_null(get(t, "ArriveTangentWeight")));
        k.push(or_null(get(t, "LeaveTangentWeight")));
        k.push(wm);
    }
    Ok(Json::Arr(k))
}

/// A channel (`Channel`: `{ t, v, pre, post, def }`).
fn channel(v: &Json, values: Vec<Json>) -> Json {
    let mut c = Map::new();
    let times = match get(v, "Times") {
        Some(Json::Arr(a)) => a.iter().map(|x| or_null(get(x, "Value"))).collect(),
        _ => Vec::new(),
    };
    c.insert("t".into(), Json::Arr(times));
    c.insert("v".into(), Json::Arr(values));
    for (k, from) in [("pre", "PreInfinityExtrap"), ("post", "PostInfinityExtrap")] {
        if let Some(x) = get(v, from) {
            c.insert(k.into(), x.clone());
        }
    }
    if truthy(get(v, "bHasDefaultValue")) {
        if let Some(d) = get(v, "DefaultValue") {
            c.insert("def".into(), d.clone());
        }
    } else {
        c.insert("def".into(), Json::Null);
    }
    Json::Obj(c)
}

/// A WidgetAnimation: tracks bound to widgets by AnimationBindings, the master event tracks' keys (`animation`).
fn animation(a: &Obj<'_>, w: &WidgetPackage<'_>) -> Result<Json, KindError> {
    let Some(ms) = w.at(mget(&a.props, "MovieScene")) else {
        return failed(format!("ui: {}: no movie scene", a.path()));
    };
    let mut bind: HashMap<String, String> = HashMap::new();
    if let Some(Json::Arr(bs)) = mget(&a.props, "AnimationBindings") {
        for b in bs {
            let guid = gets(b, "AnimationGuid").unwrap_or("[object Object]").to_owned();
            let to =
                if truthy(get(b, "bIsRootWidget")) { "$root" } else { gets(b, "WidgetName").unwrap_or("undefined") };
            bind.insert(guid, to.to_owned());
        }
    }
    let Some(pr) = mget(&ms.props, "PlaybackRange").and_then(|r| get(r, "Value")) else {
        return failed(format!("ui: {}: no playback range", ms.path()));
    };
    let mut tracks = Vec::new();
    if let Some(Json::Arr(obs)) = mget(&ms.props, "ObjectBindings") {
        for ob in obs {
            let Some(Json::Arr(trs)) = get(ob, "Tracks") else { continue };
            for tr in trs {
                let Some(t) = w.at(Some(tr)) else {
                    return failed(format!("ui: {}: a track that is not in the package", a.path()));
                };
                let pb = mget(&t.props, "PropertyBinding").filter(|p| truthy(Some(p)));
                let mut sections = Vec::new();
                if let Some(Json::Arr(srs)) = mget(&t.props, "Sections") {
                    for sr in srs {
                        let Some(sec) = w.at(Some(sr)) else {
                            return failed(format!("ui: {}: a section that is not in the package", t.path()));
                        };
                        sections.push(section(&sec.props)?);
                    }
                }
                let mut tk = Map::new();
                let widget = gets(ob, "ObjectGuid")
                    .and_then(|g| bind.get(g).map(|s| Json::from(s.as_str())))
                    .or_else(|| get(ob, "BindingName").cloned());
                if let Some(wd) = widget {
                    tk.insert("widget".into(), wd);
                }
                tk.insert("type".into(), Json::from(short_name(t.class())));
                match pb {
                    Some(p) => {
                        if let Some(v) = get(p, "PropertyPath") {
                            tk.insert("prop".into(), v.clone());
                        }
                    }
                    None => {
                        tk.insert("prop".into(), Json::Null);
                    }
                }
                tk.insert("sections".into(), Json::Arr(sections));
                tracks.push(Json::Obj(tk));
            }
        }
    }
    // master event tracks (MovieSceneEventTriggerSection): [key time, the entry point function's name]
    let mut events = Vec::new();
    if let Some(Json::Arr(trs)) = mget(&ms.props, "MasterTracks") {
        for tr in trs {
            let Some(t) = w.at(Some(tr)) else { continue };
            if short_name(t.class()) != "MovieSceneEventTrack" {
                continue;
            }
            let Some(Json::Arr(srs)) = mget(&t.props, "Sections") else { continue };
            for sr in srs {
                let Some(ch) = w.at(Some(sr)).and_then(|s| mget(&s.props, "EventChannel")).filter(|c| truthy(Some(c)))
                else {
                    continue;
                };
                let keys = get(ch, "KeyTimes").and_then(Json::as_array).unwrap_or(&[]);
                let vals = get(ch, "KeyValues").and_then(Json::as_array).unwrap_or(&[]);
                for (i, k) in keys.iter().enumerate() {
                    let f = vals
                        .get(i)
                        .and_then(|v| get(v, "Ptrs"))
                        .and_then(|p| get(p, "Function"))
                        .and_then(Json::as_str);
                    events.push(Json::Arr(vec![or_null(get(k, "Value")), Json::from(short_name(f.unwrap_or("")))]));
                }
            }
        }
    }
    let mut out = Map::new();
    out.insert("res".into(), Json::Num(60000.0));
    out.insert(
        "range".into(),
        Json::Arr(vec![
            frame(get(pr, "LowerBound").unwrap_or(&Json::Null))?,
            frame(get(pr, "UpperBound").unwrap_or(&Json::Null))?,
        ]),
    );
    out.insert("tracks".into(), Json::Arr(tracks));
    if !events.is_empty() {
        out.insert("events".into(), Json::Arr(events));
    }
    Ok(Json::Obj(out))
}

/// One section of a track: its channels and its range.
fn section(sp: &Map) -> Result<Json, KindError> {
    let mut ch = Map::new();
    for (k, v) in sp {
        if !is_obj(v) || !truthy(get(v, "Times")) {
            continue;
        }
        let values = match get(v, "Values") {
            Some(Json::Arr(a)) => a
                .iter()
                .map(|x| {
                    if is_obj(x) && truthy(get(x, "Tangent")) {
                        float_key(x)
                    } else {
                        let n = match x {
                            Json::Obj(_) if get(x, "Value").is_some() => {
                                js_number(get(x, "Value").unwrap_or(&Json::Null))
                            }
                            other => js_number(other),
                        };
                        Ok(Json::Arr(vec![Json::Num(n), Json::Num(1.0), Json::Num(0.0), Json::Num(0.0)]))
                    }
                })
                .collect::<Result<Vec<_>, KindError>>()?,
            _ => return failed("ui: a channel without Values".into()),
        };
        ch.insert(k.clone(), channel(v, values));
    }
    // MovieSceneParameterSection (widget material tracks): one channel per scalar parameter, keyed 'S:<name>'
    if let Some(Json::Arr(list)) = mget(sp, "ScalarParameterNamesAndCurves") {
        for x in list {
            let curve = get(x, "ParameterCurve").unwrap_or(&Json::Null);
            let values = match get(curve, "Values") {
                Some(Json::Arr(a)) => a.iter().map(float_key).collect::<Result<Vec<_>, _>>()?,
                _ => return failed("ui: a scalar parameter curve without Values".into()),
            };
            let name = gets(x, "ParameterName").unwrap_or("undefined");
            ch.insert(format!("S:{name}"), channel(curve, values));
        }
    }
    let r = mget(sp, "SectionRange").and_then(|r| get(r, "Value"));
    let (lo, hi) = (r.and_then(|r| get(r, "LowerBound")), r.and_then(|r| get(r, "UpperBound")));
    let edge = |b: Option<&Json>| -> Result<Json, KindError> {
        let Some(b) = b else { return failed("ui: a section range without its bounds".into()) };
        Ok(if bound_type(b)? == 2.0 { Json::Null } else { frame(b)? })
    };
    let range = if r.is_some() { vec![edge(lo)?, edge(hi)?] } else { vec![Json::Null, Json::Null] };
    let mut s = Map::new();
    s.insert("range".into(), Json::Arr(range));
    s.insert("ch".into(), Json::Obj(ch));
    if let Some(hi) = hi.filter(|h| truthy(Some(h)))
        && bound_type(hi)? == 0.0
    {
        s.insert("excl".into(), Json::Bool(true));
    }
    if let Some(mask) = mget(sp, "TransformMask").filter(|m| truthy(Some(m)))
        && let Some(m) = get(mask, "Mask")
    {
        s.insert("mask".into(), m.clone());
    }
    Ok(Json::Obj(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_paths() {
        assert!(is_object_path("/Game/A/B.B"));
        assert!(is_object_path("/Script/UMG.TextBlock"));
        assert!(is_object_path("/Game/A/B.B_C:Comp.X"));
        assert!(!is_object_path("/Game/A/B"));
        assert!(!is_object_path("/Game/A B/C.C"));
        assert!(!is_object_path("/Game/A/B."));
        assert!(!is_object_path("/Other/A.B"));
    }

    #[test]
    fn shapes() {
        let shape_in = |s: &str| Json::parse(s.as_bytes()).unwrap();
        // a text history in the runtime's form; a reference as its short name; Hex dropped from a colour
        let v = shape_in(
            r#"{"t":{"namespace":"N","key":"K","source":"S"},"r":"/Game/A/B.B_C","c":{"R":1,"Hex":"FF"},"n":[1,"x"]}"#,
        );
        let Json::Obj(m) = &v else { panic!("an object") };
        assert_eq!(
            crate::json::to_vec(&text_of(&m["t"])).unwrap(),
            br#"{"Namespace":"N","Key":"K","SourceString":"S","LocalizedString":"S"}"#
        );
        assert_eq!(text_of(&shape_in(r#"{"source":null}"#)).get("CultureInvariantString"), Some(&Json::Null));
        assert!(truthy(Some(&Json::Arr(vec![]))) && !truthy(Some(&Json::Str(String::new()))));
        let keyed = IndexMap::from([
            ("b".to_owned(), Json::Num(1.0)),
            ("2".to_owned(), Json::Num(2.0)),
            ("1".to_owned(), Json::Num(3.0)),
        ]);
        let order: Vec<&Json> = values_in_order(&keyed);
        assert_eq!(order, [&Json::Num(3.0), &Json::Num(2.0), &Json::Num(1.0)]);
        assert_eq!(
            float_key(&shape_in(
                r#"{"Value":1,"InterpMode":"RCIM_Cubic","Tangent":{"ArriveTangent":2,"LeaveTangent":3}}"#
            ))
            .unwrap(),
            shape_in("[1,2,2,3]")
        );
        assert!(float_key(&shape_in(r#"{"Value":1,"Tangent":{}}"#)).is_err());
    }
}
