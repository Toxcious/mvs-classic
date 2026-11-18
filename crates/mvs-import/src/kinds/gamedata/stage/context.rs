//! The context a stage's parts bind from: its levels loaded as objects (`crate::stage_levels`), their actors and
//! components in export order, and the helpers the parts share (the `Props` bag helpers).

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use mvs_data::{Props, Value};

use super::super::profiles::{CollisionTable, collision_table};
use super::super::shape::{fail, properties, short_name};
use super::super::transform::{Quat, Xf, compose, quat_rotator, rotator_quat};
use crate::game::MAPS_ROOT;
use crate::kind::{Cx, KindError};
use crate::objview::ObjView;
use crate::stage_levels::{StageLevel, StageLevels, stage_levels};

pub type Vec3 = [f64; 3];

// ---- the bag's values as the binds read them

/// A struct's X Y Z, or `d`.
pub fn vec(v: Option<&Value>, d: Vec3) -> Vec3 {
    match v {
        Some(Value::Obj(s)) => [num_of(s.get("X")), num_of(s.get("Y")), num_of(s.get("Z"))],
        _ => d,
    }
}

/// A struct's X Y Z, zeros where it has none.
pub fn vec0(v: Option<&Value>) -> Vec3 {
    vec(v, [0.0; 3])
}

/// A rotator's pitch, yaw, roll (degrees), or zeros.
pub fn rot(v: Option<&Value>) -> Vec3 {
    match v {
        Some(Value::Obj(s)) => [num_of(s.get("Pitch")), num_of(s.get("Yaw")), num_of(s.get("Roll"))],
        _ => [0.0; 3],
    }
}

/// A member read as `v as number`: a missing or non-number member is NaN (JS `undefined` in arithmetic).
fn num_of(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// A member as a number, when it is one.
pub fn f(p: &Props, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64)
}

/// A member as a string, when it is one.
pub fn s<'p>(p: &'p Props, k: &str) -> Option<&'p str> {
    p.get(k).and_then(Value::as_str)
}

/// A member as a struct, when it is one.
pub fn st<'p>(p: &'p Props, k: &str) -> Option<&'p Props> {
    p.get(k).and_then(Value::as_obj)
}

/// A member as an array, when it is one.
pub fn ar<'p>(p: &'p Props, k: &str) -> Option<&'p [Value]> {
    p.get(k).and_then(Value::as_arr)
}

/// JavaScript's truth value of a member (an empty array or object is true).
pub fn js_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => !(*n == 0.0 || n.is_nan()),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// Python's truth value (`truthy`: an empty struct or array is false).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Arr(a)) => !a.is_empty(),
        Some(Value::Obj(o)) => !o.is_empty(),
        v => js_truthy(v),
    }
}

/// A member's string array (a bag list of strings), `None` where it is no such list.
pub fn strings(v: Option<&Value>) -> Option<Vec<String>> {
    v.and_then(Value::as_arr)?.iter().map(|x| x.as_str().map(str::to_owned)).collect()
}

/// `[a, b, c]` of a vector as the data holds it.
pub fn v3(v: Vec3) -> Vec<f64> {
    v.to_vec()
}

/// An object's name: the last component of its path (`nameOf`).
pub fn name_of(o: &ObjView<'_>) -> String {
    short_name(o.path()).to_owned()
}

// ---- the context

/// A stage's levels with the indexes its parts read through.
pub struct StageCtx<'a> {
    pub cx: Cx<'a>,
    pub key: String,
    pub levels: StageLevels<'a>,
    /// Every object of the levels (the audio level's too) by path.
    by_path: HashMap<String, ObjView<'a>>,
    /// An object's direct inner objects by its path, in export order.
    kids: HashMap<String, Vec<ObjView<'a>>>,
    /// An object path's level: an index into `levels.levels`, `usize::MAX` for the audio level.
    level_of: HashMap<String, usize>,
    collisions: OnceCell<Arc<CollisionTable>>,
    props: RefCell<HashMap<String, Rc<Props>>>,
    templated: RefCell<HashMap<String, Rc<Props>>>,
    worlds: RefCell<HashMap<String, Xf>>,
    chains: RefCell<HashMap<String, Rc<Vec<String>>>>,
}

/// The parent of an inner object's path: the path up to its last `.` after the `:`.
fn parent_path(p: &str) -> Option<&str> {
    let colon = p.find(':')?;
    p.rfind('.').filter(|&d| d > colon).map(|d| &p[..d])
}

impl<'a> StageCtx<'a> {
    /// The context of stage `key` bound from its manifest's `levels`.
    pub fn new(cx: &Cx<'a>, key: &str, levels: &[(String, String)]) -> Result<StageCtx<'a>, KindError> {
        let lv = stage_levels(cx, key, levels)?;
        let mut by_path = HashMap::new();
        let mut kids: HashMap<String, Vec<ObjView<'a>>> = HashMap::new();
        let mut level_of = HashMap::new();
        for (li, l) in lv.levels.iter().map(|l| (l, false)).chain(lv.audio.iter().map(|l| (l, true))).enumerate() {
            let (l, is_audio) = l;
            let at = if is_audio { usize::MAX } else { li };
            for o in &l.objects {
                by_path.insert(o.path().to_owned(), o.clone());
                level_of.insert(o.path().to_owned(), at);
                if let Some(p) = parent_path(o.path()) {
                    kids.entry(p.to_owned()).or_default().push(o.clone());
                }
            }
        }
        Ok(StageCtx {
            cx: *cx,
            key: key.to_owned(),
            levels: lv,
            by_path,
            kids,
            level_of,
            collisions: OnceCell::new(),
            props: RefCell::default(),
            templated: RefCell::default(),
            worlds: RefCell::default(),
            chains: RefCell::default(),
        })
    }

    /// The build's collision profile table, read on first use.
    pub fn collisions(&self) -> Result<&CollisionTable, KindError> {
        if self.collisions.get().is_none() {
            let t = collision_table(&self.cx)?;
            let _ = self.collisions.set(t);
        }
        Ok(self.collisions.get().expect("set above"))
    }

    /// The actors of the gameplay and lighting levels, in level then export order (`actors`).
    pub fn actors(&self) -> Vec<ObjView<'a>> {
        self.levels.actors(false).cloned().collect()
    }

    /// The actors of the scenery and vfx levels (`sceneryActors`).
    pub fn scenery_actors(&self) -> Vec<ObjView<'a>> {
        self.levels.actors(true).cloned().collect()
    }

    /// The gameplay and lighting levels' loaded levels (`levels`).
    pub fn level(&self, li: usize) -> &StageLevel<'a> {
        &self.levels.levels[li]
    }

    /// The level (its `li`) an object of any level is in; -1 for the audio level (`levelOf(o).li`).
    pub fn li_of(&self, o: &ObjView<'_>) -> Result<i32, KindError> {
        match self.level_of.get(o.path()) {
            Some(&usize::MAX) => Ok(-1),
            Some(&i) => Ok(self.levels.levels[i].li),
            None => fail(format!("stage data {}: {} is in none of its levels", self.key, o.path())),
        }
    }

    /// An object of the build (`ctx.load`).
    pub fn load(&self, path: &str) -> Result<ObjView<'a>, KindError> {
        self.cx.object(path)
    }

    /// An object of the levels by path, else the build's.
    fn find(&self, path: &str) -> Result<ObjView<'a>, KindError> {
        match self.by_path.get(path) {
            Some(o) => Ok(o.clone()),
            None => self.load(path),
        }
    }

    /// An object's own properties as the bag (`o.properties`), once.
    pub fn properties(&self, o: &ObjView<'_>) -> Result<Rc<Props>, KindError> {
        if let Some(p) = self.props.borrow().get(o.path()) {
            return Ok(p.clone());
        }
        let p = Rc::new(properties(o)?);
        self.props.borrow_mut().insert(o.path().to_owned(), p.clone());
        Ok(p)
    }

    /// An actor's components (its direct inner objects, in export order); of one class's short name when given.
    pub fn comps(&self, actor: &ObjView<'a>, class: Option<&str>) -> Vec<ObjView<'a>> {
        let l = self.kids.get(actor.path()).map_or(&[][..], Vec::as_slice);
        l.iter().filter(|c| class.is_none_or(|k| short_name(c.class()) == k)).cloned().collect()
    }

    /// An actor's root component (its RootComponent), or none.
    pub fn root(&self, actor: &ObjView<'a>) -> Result<Option<ObjView<'a>>, KindError> {
        let p = self.properties(actor)?;
        let Some(rc) = s(&p, "RootComponent") else { return Ok(None) };
        Ok(self.comps(actor, None).into_iter().find(|c| short_name(c.path()) == short_name(rc)))
    }

    /// An object's properties over its archetypes' (its `template` chain up to a /Script/ object): a struct is
    /// written as its members that differ from the archetype's.
    pub fn with_templates(&self, o: &ObjView<'a>) -> Result<Rc<Props>, KindError> {
        if let Some(p) = self.templated.borrow().get(o.path()) {
            return Ok(p.clone());
        }
        let mut props: Props = (*self.properties(o)?).clone();
        let mut chain = o.clone();
        while let Some(tpl) = chain.template()? {
            if tpl.starts_with("/Script/") {
                break;
            }
            chain = self.find(&tpl)?;
            for (k, v) in self.properties(&chain)?.iter() {
                match (v, props.get(k)) {
                    (Value::Obj(tv), Some(Value::Obj(own))) => {
                        let mut merged = tv.clone();
                        for (ok, ov) in own {
                            merged.insert(ok.clone(), ov.clone());
                        }
                        props.insert(k.clone(), Value::Obj(merged));
                    }
                    (_, None) => {
                        props.insert(k.clone(), v.clone());
                    }
                    _ => {}
                }
            }
        }
        let p = Rc::new(props);
        self.templated.borrow_mut().insert(o.path().to_owned(), p.clone());
        Ok(p)
    }

    /// A component's world transform: its relative transform over its templates', under its AttachParent chain.
    pub fn world(&self, c: &ObjView<'a>) -> Result<Xf, KindError> {
        if let Some(x) = self.worlds.borrow().get(c.path()) {
            return Ok(*x);
        }
        let t = self.with_templates(c)?;
        let [p, y, r] = rot(t.get("RelativeRotation"));
        let rel = Xf {
            loc: vec0(t.get("RelativeLocation")),
            q: rotator_quat(p, y, r),
            scale: vec(t.get("RelativeScale3D"), [1.0; 3]),
        };
        for flag in ["bAbsoluteLocation", "bAbsoluteRotation", "bAbsoluteScale"] {
            if js_truthy(t.get(flag)) {
                return fail(format!("stage data {}: {} has {flag}", self.key, c.path()));
            }
        }
        let own = self.properties(c)?;
        let xf = match s(&own, "AttachParent") {
            Some(par) => compose(&self.world(&self.find(par)?)?, &rel)?,
            None => rel,
        };
        self.worlds.borrow_mut().insert(c.path().to_owned(), xf);
        Ok(xf)
    }

    /// An actor's world transform as (location, rotation in degrees, scale): its root's relative transform, or where
    /// the root is attached, its world transform by the attach chain.
    pub fn root_xf(&self, actor: &ObjView<'a>) -> Result<(Vec3, Vec3, Vec3), KindError> {
        let rc = self.root(actor)?;
        let rp = match &rc {
            Some(rc) => self.properties(rc)?,
            None => Rc::new(Props::new()),
        };
        if let Some(rc) = &rc
            && js_truthy(rp.get("AttachParent"))
        {
            let w = self.world(rc)?;
            return Ok((w.loc, quat_rotator(w.q), w.scale));
        }
        Ok((
            vec0(rp.get("RelativeLocation")),
            rot(rp.get("RelativeRotation")),
            vec(rp.get("RelativeScale3D"), [1.0; 3]),
        ))
    }

    /// An actor's class and its Blueprint parents by name, the class first.
    pub fn class_chain(&self, a: &ObjView<'a>) -> Result<Rc<Vec<String>>, KindError> {
        if let Some(c) = self.chains.borrow().get(a.class()) {
            return Ok(c.clone());
        }
        let mut out = vec![short_name(a.class()).to_owned()];
        let mut cls = a.class().to_owned();
        while !cls.starts_with("/Script/") {
            let Some(sup) = self.load(&cls)?.super_path()? else { break };
            if sup.starts_with("/Script/") {
                break;
            }
            out.push(short_name(&sup).to_owned());
            cls = sup;
        }
        let out = Rc::new(out);
        self.chains.borrow_mut().insert(a.class().to_owned(), out.clone());
        Ok(out)
    }
}

/// A quaternion's identity within a tolerance.
pub fn identity(q: Quat) -> bool {
    super::super::transform::identity_rot(q, 1e-7)
}

/// The level path below the maps root for the data's `name`: `M011/Map_M011` -> `Map_M011`.
pub fn level_name(level: &str) -> &str {
    level.rsplit('/').next().unwrap_or(level)
}

/// The level's object path (`<maps root><level>`).
pub fn level_path(level: &str) -> String {
    format!("{MAPS_ROOT}{level}")
}
