//! Actor classes bound from the build: a class's def (its defaults and its components' templates, shaped), the class
//! chain and interfaces, and the raw values the worklists follow.
//!
//! A spawned actor class's data as the fighter data's `actors` and the perk parts' `actors` hold it: `p`, its class
//! defaults merged over its Blueprint parents'; `components`, its component templates by name; `supers`, the class
//! chain; `ifaces`, its interfaces. Each component template is merged over the same-named one of its parents (the
//! default subobjects of a native parent's constructor saved on the class default object, the SCS and inherited
//! `_GEN_VARIABLE` templates, the AddComponent nodes' templates, in the export order of its package; then the
//! construction script tree's AttachParent and socket). Read from the build's objects (the class and its default
//! object with their inner objects), shaped as the tables hold them ([`ActorClasses::dump_shape`]). The raw values
//! (object paths) ride beside the shaped ones: the worklist follows the classes they name.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use mvs_data::fighter::ActorDef;
use mvs_data::{Props, Value};
use mvs_math::libm;

use super::shape::{
    EnumTable, cdo_path, deep_update, enum_value_name, enums_for, fail, native, properties, short_name,
};
use crate::kind::{Cx, KindError};
use crate::objview::ObjView;

/// A declared member (a class's or struct's ChildProperties entry, as the reader writes it); `ClassMembers` is the
/// binder's own: the members of the class or struct `strukt` names.
#[derive(Clone, Debug, PartialEq)]
pub struct Desc {
    pub ty: String,
    pub name: String,
    pub inner: Option<Box<Desc>>,
    pub element_prop: Option<Box<Desc>>,
    pub key_prop: Option<Box<Desc>>,
    pub value_prop: Option<Box<Desc>>,
    pub strukt: Option<String>,
}

impl Desc {
    /// The binder's `{ Type: 'ClassMembers', Name: '', Struct: cls }`.
    pub fn class_members(cls: &str) -> Desc {
        Desc {
            ty: "ClassMembers".into(),
            name: String::new(),
            inner: None,
            element_prop: None,
            key_prop: None,
            value_prop: None,
            strukt: Some(cls.to_owned()),
        }
    }

    /// A ChildProperties entry as the object kind's JSON holds it.
    fn of(v: &Value) -> Option<Desc> {
        let o = v.as_obj()?;
        let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_owned);
        let sub = |k: &str| o.get(k).and_then(Desc::of).map(Box::new);
        Some(Desc {
            ty: s("Type").unwrap_or_default(),
            name: s("Name").unwrap_or_default(),
            inner: sub("Inner"),
            element_prop: sub("ElementProp"),
            key_prop: sub("KeyProp"),
            value_prop: sub("ValueProp"),
            strukt: s("Struct"),
        })
    }
}

/// An actor class bound from the build: its def, and the same merges over the raw values.
#[derive(Clone, Debug)]
pub struct ActorClass {
    pub path: String,
    pub def: ActorDef,
    /// The class default object's merged defaults (object paths).
    pub raw_p: Props,
    /// The components' merged template properties (object paths).
    pub raw_components: IndexMap<String, Props>,
    /// Each component's class (an object path: `/Script/Engine.SkeletalMeshComponent`), by the raw_components name.
    pub component_class: IndexMap<String, String>,
}

/// A class's declared members by name.
type Members = IndexMap<String, Desc>;

/// Binds actor classes: the objects a class and its parents need, the enum assets their values name, then the
/// shaping. A class is bound once per binder. `key` names the bind in its errors (a character key, `items`, the
/// perk part's).
pub struct ActorClasses<'a> {
    pub cx: Cx<'a>,
    pub key: String,
    done: Mutex<IndexMap<String, Arc<ActorClass>>>,
    members: Mutex<IndexMap<String, Arc<Members>>>,
}

const SCS_NODE: &str = "/Script/Engine.SCS_Node";
const TEXT_KEYS: [&str; 4] = ["source", "namespace", "key", "table"];

/// The last element of an inner object's name chain (`SimpleConstructionScript_0.SCS_Node_3` -> `SCS_Node_3`).
fn last_name(chain: &str) -> &str {
    &chain[chain.rfind('.').map_or(0, |i| i + 1)..]
}

/// A UserDefinedStruct member's name without its GUID tail (`Actor_7_1B834F2F451C03B4A3F93BB2B909BC7B` -> `Actor`:
/// `_\d+_[0-9A-F]{32}$`).
fn without_guid_tail(n: &str) -> &str {
    let b = n.as_bytes();
    if b.len() < 35 || !b[b.len() - 32..].iter().all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(c)) {
        return n;
    }
    let head = &n[..n.len() - 32];
    let Some(head) = head.strip_suffix('_') else { return n };
    let digits = head.bytes().rev().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return n;
    }
    head[..head.len() - digits].strip_suffix('_').unwrap_or(n)
}

/// An object path (a hard or soft reference's value): `^\/[A-Za-z0-9_]+\/.*\.`.
fn is_ref(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('/') else { return false };
    let n = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
    // (`.` stops at a line terminator)
    let line = |r: &str| {
        r.split(|c: char| matches!(c as u32, 0x0a | 0x0d | 0x2028 | 0x2029)).next().unwrap_or_default().contains('.')
    };
    n > 0 && rest[n..].strip_prefix('/').is_some_and(line)
}

/// A JavaScript value's truth.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Num(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        Value::Arr(_) | Value::Obj(_) | Value::F32s(_) => true,
    }
}

/// A number as JavaScript's String(n).
fn js_string(n: f64) -> String {
    ryu_js::Buffer::new().format(n).to_owned()
}

/// Its properties without the ubergraph frame.
fn without_frame(mut p: Props) -> Props {
    p.shift_remove("UberGraphFrame");
    p
}

/// The members of `p[name]`, an object (empty when none).
fn obj_mut<'p>(p: &'p mut Props, name: &str) -> &'p mut Props {
    let v = p.entry(name.to_owned()).or_insert_with(|| Value::Obj(Props::new()));
    if !matches!(v, Value::Obj(_)) {
        *v = Value::Obj(Props::new());
    }
    match v {
        Value::Obj(o) => o,
        _ => unreachable!("made an object above"),
    }
}

impl<'a> ActorClasses<'a> {
    pub fn new(cx: Cx<'a>, key: &str) -> ActorClasses<'a> {
        ActorClasses { cx, key: key.to_owned(), done: Mutex::default(), members: Mutex::default() }
    }

    /// The enum tables of the UserDefinedEnums `vals` hold values of.
    pub fn take_enums(&self, vals: &[&Value]) -> Result<EnumTable, KindError> {
        enums_for(&self.cx, &self.key, vals)
    }

    /// A class default object's merged defaults, shaped by its declared members, without its ubergraph frame (a
    /// buff's def).
    pub fn class_defaults(&self, cls: &str) -> Result<Props, KindError> {
        let d = Value::Obj(without_frame(self.raw_defaults(cls)?));
        let enums = self.take_enums(&[&d])?;
        match self.dump_shape(&d, Some(&Desc::class_members(cls)), &enums)? {
            Value::Obj(p) => Ok(p),
            _ => unreachable!("an object shapes to an object"),
        }
    }

    /// The raw merged defaults of a class (object paths).
    pub fn raw_defaults(&self, cls: &str) -> Result<Props, KindError> {
        let cdo = self.cx.object(&cdo_path(cls))?;
        super::shape::defaults_or_properties(&cdo)
    }

    /// The declared members of a Blueprint class (its own and its Blueprint parents') or a UserDefinedStruct, by
    /// name (a struct member's name without its GUID tail); a native class or struct has none here.
    fn members_of(&self, path: &str) -> Result<Arc<Members>, KindError> {
        if let Some(m) = self.members.lock().unwrap_or_else(|e| e.into_inner()).get(path) {
            return Ok(m.clone());
        }
        let mut out = Members::new();
        let mut c = Some(path.to_owned());
        while let Some(p) = c.take().filter(|p| p.starts_with("/Game/")) {
            let o = self.cx.object(&p)?;
            let n = native(&o)?.unwrap_or_default();
            for d in n.get("ChildProperties").and_then(Value::as_arr).unwrap_or_default() {
                let Some(d) = Desc::of(d) else { continue };
                let name = without_guid_tail(&d.name).to_owned();
                out.entry(name).or_insert(d);
            }
            c = n.get("SuperStruct").and_then(Value::as_str).map(str::to_owned);
        }
        let out = Arc::new(out);
        self.members.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_owned(), out.clone());
        Ok(out)
    }

    /// A value as the tables hold it: a hard reference as its last name component; a soft
    /// one as {AssetPathName, SubPathString} (a null one as AssetPathName None); a map as {Key, Value} entries, a
    /// reference key as its full path and an int key as its digits; a text as {Namespace, Key, SourceString,
    /// LocalizedString}, a culture-invariant one as its string; a UserDefinedEnum value as its byte; an FColor
    /// without its Hex. The declared type (`d`: a Blueprint class's or UserDefinedStruct's member) tells soft from
    /// hard and an int key; a native struct's members have none here and read as hard.
    pub fn dump_shape(&self, v: &Value, d: Option<&Desc>, enums: &EnumTable) -> Result<Value, KindError> {
        let t = d.map(|d| d.ty.as_str());
        let soft_pair = |path: &str| {
            Value::Obj(Props::from([
                ("AssetPathName".to_owned(), Value::Str(path.to_owned())),
                ("SubPathString".to_owned(), Value::Str(String::new())),
            ]))
        };
        if matches!(t, Some("SoftClassProperty" | "SoftObjectProperty")) {
            match v {
                Value::Null => return Ok(soft_pair("None")),
                Value::Str(s) => return Ok(soft_pair(s)),
                _ => {}
            }
        }
        Ok(match v {
            Value::Null | Value::Num(_) | Value::Bool(_) | Value::F32s(_) => v.clone(),
            Value::Str(s) => {
                if let Some(n) = enum_value_name(s) {
                    match enums.get(n).and_then(|t| t.get(s.as_str())) {
                        Some(b) => Value::Num(*b),
                        None => return fail(format!("{s}: not a value of a known enum asset")),
                    }
                } else if matches!(t, Some("StrProperty" | "NameProperty")) {
                    v.clone()
                } else if is_ref(s) {
                    // Unverified: a member with no declared type here (a native class's or a native struct's) is read
                    // as a hard reference, its last name component; a soft reference there would be the dump's
                    // {AssetPathName, SubPathString}. None of today's data holds one; the native types are not read.
                    Value::Str(short_name(s).to_owned())
                } else {
                    v.clone()
                }
            }
            Value::Arr(a) => {
                let pairs = !a.is_empty() && a.iter().all(|e| matches!(e, Value::Arr(p) if p.len() == 2));
                if t == Some("MapProperty") || (d.is_none() && pairs) {
                    let kd = d.and_then(|d| d.key_prop.as_deref());
                    let vd = d.and_then(|d| d.value_prop.as_deref());
                    let mut out = Vec::with_capacity(a.len());
                    for e in a {
                        let (k, x) = match e.as_arr() {
                            Some([k, x]) => (k, x),
                            _ => return fail(format!("a map entry is no [key, value] pair ({})", self.key)),
                        };
                        let key = match k {
                            Value::Str(s) if is_ref(s) => k.clone(),
                            Value::Num(n) if kd.is_some_and(|kd| kd.ty == "IntProperty") => Value::Str(js_string(*n)),
                            _ => self.dump_shape(k, kd, enums)?,
                        };
                        out.push(Value::Obj(Props::from([
                            ("Key".to_owned(), key),
                            ("Value".to_owned(), self.dump_shape(x, vd, enums)?),
                        ])));
                    }
                    Value::Arr(out)
                } else {
                    let inner = d.and_then(|d| d.inner.as_deref().or(d.element_prop.as_deref()));
                    Value::Arr(a.iter().map(|x| self.dump_shape(x, inner, enums)).collect::<Result<_, _>>()?)
                }
            }
            Value::Obj(o) => {
                let color = ["R", "G", "B", "A"].iter().all(|k| o.contains_key(*k));
                let keys: Vec<&String> = o.keys().filter(|k| *k != "Hex" || !color).collect();
                if o.contains_key("source") && keys.iter().all(|k| TEXT_KEYS.contains(&k.as_str())) {
                    // (an empty text: source null, as the dump's null)
                    if keys.len() == 1 {
                        return Ok(o["source"].clone());
                    }
                    if let Some(table) = o.get("table") {
                        return fail(format!(
                            "a string table text ({}): its dump form is not known here",
                            super::shape::show(table)
                        ));
                    }
                    // Unverified: LocalizedString is written as the source string: the reader keeps no localized
                    // string of its own, and every text of today's data has the two equal (the dump's base-culture
                    // text); a text whose package held another would differ.
                    let or_empty = |k: &str| match o.get(k) {
                        Some(Value::Null) | None => Value::Str(String::new()),
                        Some(v) => v.clone(),
                    };
                    return Ok(Value::Obj(Props::from([
                        ("Namespace".to_owned(), or_empty("namespace")),
                        ("Key".to_owned(), or_empty("key")),
                        ("SourceString".to_owned(), o["source"].clone()),
                        ("LocalizedString".to_owned(), o["source"].clone()),
                    ])));
                }
                let members = match d {
                    Some(d)
                        if d.ty == "ClassMembers"
                            || (d.ty == "StructProperty"
                                && d.strukt.as_deref().is_some_and(|s| s.starts_with("/Game/"))) =>
                    {
                        Some(self.members_of(d.strukt.as_deref().unwrap_or_default())?)
                    }
                    _ => None,
                };
                let mut out = Props::new();
                // (a member name as the dump wrote it: without the trailing space a Blueprint variable's name may
                // have, ShadowClone_Parent_Projectile's "AK Projectile Death ")
                for k in keys {
                    let kd = members.as_ref().and_then(|m| m.get(k.as_str()));
                    out.insert(k.trim_end().to_owned(), self.dump_shape(&o[k], kd, enums)?);
                }
                Value::Obj(out)
            }
        })
    }

    /// The class `cls` bound, once per binder.
    pub fn actor(&self, cls: &str) -> Result<Arc<ActorClass>, KindError> {
        let key = cls.to_lowercase();
        if let Some(a) = self.done.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(a.clone());
        }
        let a = Arc::new(self.bind(cls)?);
        Ok(self.done.lock().unwrap_or_else(|e| e.into_inner()).entry(key).or_insert(a).clone())
    }

    fn bind(&self, cls: &str) -> Result<ActorClass, KindError> {
        let k = self.cx.object(cls)?;
        let cdo = self.cx.object(&cdo_path(cls))?;
        let k_native = native(&k)?;
        let sup = match k.super_path()? {
            Some(s) => Some(s),
            None => k_native.as_ref().and_then(|n| n.get("SuperStruct")).and_then(Value::as_str).map(str::to_owned),
        };
        let (mut res, supers) = match sup.as_deref() {
            Some(s) if s.starts_with("/Game/") && short_name(s) != short_name(cls) => {
                let parent = self.actor(s)?;
                let mut res = (*parent).clone();
                res.path = cls.to_owned();
                let supers = parent.def.supers.as_ref().map(|ps| format!("{} {ps}", short_name(s)));
                (res, supers)
            }
            _ => {
                let res = ActorClass {
                    path: cls.to_owned(),
                    def: ActorDef { p: Props::new(), components: Props::new(), supers: None, ifaces: None },
                    raw_p: Props::new(),
                    raw_components: IndexMap::new(),
                    component_class: IndexMap::new(),
                };
                (res, sup.as_deref().map(|s| short_name(s).to_owned()))
            }
        };
        let itf = k_native.as_ref().and_then(|n| n.get("Interfaces")).and_then(Value::as_arr).unwrap_or_default();
        let own: Vec<String> =
            itf.iter().filter_map(|i| i.as_obj()?.get("Class")?.as_str().map(|c| short_name(c).to_owned())).collect();
        let mut ifaces = own.clone();
        for i in res.def.ifaces.as_deref().unwrap_or_default().split(' ') {
            if !i.is_empty() && !own.iter().any(|o| o == i) {
                ifaces.push(i.to_owned());
            }
        }
        res.def.supers = supers;
        res.def.ifaces = (!ifaces.is_empty()).then(|| ifaces.join(" "));

        // the class default object over the parents'
        let own_p = without_frame(properties(&cdo)?);
        let mut inners: Vec<(ObjView<'a>, bool)> = k.inner()?.into_iter().map(|o| (o, false)).collect();
        inners.extend(cdo.inner()?.into_iter().map(|o| (o, true)));
        inners.sort_by_key(|(o, _)| o.index());
        let props: Vec<Props> = inners.iter().map(|(o, _)| properties(o)).collect::<Result<_, _>>()?;
        let own_v = Value::Obj(own_p);
        let prop_vs: Vec<Value> = props.iter().map(|p| Value::Obj(p.clone())).collect();
        let mut vals: Vec<&Value> = vec![&own_v];
        vals.extend(prop_vs.iter());
        let enums = self.take_enums(&vals)?;
        let Value::Obj(shaped) = self.dump_shape(&own_v, Some(&Desc::class_members(cls)), &enums)? else {
            unreachable!("an object shapes to an object")
        };
        deep_update(&mut res.def.p, &shaped);
        let Value::Obj(own_p) = own_v else { unreachable!() };
        deep_update(&mut res.raw_p, &own_p);
        let comp = |res: &mut ActorClass, name: &str, o: &ObjView<'_>, p: &Props| -> Result<(), KindError> {
            let shaped = self.dump_shape(&Value::Obj(p.clone()), Some(&Desc::class_members(o.class())), &enums)?;
            let Value::Obj(shaped) = shaped else { unreachable!("an object shapes to an object") };
            deep_update(obj_mut(&mut res.def.components, name), &shaped);
            deep_update(res.raw_components.entry(name.to_owned()).or_default(), p);
            res.component_class.insert(name.to_owned(), o.class().to_owned());
            Ok(())
        };
        for ((o, on_cdo), p) in inners.iter().zip(&props) {
            if p.is_empty() {
                continue;
            }
            let chain = o.chain();
            let name = last_name(chain);
            // a default subobject of a native parent's constructor, saved as a delta on the class default object
            // (ItemHelper_BananaGuard_C, a native Character: CollisionCylinder, CharMoveComp, CharacterMesh0), under
            // its own name; its AttachParent is the property itself (a default subobject has no SCS node)
            if *on_cdo && !chain.contains('.') {
                comp(&mut res, chain, o, p)?;
            } else if let Some(n) = name.strip_suffix("_GEN_VARIABLE") {
                comp(&mut res, n, o, p)?;
            } else if !*on_cdo && !chain.contains('.') && name.starts_with("NODE_Add") {
                // an AddComponent node's template (C019Marker's NODE_AddSplineMeshComponent-0), under its own name
                comp(&mut res, name, o, p)?;
            }
        }
        // The construction script's tree: AttachParent = the component a node hangs under (a node's ChildNodes, or a
        // root node's ParentComponentOrVariableName in a parent class); socket = the socket (or bone) of the parent it
        // hangs on (ItemHelper_BananaGuard's SpearHitBox on CharacterMesh0's Spear_Tip).
        let mut nodes: IndexMap<String, &Props> = IndexMap::new();
        for ((o, on_cdo), p) in inners.iter().zip(&props) {
            if !*on_cdo && o.class() == SCS_NODE && !p.is_empty() {
                nodes.insert(last_name(o.chain()).to_owned(), p);
            }
        }
        let var_of = |n: &str| match nodes.get(n).and_then(|p| p.get("InternalVariableName")) {
            Some(Value::Str(v)) if v != "None" => Some(v.clone()),
            _ => None,
        };
        let set = |res: &mut ActorClass, name: &str, key: &str, v: &str| {
            obj_mut(&mut res.def.components, name).insert(key.to_owned(), Value::Str(v.to_owned()));
            res.raw_components.entry(name.to_owned()).or_default().insert(key.to_owned(), Value::Str(v.to_owned()));
        };
        for (n, p) in &nodes {
            let vn = var_of(n);
            if let (Some(Value::Str(pc)), Some(vn)) = (p.get("ParentComponentOrVariableName"), &vn)
                && !pc.is_empty()
            {
                set(&mut res, vn, "AttachParent", pc);
            }
            if let (Some(Value::Str(at)), Some(vn)) = (p.get("AttachToName"), &vn)
                && !at.is_empty()
            {
                set(&mut res, vn, "socket", at);
            }
            for c in p.get("ChildNodes").and_then(Value::as_arr).unwrap_or_default() {
                let child = match c {
                    Value::Str(s) => scs_node_name(s),
                    _ => None,
                };
                let vc = child.filter(|ch| nodes.contains_key(*ch)).and_then(var_of);
                if let (Some(vc), Some(vn)) = (vc, &vn) {
                    set(&mut res, &vc, "AttachParent", vn);
                }
            }
        }
        self.welded_mesh_mass(&mut res)?;
        Ok(res)
    }

    /// A static mesh component of a simulating actor that is welded into it (its CollisionEnabled PhysicsOnly or
    /// QueryAndPhysics, not simulating itself, attached under a simulating component: FBodyInstance::Weld) gets its
    /// mesh's simple collision as mass properties at unit density and scale, for UpdateMassProperties:
    /// Hull{Volume, CX, CY, CZ, Ixx, Iyy, Izz, Ixy, Ixz, Iyz}, the inertia about the centre. Flat numbers: an object
    /// here would renumber the character's static ids. Only ConvexElems with bContributeToMass and an identity
    /// Transform are read; any other element is an error. SmokeGrenade's MineMesh (SM_BatmanBomb01: one hull, volume
    /// 4918.91).
    fn welded_mesh_mass(&self, a: &mut ActorClass) -> Result<(), KindError> {
        let comps = &a.def.components;
        let body_of = |c: Option<&Value>| -> Props {
            c.and_then(Value::as_obj)
                .and_then(|c| c.get("BodyInstance"))
                .and_then(Value::as_obj)
                .cloned()
                .unwrap_or_default()
        };
        let sim = |c: &str| body_of(comps.get(c)).get("bSimulatePhysics").is_some_and(truthy);
        let mut writes: Vec<(String, [f64; 10])> = Vec::new();
        for (name, c) in comps {
            let bi = body_of(Some(c));
            let Some(c) = c.as_obj() else { continue };
            if !matches!(c.get("StaticMesh"), Some(Value::Str(_))) || bi.get("bSimulatePhysics").is_some_and(truthy) {
                continue;
            }
            let en = bi.get("CollisionEnabled").and_then(Value::as_str);
            if !matches!(en, Some("ECollisionEnabled::PhysicsOnly" | "ECollisionEnabled::QueryAndPhysics")) {
                continue;
            }
            let mut p = match c.get("AttachParent") {
                Some(Value::Str(s)) if !s.is_empty() => Some(s.clone()),
                _ => None,
            };
            let mut seen: Vec<String> = Vec::new();
            while let Some(q) = p.clone().filter(|q| !seen.contains(q) && !sim(q)) {
                p = match comps.get(&q).and_then(Value::as_obj).and_then(|c| c.get("AttachParent")) {
                    Some(Value::Str(s)) if !s.is_empty() => Some(s.clone()),
                    _ => None,
                };
                seen.push(q);
            }
            if !p.as_deref().is_some_and(sim) {
                continue;
            }
            let Some(Value::Str(mesh_path)) = a.raw_components.get(name).and_then(|r| r.get("StaticMesh")) else {
                return fail(format!("welded mesh {name}: no StaticMesh path"));
            };
            let mesh = self.cx.object(mesh_path)?;
            let Some(bs) = mesh.inner()?.into_iter().find(|o| o.class() == "/Script/Engine.BodySetup") else {
                return fail(format!("welded mesh {name}: no BodySetup for {mesh_path}"));
            };
            let bsp = properties(&bs)?;
            let agg = bsp.get("AggGeom").and_then(Value::as_obj).cloned().unwrap_or_default();
            let other = ["SphereElems", "BoxElems", "SphylElems", "TaperedCapsuleElems"]
                .iter()
                .any(|k| agg.get(*k).and_then(Value::as_arr).is_some_and(|l| !l.is_empty()));
            if other {
                return fail(format!("welded mesh {name}: {mesh_path} has elements other than hulls (not read)"));
            }
            let mut tot: Option<HullMass> = None;
            for e in agg.get("ConvexElems").and_then(Value::as_arr).unwrap_or_default() {
                let e = e.as_obj().cloned().unwrap_or_default();
                let t = e.get("Transform").and_then(Value::as_obj).cloned().unwrap_or_default();
                let off = |v: &str, k: &str, d: f64| {
                    let x = t.get(v).and_then(Value::as_obj).and_then(|v| v.get(k)).and_then(Value::as_f64);
                    (x.unwrap_or(d) - d).abs()
                };
                if ["X", "Y", "Z"].iter().any(|k| {
                    off("Translation", k, 0.0) > 1e-6 || off("Rotation", k, 0.0) > 1e-6 || off("Scale3D", k, 1.0) > 1e-6
                }) {
                    return fail(format!("welded mesh {name}: a hull of {mesh_path} is moved or scaled (not read)"));
                }
                if e.get("bContributeToMass") == Some(&Value::Bool(false)) {
                    continue;
                }
                if tot.is_some() {
                    return fail(format!("welded mesh {name}: {mesh_path} has more than one hull (not summed)"));
                }
                let mut pts = Vec::new();
                for v in e.get("VertexData").and_then(Value::as_arr).unwrap_or_default() {
                    let c = |k: &str| v.as_obj().and_then(|v| v.get(k)).and_then(Value::as_f64);
                    match (c("X"), c("Y"), c("Z")) {
                        (Some(x), Some(y), Some(z)) => pts.push([x, y, z]),
                        _ => return fail(format!("welded mesh {name}: a vertex of {mesh_path} is no vector")),
                    }
                }
                tot = Some(hull_mass(&pts));
            }
            let Some((vol, com, it)) = tot else { continue };
            let mut vals = [0.0; 10];
            vals[0] = vol;
            vals[1..4].copy_from_slice(&com);
            vals[4..].copy_from_slice(&it);
            writes.push((name.clone(), vals));
        }
        const HULL_KEYS: [&str; 10] = [
            "HullVolume",
            "HullCX",
            "HullCY",
            "HullCZ",
            "HullIxx",
            "HullIyy",
            "HullIzz",
            "HullIxy",
            "HullIxz",
            "HullIyz",
        ];
        for (name, vals) in writes {
            let c = obj_mut(&mut a.def.components, &name);
            for (k, v) in HULL_KEYS.iter().zip(vals) {
                c.insert((*k).to_owned(), Value::Num(round4(v)));
            }
        }
        Ok(())
    }
}

/// The first `SCS_Node_\d+` of a path.
fn scs_node_name(s: &str) -> Option<&str> {
    s.match_indices("SCS_Node_").find_map(|(at, m)| {
        let digits = s[at + m.len()..].bytes().take_while(u8::is_ascii_digit).count();
        (digits > 0).then(|| &s[at..at + m.len() + digits])
    })
}

/// The buff classes of an actor's Comp_Pawn_PassiveBuffManager component (its Buffs entries' Buff), in the list's
/// order, as object paths.
pub fn actor_passive_buffs(a: &ActorClass) -> Vec<String> {
    let Some(Value::Arr(l)) = a.raw_components.get("Comp_Pawn_PassiveBuffManager").and_then(|pm| pm.get("Buffs"))
    else {
        return Vec::new();
    };
    l.iter()
        .filter_map(|e| match e.as_obj().and_then(|e| e.get("Buff")) {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

// ---- round4, hull mass

/// The exact decimal expansion of `|v|` (a finite double below 2^64) cut to `f` fraction digits as JavaScript's
/// toFixed rounds it: the nearest, a tie to the larger.
fn to_fixed_abs(v: f64, f: usize) -> String {
    let v = v.abs();
    let bits = v.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i64;
    let frac = bits & ((1u64 << 52) - 1);
    let (m, e) = if exp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), exp - 1075) };
    // v = m * 2^e: as a decimal digit string of int . frac
    let (mut int_digits, frac_digits): (Vec<u8>, Vec<u8>) = if e >= 0 {
        let mut d = digits_of(m);
        for _ in 0..e {
            times(&mut d, 2);
        }
        (d, Vec::new())
    } else {
        // m / 2^k = m * 5^k / 10^k
        let k = (-e) as usize;
        let mut d = digits_of(m);
        for _ in 0..k {
            times(&mut d, 5);
        }
        while d.len() <= k {
            d.insert(0, 0);
        }
        let split = d.len() - k;
        (d[..split].to_vec(), d[split..].to_vec())
    };
    let mut fr: Vec<u8> = frac_digits.iter().copied().chain(std::iter::repeat(0)).take(f).collect();
    let rest = frac_digits.get(f..).unwrap_or_default();
    // (a tie, exactly half, rounds up: the larger n)
    if rest.first().is_some_and(|d| *d >= 5) {
        let mut i = fr.len();
        let mut carry = true;
        while carry && i > 0 {
            i -= 1;
            fr[i] += 1;
            carry = fr[i] == 10;
            if carry {
                fr[i] = 0;
            }
        }
        if carry {
            let mut j = int_digits.len();
            while carry && j > 0 {
                j -= 1;
                int_digits[j] += 1;
                carry = int_digits[j] == 10;
                if carry {
                    int_digits[j] = 0;
                }
            }
            if carry {
                int_digits.insert(0, 1);
            }
        }
    }
    let mut s: String = int_digits.iter().map(|d| char::from(b'0' + d)).collect();
    if s.is_empty() {
        s.push('0');
    }
    if f > 0 {
        s.push('.');
        s.extend(fr.iter().map(|d| char::from(b'0' + d)));
    }
    s
}

fn digits_of(mut m: u64) -> Vec<u8> {
    let mut d = Vec::new();
    while m > 0 {
        d.push((m % 10) as u8);
        m /= 10;
    }
    d.reverse();
    let first = d.iter().position(|x| *x != 0).unwrap_or(d.len());
    d.drain(..first);
    d
}

fn times(d: &mut Vec<u8>, k: u8) {
    let mut carry = 0u8;
    for x in d.iter_mut().rev() {
        let p = *x * k + carry;
        *x = p % 10;
        carry = p / 10;
    }
    if carry > 0 {
        d.insert(0, carry);
    }
}

/// JavaScript's v.toFixed(f) (a finite double below 1e21).
fn to_fixed(v: f64, f: usize) -> String {
    let s = to_fixed_abs(v, f);
    if v < 0.0 { format!("-{s}") } else { s }
}

/// Python's round(v, 4): the double nearest v rounded to 4 decimals, from v's exact decimal value, a tie (a value
/// exactly halfway, such as 0.03125) to the even last digit. toFixed rounds a tie away from zero: the exact
/// expansion tells a tie (a double that is no tie differs from one by far more than 1e-30 at these magnitudes).
pub fn round4(v: f64) -> f64 {
    let parse = |s: &str| s.parse::<f64>().unwrap_or(f64::NAN);
    let r = parse(&to_fixed(v, 4));
    let exact = to_fixed_abs(v, 30);
    let dot = exact.find('.').unwrap_or(exact.len());
    let b = exact.as_bytes();
    if b.get(dot + 5) != Some(&b'5') || exact[dot + 6..].bytes().any(|c| c != b'0') {
        return r;
    }
    let down = parse(&format!("{}{}", if v < 0.0 { "-" } else { "" }, &exact[..dot + 5]));
    let last = b[dot + 4] - b'0';
    if last.is_multiple_of(2) { down } else { r }
}

type V3 = [f64; 3];
/// (volume, centre, inertia about the centre as (xx, yy, zz, xy, xz, yz)).
type HullMass = (f64, [f64; 3], [f64; 6]);

/// A convex hull's mass properties at unit density. The hull of the points by brute force (a plane through three
/// points with every point on one side, within 1e-4), each face's points fanned round their centroid into tetrahedra
/// from the origin. The port's own integrator (FBodyInstance::UpdateMassProperties 142c82880).
pub fn hull_mass(pts: &[V3]) -> HullMass {
    let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let eps = 1e-4;
    let n = pts.len();
    let mut planes: Vec<(V3, f64)> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let mut nn = cross(sub(pts[j], pts[i]), sub(pts[k], pts[i]));
                let l = dot(nn, nn).sqrt();
                if l < 1e-9 {
                    continue;
                }
                nn = [nn[0] / l, nn[1] / l, nn[2] / l];
                let mut off = dot(nn, pts[i]);
                let s: Vec<f64> = pts.iter().map(|p| dot(nn, *p) - off).collect();
                let pos = s.iter().any(|x| *x > eps);
                let neg = s.iter().any(|x| *x < -eps);
                if pos && neg {
                    continue;
                }
                if pos {
                    nn = [-nn[0], -nn[1], -nn[2]];
                    off = -off;
                }
                if !planes.iter().any(|(q, o)| dot(*q, nn) > 1.0 - 1e-9 && (o - off).abs() < eps) {
                    planes.push((nn, off));
                }
            }
        }
    }
    let mut vol = 0.0;
    let mut first = [0.0; 3];
    let mut second = [[0.0; 3]; 3];
    for (nn, off) in &planes {
        let mut on: Vec<V3> = pts.iter().copied().filter(|p| (dot(*nn, *p) - off).abs() < eps).collect();
        let sum = |i: usize| {
            let mut t = 0.0;
            for p in &on {
                t += p[i];
            }
            t
        };
        let len = on.len() as f64;
        let c = [sum(0) / len, sum(1) / len, sum(2) / len];
        let mut u = sub(on[0], c);
        let ul = dot(u, u).sqrt();
        u = [u[0] / ul, u[1] / ul, u[2] / ul];
        let w = cross(*nn, u);
        let key = |p: &V3| libm::atan2(dot(sub(*p, c), w), dot(sub(*p, c), u));
        // (Array.prototype.sort with key(x) - key(y): stable)
        on.sort_by(|x, y| (key(x) - key(y)).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
        for t in 1..on.len().saturating_sub(1) {
            let (a, b, cc) = (on[0], on[t], on[t + 1]);
            let v = dot(a, cross(b, cc)) / 6.0;
            vol += v;
            for i in 0..3 {
                first[i] += (v * (a[i] + b[i] + cc[i])) / 4.0;
                for j in 0..3 {
                    second[i][j] += (v / 20.0)
                        * (a[i] * a[j] + b[i] * b[j] + cc[i] * cc[j] + (a[i] + b[i] + cc[i]) * (a[j] + b[j] + cc[j]));
                }
            }
        }
    }
    let com = first.map(|x| x / vol);
    let cov: [[f64; 3]; 3] = [0, 1, 2].map(|i| [0, 1, 2].map(|j| second[i][j] - vol * com[i] * com[j]));
    let tr = cov[0][0] + cov[1][1] + cov[2][2];
    (vol, com, [tr - cov[0][0], tr - cov[1][1], tr - cov[2][2], -cov[0][1], -cov[0][2], -cov[1][2]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(without_guid_tail("Actor_7_1B834F2F451C03B4A3F93BB2B909BC7B"), "Actor");
        assert_eq!(
            without_guid_tail("Actor_1B834F2F451C03B4A3F93BB2B909BC7B"),
            "Actor_1B834F2F451C03B4A3F93BB2B909BC7B"
        );
        assert_eq!(without_guid_tail("Speed"), "Speed");
        assert!(is_ref("/Game/A/B.B_C") && is_ref("/Script/Engine.X/.y") && is_ref("/Script/Engine.StaticMesh"));
        assert!(!is_ref("/Script.X") && !is_ref("/A/b\nc.d") && is_ref("/A/b.c\nd"));
        assert!(!is_ref("Enum::A"));
        assert_eq!(scs_node_name("/Game/A.A_C:SimpleConstructionScript_0.SCS_Node_12"), Some("SCS_Node_12"));
        assert_eq!(scs_node_name("SCS_Node_x"), None);
    }

    #[test]
    fn rounded_as_python() {
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(-2.5, 0), "-3");
        assert_eq!(to_fixed(4918.912345, 4), "4918.9123");
        assert_eq!(round4(0.03125), 0.0312);
        assert_eq!(round4(0.09375), 0.0938);
        assert_eq!(round4(-0.03125), -0.0312);
        assert_eq!(round4(1.23456), 1.2346);
    }

    #[test]
    fn unit_cube() {
        let mut pts = Vec::new();
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [0.0, 1.0] {
                    pts.push([x, y, z]);
                }
            }
        }
        let (vol, com, it) = hull_mass(&pts);
        assert!((vol - 1.0).abs() < 1e-12);
        assert!(com.iter().all(|c| (c - 0.5).abs() < 1e-12));
        assert!((it[0] - 1.0 / 6.0).abs() < 1e-12 && it[3].abs() < 1e-12);
    }
}
