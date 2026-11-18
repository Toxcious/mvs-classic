//! The fighter data's montages part: the character's montages, the notifies of
//! its sequences (seqNotifies), its BranchGroup assets, the AnimComposites its sub-mesh notifies play
//! (meshComposites); and one montage bound from the build, which the actors, skin and items parts bind theirs with.
//!
//! Properties of the build read as typed objects: an AnimMontage's own properties, its notify instances (inner
//! objects) over their class's defaults (a Blueprint CDO's `defaults`, merged over its Blueprint parents), the
//! AnimSequences of its slot tracks (Notifies, AdditiveAnimType), the montage's Skeleton's SlotGroups. What a property
//! leaves out is its class default, from the native constructors (UAnimMontage ctor 142849770). The set, the order
//! and the shaping are the port's: the snapshot numbers the data's objects by walk, so the order is the data's.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use mvs_data::fighter::Montage;
use mvs_data::value::from_value;
use mvs_data::{Props, Value};
use mvs_uasset::locres::LocresTable;

use super::Bind;
use super::anim_graph::{child_props, prop_name};
use super::bag::{arr_of, at, num, obj, s, str_of, tail, truthy};
use crate::fighter_port::FighterPort;
use crate::kind::{Cx, KindError};
use crate::kinds::gamedata::shape::{
    cdo_path, deep_update, defaults_or_properties, enums_for, fail, native, properties, shape_value, short_name,
};
use crate::objview::ObjView;

const ANIM_MONTAGE: &str = "/Script/Engine.AnimMontage";
const ANIM_SEQUENCE: &str = "/Script/Engine.AnimSequence";
const ANIM_COMPOSITE: &str = "/Script/Engine.AnimComposite";

// ---- native constants

// UAnimMontage ctor 142849770: BlendIn / BlendOut FAlphaBlend with BlendTime 0.25, bEnableAutoBlendOut true,
// BlendOutTriggerTime -1.
const MONTAGE_BLEND_TIME: f64 = 0.25;
const MONTAGE_AUTO_BLEND_OUT: bool = true;
const MONTAGE_BLEND_OUT_TRIGGER_TIME: f64 = -1.0;
// UAnimSequenceBase ctor 14286a9e0: RateScale 1.
const RATE_SCALE: f64 = 1.0;
// FAnimSegment Construct 142ed7fd0: AnimPlayRate 1, LoopingCount 1, the times 0.
const SEGMENT_PLAY_RATE: f64 = 1.0;
const SEGMENT_LOOPING_COUNT: f64 = 1.0;
// FAnimNotifyEvent's MontageTickType when a notify does not store it (the store's notifies all write it).
const NOTIFY_TICK: &str = "Queued";
// The native notify classes whose CDO sets bIsNativeBranchingPoint (UAnimNotify +0x28; ctors 142454c30 / 142454c60):
// FAnimNotifyEvent::IsBranchingPoint 1428a03d0 is true for them whatever MontageTickType the montage stores, so `tick`
// is BranchingPoint, the type the montage player acts on.
const NATIVE_BRANCHING: &[&str] = &["AnimNotify_PlayMontageNotify", "AnimNotify_PlayMontageNotifyWindow"];

// ---- port-made filters

// The notifies of a montage's slot-track sequences and of the sequences the anim graph plays: every one, the
// hit-react clips' AnimNotify_Hitframe states included (FullBody, Damage 10, IgnoreHitStun, on the HR_Bounce /
// HR_Wall clips and the floored ones: Shaggy_HR_Floored(_Back), Superman_Floored(_B / _F), Harley / Bugs / Steven
// HR_Floor_B / F, C017_HR_Ground_B / F, Creature_Floored_Pose, C020_HR_Bounce_FD), by which a bounced or floored
// fighter hits whoever is beside it.

/// The notify classes whose class defaults the data merges under an instance's properties (classes named *Notify*,
/// Ak_*, MovementStop*, OffGround*); another class's notify carries its own properties alone.
fn takes_class_defaults(cls: &str) -> bool {
    let n = cls.strip_suffix("_C").unwrap_or(cls);
    n.contains("Notify") || n.starts_with("Ak_") || n.starts_with("MovementStop") || n.starts_with("OffGround")
}

// The notify classes whose `Animation` is an AnimComposite played on a sub mesh (meshComposites).
const SUB_MESH_NOTIFIES: &[&str] = &["AnimNotify_SubMeshSpawn_C", "AnimNotify_SubMeshAnimChange_C"];

// ---- what a bind keeps

/// An object-keyed map member's key property: the class it names, and whether it is a soft reference.
#[derive(Clone)]
struct MapKey {
    cls: String,
    soft: bool,
}

/// What a montage's bind reads through and caches: a
/// fighter's bind, or another bind's (the items part's, key `items`). One per bind; its caches are its own: the
/// sequences' notifies by name (the first sequence of a name wins, as the data has it), the additive test by name,
/// the notify classes' defaults, the skeletons' slot groups, the game text and the classes' object-keyed maps.
pub struct MontageBind<'a> {
    pub cx: Cx<'a>,
    pub key: String,
    seq_notifies: RefCell<HashMap<String, Vec<Value>>>,
    additive: RefCell<HashMap<String, bool>>,
    class_defaults: RefCell<HashMap<String, Props>>,
    skeletons: RefCell<HashMap<String, Arc<HashMap<String, String>>>>,
    text: RefCell<Option<Arc<LocresTable>>>,
    map_keys: RefCell<HashMap<String, Arc<MapKeys>>>,
}

/// A class's map members with their key kinds.
type MapKeys = Vec<(String, MapKey)>;

impl<'a> MontageBind<'a> {
    pub fn new(cx: Cx<'a>, key: &str) -> MontageBind<'a> {
        MontageBind {
            cx,
            key: key.to_owned(),
            seq_notifies: RefCell::default(),
            additive: RefCell::default(),
            class_defaults: RefCell::default(),
            skeletons: RefCell::default(),
            text: RefCell::default(),
            map_keys: RefCell::default(),
        }
    }

    fn text(&self) -> Result<Arc<LocresTable>, KindError> {
        if let Some(t) = &*self.text.borrow() {
            return Ok(t.clone());
        }
        let t = crate::kinds::text::table(&self.cx, crate::kinds::text::GAME_CULTURE)?;
        *self.text.borrow_mut() = Some(t.clone());
        Ok(t)
    }
}

/// An object path's export name: `/Game/A/B.C` -> `C`.
pub fn export_name(path: &str) -> &str {
    &path[path.rfind('.').map_or(0, |i| i + 1)..]
}

/// A package path's object path (the asset named as its package).
pub fn asset_of(pkg: &str) -> String {
    format!("{pkg}.{}", &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..])
}

/// `typeof v === 'number' ? v : d`.
fn numd(v: Option<&Value>, d: f64) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(d)
}

// ---- shaping

const TEXT_KEYS: &[&str] = &["source", "namespace", "key", "table"];

/// A localized text in the form the data holds it, ahead of shape_value: `{Namespace, Key, SourceString,
/// LocalizedString}` (AnimNotify_PopText's Text: the HUD draws LocalizedString, the Game target's English string).
fn as_data(b: &MontageBind<'_>, v: &Value) -> Result<Value, KindError> {
    Ok(match v {
        Value::Arr(a) => Value::Arr(a.iter().map(|x| as_data(b, x)).collect::<Result<_, _>>()?),
        Value::Obj(o) => {
            let text = o.contains_key("source")
                && o.keys().all(|k| TEXT_KEYS.contains(&k.as_str()))
                && matches!(o.get("namespace"), Some(Value::Str(_)))
                && matches!(o.get("key"), Some(Value::Str(_)));
            if text {
                let ns = str_of(o.get("namespace")).unwrap_or_default();
                let key = str_of(o.get("key")).unwrap_or_default();
                let table = b.text()?;
                let shown = match table.get(ns).and_then(|n| n.get(key)) {
                    Some(t) => s(t.clone()),
                    None => match o.get("source") {
                        Some(Value::Null) | None => s(""),
                        Some(x) => x.clone(),
                    },
                };
                let src = o.get("source").cloned().unwrap_or(Value::Null);
                obj([("Namespace", s(ns)), ("Key", s(key)), ("SourceString", src), ("LocalizedString", shown)])
            } else {
                Value::Obj(o.iter().map(|(k, x)| Ok((k.clone(), as_data(b, x)?))).collect::<Result<_, KindError>>()?)
            }
        }
        _ => v.clone(),
    })
}

fn is_map(v: &[Value]) -> bool {
    !v.is_empty() && v.iter().all(|e| matches!(e, Value::Arr(p) if p.len() == 2))
}

/// `v` shaped (shape_value), the UserDefinedEnum assets its values name loaded by name first. `map_keys`: the key
/// property of each object-keyed map member of `v` (by member name).
fn shape(b: &MontageBind<'_>, v: &Value, map_keys: Option<&[(String, MapKey)]>) -> Result<Value, KindError> {
    let mut out = shape_value(&as_data(b, v)?, &enums_for(&b.cx, &b.key, &[v])?)?;
    // A map member's object keys in the form the data holds them: an object key as
    // `<Class>'MultiVersus/Content/<package path>.<object>'` (VFX_Skin's SkinData keys: the notify's reader takes the
    // object name off its end), a soft object key as its path (SubMeshSpawn's SkinMesdhOverride). The class is the one
    // the map property's KeyProp names, so a key the build lacks (Rick's C020_S03) keeps its form. A key whose
    // package the build lacks keeps its `/Game/` path in the quotes (C020B_S00).
    if let (Some(mk), Value::Obj(raw)) = (map_keys, v) {
        for (k, kp) in mk {
            let Some(Value::Arr(rawm)) = raw.get(k) else { continue };
            if !is_map(rawm) {
                continue;
            }
            let Some(Value::Arr(shaped)) = out.as_obj_mut_compat().and_then(|o| o.get_mut(k)) else { continue };
            for (i, e) in rawm.iter().enumerate() {
                let Some(Value::Str(mkey)) = e.as_arr().and_then(|p| p.first()) else { continue };
                let Some(rest) = mkey.strip_prefix("/Game/") else { continue };
                let key = if kp.soft {
                    mkey.clone()
                } else if b.cx.exists(mkey) {
                    format!("{}'MultiVersus/Content/{rest}'", kp.cls)
                } else {
                    format!("{}'{mkey}'", kp.cls)
                };
                if let Some(Value::Obj(se)) = shaped.get_mut(i) {
                    se.insert("Key".into(), s(key));
                }
            }
        }
    }
    Ok(out)
}

/// Mutable access to an object value's members.
trait ObjMut {
    fn as_obj_mut_compat(&mut self) -> Option<&mut Props>;
}

impl ObjMut for Value {
    fn as_obj_mut_compat(&mut self) -> Option<&mut Props> {
        match self {
            Value::Obj(o) => Some(o),
            _ => None,
        }
    }
}

/// A Blueprint class's object-keyed map members (its own and its Blueprint parents'), by name, from its
/// ChildProperties' MapProperty KeyProp.
fn map_keys_of(b: &MontageBind<'_>, cls: &str) -> Result<Arc<Vec<(String, MapKey)>>, KindError> {
    if let Some(m) = b.map_keys.borrow().get(cls) {
        return Ok(m.clone());
    }
    let mut out: Vec<(String, MapKey)> = Vec::new();
    let mut c = Some(cls.to_owned());
    while let Some(cur) = c.filter(|c| c.starts_with("/Game/")) {
        let o = b.cx.object(&cur)?;
        let nat = native(&o)?.unwrap_or_default();
        for p in child_props(&nat) {
            let name = prop_name(p);
            let kp = p.get("KeyProp");
            let Some(pc) = str_of(kp.and_then(|k| at(k, "PropertyClass"))) else { continue };
            if str_of(p.get("Type")) != Some("MapProperty") || out.iter().any(|(n, _)| n == name) {
                continue;
            }
            let kt = str_of(kp.and_then(|k| at(k, "Type")));
            if kt != Some("ObjectProperty") && kt != Some("SoftObjectProperty") {
                return fail(format!("{cls} {name}: a map keyed by {}", kt.unwrap_or("undefined")));
            }
            out.push((
                name.to_owned(),
                MapKey { cls: short_name(pc).to_owned(), soft: kt == Some("SoftObjectProperty") },
            ));
        }
        c = o.super_path()?;
    }
    let out = Arc::new(out);
    b.map_keys.borrow_mut().insert(cls.to_owned(), out.clone());
    Ok(out)
}

/// A notify class's defaults (its CDO's, over its Blueprint parents'), raw.
fn class_defaults_of(b: &MontageBind<'_>, cls: &str) -> Result<Props, KindError> {
    if let Some(d) = b.class_defaults.borrow().get(cls) {
        return Ok(d.clone());
    }
    let name = short_name(cls);
    // Unverified: the defaults are found by the class's package, named as the class without `_C`, case and all; a
    // class whose package is spelled otherwise (AnimNotify_traceMovement's AnimNotify_TraceMovement_C, Creature's
    // FastBall montages) has none in the data.
    let pkg = &cls[cls.rfind('/').map_or(0, |i| i + 1)..cls.rfind('.').unwrap_or(cls.len())];
    let d = if cls.starts_with("/Game/") && takes_class_defaults(name) && name == format!("{pkg}_C") {
        defaults_or_properties(&b.cx.object(&cdo_path(cls))?)?
    } else {
        Props::new()
    };
    b.class_defaults.borrow_mut().insert(cls.to_owned(), d.clone());
    Ok(d)
}

/// The inner object a notify reference names (`<pkg>.<export>:<chain>`), in `owner`.
fn inner_of<'a>(owner: &ObjView<'a>, r: &str) -> Result<Option<ObjView<'a>>, KindError> {
    let Some(at) = r.find(':') else { return Ok(None) };
    owner.inner_named(&r[at + 1..])
}

/// One FAnimNotifyEvent of a montage's or a sequence's `Notifies` as the data holds it; `owner` is the asset whose
/// inner objects the notify instances are.
fn notify_entry(b: &MontageBind<'_>, n: &Value, owner: &ObjView<'_>) -> Result<Props, KindError> {
    let r = if truthy(at(n, "NotifyStateClass")) { at(n, "NotifyStateClass") } else { at(n, "Notify") };
    let r = str_of(r).filter(|x| !x.is_empty());
    let inst_name = r.map(short_name);
    let inst = match r {
        Some(r) => inner_of(owner, r)?,
        None => None,
    };
    let cls = inst.as_ref().map(|i| short_name(i.class()).to_owned());
    let mut p = Props::new();
    let mut map_keys = None;
    if let Some(i) = &inst {
        p = class_defaults_of(b, i.class())?;
        deep_update(&mut p, &properties(i)?);
        p.shift_remove("UberGraphFrame");
        if p.values().any(|v| matches!(v, Value::Arr(a) if is_map(a))) {
            map_keys = Some(map_keys_of(b, i.class())?);
        }
    }
    let state = truthy(at(n, "NotifyStateClass"));
    let tick = if cls.as_deref().is_some_and(|c| NATIVE_BRANCHING.contains(&c)) {
        "BranchingPoint".to_owned()
    } else {
        let t = match at(n, "MontageTickType") {
            Some(Value::Str(t)) => t.clone(),
            _ => format!("EMontageNotifyTickType::{NOTIFY_TICK}"),
        };
        tail(&t).to_owned()
    };
    let mut e = Props::new();
    e.insert(
        "cls".into(),
        match &cls {
            Some(c) => s(c.clone()),
            None => match at(n, "NotifyName") {
                Some(x) if !x.is_null() => x.clone(),
                _ => Value::Null,
            },
        },
    );
    e.insert("inst".into(), inst_name.map_or(Value::Null, s));
    e.insert("t".into(), num(numd(at(n, "LinkValue"), 0.0) + numd(at(n, "TriggerTimeOffset"), 0.0)));
    // FAnimNotifyEvent::GetEndTriggerTime (4.26 source): a state ends at GetTriggerTime() + Duration +
    // EndTriggerTimeOffset, so the offset is folded into the duration.
    let end_off = if state { numd(at(n, "EndTriggerTimeOffset"), 0.0) } else { 0.0 };
    e.insert("dur".into(), num(numd(at(n, "Duration"), 0.0) + end_off));
    e.insert("state".into(), Value::Bool(state));
    e.insert("tick".into(), s(tick));
    e.insert("track".into(), num(numd(at(n, "TrackIndex"), 0.0)));
    e.insert("p".into(), shape(b, &Value::Obj(p), map_keys.as_deref().map(|m| m.as_slice()))?);
    Ok(e)
}

// ---- sequences

/// The emitted notifies of an AnimSequence, in sequence time; cached by its name, the first sequence of a name
/// standing for it.
fn seq_notifies_of(b: &MontageBind<'_>, name: &str, seq: Option<&ObjView<'_>>) -> Result<Vec<Value>, KindError> {
    if let Some(had) = b.seq_notifies.borrow().get(name) {
        return Ok(had.clone());
    }
    let mut out = Vec::new();
    // (a sequence the build does not hold has none)
    if let Some(seq) = seq {
        let p = properties(seq)?;
        if let Some(Value::Arr(list)) = p.get("Notifies") {
            for n in list {
                out.push(Value::Obj(notify_entry(b, n, seq)?));
            }
        }
    }
    b.seq_notifies.borrow_mut().insert(name.to_owned(), out.clone());
    Ok(out)
}

/// Whether a sequence's AdditiveAnimType is other than AAT_None (UAnimSequence::IsValidAdditive), by its name.
fn additive_of(b: &MontageBind<'_>, name: &str, seq: Option<&ObjView<'_>>) -> Result<bool, KindError> {
    if let Some(&a) = b.additive.borrow().get(name) {
        return Ok(a);
    }
    // Unverified: a slot track whose first segment is no AnimSequence (an AnimComposite: its own segments' sequences
    // decide) is taken as not additive.
    let t = match seq {
        Some(q) if q.class() == ANIM_SEQUENCE => properties(q)?.get("AdditiveAnimType").cloned(),
        _ => None,
    };
    let a = matches!(&t, Some(Value::Str(t)) if tail(t) != "AAT_None");
    b.additive.borrow_mut().insert(name.to_owned(), a);
    Ok(a)
}

// ---- one montage

/// An FAnimSegment as the data holds it; `anim_class` is the class of the asset its AnimReference names, `None` when
/// the build has no such package.
fn segment_entry(sg: &Value, anim_class: Option<&str>) -> Props {
    let mut o = Props::new();
    o.insert("anim".into(), s(export_name(str_of(at(sg, "AnimReference")).unwrap_or_default())));
    // Unverified: a reference to a package the build does not hold (Arya's Ninja_Air_EvadeN, Rick's C020B_* sequences)
    // is written an AnimSequence, as the data has it from the montage's import (the store keeps no import's class);
    // the build's runtime would load it as null.
    o.insert("animType".into(), s(anim_class.map_or("AnimSequence", short_name)));
    o.insert("start".into(), num(numd(at(sg, "StartPos"), 0.0)));
    o.insert("a0".into(), num(numd(at(sg, "AnimStartTime"), 0.0)));
    o.insert("a1".into(), num(numd(at(sg, "AnimEndTime"), 0.0)));
    o.insert("rate".into(), num(numd(at(sg, "AnimPlayRate"), SEGMENT_PLAY_RATE)));
    o.insert("loops".into(), num(numd(at(sg, "LoopingCount"), SEGMENT_LOOPING_COUNT)));
    o
}

/// A montage's slot group (UAnimMontage::GetGroupName: its skeleton's group of its first slot track; a slot the
/// skeleton does not list is in DefaultGroup).
fn slot_group_of(b: &MontageBind<'_>, skeleton: &str, slot: &str) -> Result<String, KindError> {
    let had = b.skeletons.borrow().get(skeleton).cloned();
    let g = match had {
        Some(g) => g,
        None => {
            let p = properties(&b.cx.object(skeleton)?)?;
            let mut m = HashMap::new();
            for grp in arr_of(p.get("SlotGroups")) {
                for sn in arr_of(at(grp, "SlotNames")) {
                    m.insert(
                        str_of(Some(sn)).unwrap_or_default().to_owned(),
                        str_of(at(grp, "GroupName")).unwrap_or_default().to_owned(),
                    );
                }
            }
            let g = Arc::new(m);
            b.skeletons.borrow_mut().insert(skeleton.to_owned(), g.clone());
            g
        }
    };
    Ok(g.get(slot).cloned().unwrap_or_else(|| "DefaultGroup".to_owned()))
}

fn f(o: &Props, k: &str) -> f64 {
    o.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// One AnimMontage of the build as the fighter data holds it, as the bag (`bind_montage`).
pub fn montage_value(b: &MontageBind<'_>, path: &str) -> Result<Props, KindError> {
    let m = b.cx.object(path)?;
    if m.class() != ANIM_MONTAGE {
        return fail(format!("{path}: not an AnimMontage ({})", m.class()));
    }
    let p = properties(&m)?;
    let name = export_name(m.path()).to_owned();
    // FAlphaBlend's option and curve: the game has the blend-out's option alone.
    for side in ["BlendIn", "BlendOut"] {
        let bl = p.get(side).filter(|v| v.as_obj().is_some());
        let curve = truthy(bl.and_then(|v| at(v, "CustomCurve")));
        let option = match bl.and_then(|v| at(v, "BlendOption")) {
            Some(Value::Null) | None => "EAlphaBlendOption::Linear".to_owned(),
            Some(Value::Str(o)) => o.clone(),
            Some(_) => String::new(),
        };
        if curve || (side == "BlendIn" && option != "EAlphaBlendOption::Linear") {
            return fail(format!("{name}: {side} with a blend option or a custom curve is not ported"));
        }
    }
    let out_option = match p.get("BlendOut").and_then(|v| at(v, "BlendOption")) {
        Some(Value::Str(o)) => tail(o).to_owned(),
        _ => "Linear".to_owned(),
    };
    // A BlendIn / BlendOut absent, or serialised without its BlendTime, keeps the class default's 0.25: a tagged
    // struct is read over the archetype's value (Bugs' BugsBunny_Nav_Air_EvadeD_Montage and
    // BugsBunny_Nav_EvadeU_Montage, BlendOut {BlendOption CircularOut} alone). Unverified: the tagged-property rule is
    // 4.26's, not read in the exe.
    let blend = |side: &str| num(numd(p.get(side).and_then(|v| at(v, "BlendTime")), MONTAGE_BLEND_TIME));
    let mut out = Props::new();
    out.insert("name".into(), s(name.clone()));
    out.insert(
        "length".into(),
        p.get("SequenceLength").filter(|v| v.as_f64().is_some()).cloned().unwrap_or(Value::Null),
    );
    out.insert("blendIn".into(), blend("BlendIn"));
    out.insert("blendOut".into(), blend("BlendOut"));
    // the automatic blend-out's inputs (FAnimMontageInstance::Advance 14284b1d0)
    out.insert(
        "autoBlendOut".into(),
        Value::Bool(p.get("bEnableAutoBlendOut").and_then(Value::as_bool).unwrap_or(MONTAGE_AUTO_BLEND_OUT)),
    );
    out.insert("blendOutTriggerTime".into(), num(numd(p.get("BlendOutTriggerTime"), MONTAGE_BLEND_OUT_TRIGGER_TIME)));
    if out_option != "Linear" {
        out.insert("blendOutOption".into(), s(out_option));
    }
    out.insert("rateScale".into(), num(numd(p.get("RateScale"), RATE_SCALE)));
    let sections = arr_of(p.get("CompositeSections"))
        .iter()
        .map(|sc| {
            obj([
                ("name", at(sc, "SectionName").cloned().unwrap_or(Value::Null)),
                ("next", at(sc, "NextSectionName").cloned().unwrap_or(Value::Null)),
                ("t", num(numd(at(sc, "LinkValue"), 0.0))),
            ])
        })
        .collect();
    out.insert("sections".into(), Value::Arr(sections));
    // the sequences the slot tracks name
    let tracks = arr_of(p.get("SlotAnimTracks"));
    let segs_of = |tr: &Value| arr_of(at(tr, "AnimTrack").and_then(|t| at(t, "AnimSegments"))).to_vec();
    let mut anims: HashMap<String, Option<ObjView<'_>>> = HashMap::new();
    for tr in tracks {
        for sg in segs_of(tr) {
            if let Some(Value::Str(r)) = at(&sg, "AnimReference")
                && !anims.contains_key(r)
            {
                let o = if b.cx.exists(r) { Some(b.cx.object(r)?) } else { None };
                anims.insert(r.clone(), o);
            }
        }
    }
    // a segment's anim name -> its asset (the first reference of the name)
    let mut seg_assets: HashMap<String, Option<ObjView<'_>>> = HashMap::new();
    let mut slots: Vec<Props> = Vec::new();
    for tr in tracks {
        let mut segs = Vec::new();
        let mut empty = 0;
        for sg in segs_of(tr) {
            // A segment whose AnimReference the cooked asset has as null (Creature_RunStart_Montage's only segment):
            // no sequence to sample, so it is left out and counted in the slot's emptySegments.
            let Some(Value::Str(r)) = at(&sg, "AnimReference") else {
                empty += 1;
                continue;
            };
            let a = anims.get(r).cloned().flatten();
            let an = export_name(r).to_owned();
            seg_assets.entry(an).or_insert_with(|| a.clone());
            segs.push(segment_entry(&sg, a.as_ref().map(|o| o.class())));
        }
        let mut slot = Props::new();
        slot.insert("slot".into(), at(tr, "SlotName").cloned().unwrap_or(Value::Null));
        // FAnimTrack's additive test (GetSlotWeight 142856c40): the first segment's sequence alone (Unverified:
        // FUN_1428269e0's body is taken as the stock one). Stated only where true.
        let additive = match segs.first() {
            Some(s0) => {
                let an = str_of(s0.get("anim")).unwrap_or_default().to_owned();
                let asset = seg_assets.get(&an).cloned().flatten();
                additive_of(b, &an, asset.as_ref())?
            }
            None => false,
        };
        slot.insert("segments".into(), Value::Arr(segs.into_iter().map(Value::Obj).collect()));
        if empty > 0 {
            slot.insert("emptySegments".into(), num(empty as f64));
        }
        if additive {
            slot.insert("additive".into(), Value::Bool(true));
        }
        slots.push(slot);
    }
    let mut notifies: Vec<Props> = Vec::new();
    for n in arr_of(p.get("Notifies")) {
        notifies.push(notify_entry(b, n, &m)?);
    }
    // The slot tracks' sequence notifies in montage time (queued: FAnimTrack::GetAnimNotifiesFromTrackPositions),
    // clipped to the segment; `seq` names the sequence and `slot` the track. One state split by a segment boundary is
    // one window (HandleEvents 142857270, FAnimTrack::GetAnimNotifiesFromTrackPositions 142820650): where a segment's
    // window runs to its track end and the next segment, of the same sequence, starts at that track position with the
    // window from its own start, the state stays active and ends at GetEndTriggerTime mapped through the later
    // segment. A window that stops short of the boundary leaves a step no segment lists: two entries, a real end and
    // begin.
    for tr in &slots {
        let mut held: HashMap<usize, (usize, bool)> = HashMap::new();
        let mut prev_end: Option<f64> = None;
        let mut prev_anim: Option<String> = None;
        for sg in arr_of(tr.get("segments")) {
            let sg = sg.as_obj().cloned().unwrap_or_default();
            let anim = str_of(sg.get("anim")).unwrap_or_default().to_owned();
            let (start, a0, a1, rate, loops) =
                (f(&sg, "start"), f(&sg, "a0"), f(&sg, "a1"), f(&sg, "rate"), f(&sg, "loops"));
            let joined =
                prev_anim.as_deref() == Some(anim.as_str()) && prev_end.is_some_and(|e| (e - start).abs() < 1e-6);
            prev_end = if rate > 0.0 { Some(start + (a1 - a0) / rate) } else { None };
            prev_anim = Some(anim.clone());
            if !joined {
                held = HashMap::new();
            }
            if str_of(sg.get("animType")) != Some("AnimSequence") {
                held = HashMap::new();
                continue;
            }
            let mut seen: HashMap<usize, (usize, bool)> = HashMap::new();
            let asset = seg_assets.get(&anim).cloned().flatten();
            let list = seq_notifies_of(b, &anim, asset.as_ref())?;
            for (i, x) in list.iter().enumerate() {
                let x = x.as_obj().cloned().unwrap_or_default();
                let (xt, xdur) = (f(&x, "t"), f(&x, "dur"));
                let a = js_max(xt, a0);
                let b_end = js_min(xt + xdur, a1);
                if b_end < a || loops != 1.0 || rate <= 0.0 {
                    continue;
                }
                let e = match held.get(&i) {
                    Some(&(e, true)) if a == a0 => {
                        let et = f(&notifies[e], "t");
                        notifies[e].insert("dur".into(), num(start + (b_end - a0) / rate - et));
                        e
                    }
                    _ => {
                        let t = start + (a - a0) / rate;
                        let inst = match x.get("inst") {
                            Some(Value::Str(i)) => format!("{anim}:{i}"),
                            _ => format!("{anim}:null"),
                        };
                        let mut ne = Props::new();
                        ne.insert("cls".into(), x.get("cls").cloned().unwrap_or(Value::Null));
                        ne.insert("inst".into(), s(inst));
                        ne.insert("t".into(), num(t));
                        ne.insert("dur".into(), num((b_end - a) / rate));
                        ne.insert("state".into(), x.get("state").cloned().unwrap_or(Value::Null));
                        ne.insert("tick".into(), s("Queued"));
                        ne.insert("track".into(), x.get("track").cloned().unwrap_or(Value::Null));
                        ne.insert("p".into(), x.get("p").cloned().unwrap_or(Value::Null));
                        ne.insert("seq".into(), s(anim.clone()));
                        ne.insert("slot".into(), tr.get("slot").cloned().unwrap_or(Value::Null));
                        notifies.push(ne);
                        notifies.len() - 1
                    }
                };
                seen.insert(i, (e, b_end == a1));
            }
            held = seen;
        }
    }
    notifies.sort_by(|x, y| f(x, "t").partial_cmp(&f(y, "t")).unwrap_or(std::cmp::Ordering::Equal));
    out.insert("slots".into(), Value::Arr(slots.iter().cloned().map(Value::Obj).collect()));
    out.insert("notifies".into(), Value::Arr(notifies.into_iter().map(Value::Obj).collect()));
    // the slot group (written only when not DefaultGroup: Montage_Play stops the instances of the new montage's own
    // group alone, Montage_PlayInternal 142840b00)
    // Unverified: a montage whose Skeleton the build does not hold (Arya's Ninja_* montages' Ninja_Skeleton) is taken
    // as in DefaultGroup (the data took the character's skeleton, whose one group is DefaultGroup).
    if let (Some(first), Some(Value::Str(skel))) = (slots.first(), p.get("Skeleton"))
        && b.cx.exists(skel)
    {
        let slot = str_of(first.get("slot")).unwrap_or("None");
        let g = slot_group_of(b, skel, slot)?;
        if g != "DefaultGroup" {
            out.insert("group".into(), s(g));
        }
    }
    Ok(out)
}

/// JS Math.max of two numbers (NaN if either is).
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// One AnimMontage of the build as the fighter data holds it (FighterData.montages' entries): its blend, sections,
/// slot tracks with their segments, its notifies with its slot tracks' sequence notifies in montage time, sorted by
/// time, and its slot group when not DefaultGroup. `path` is the montage's object path (`/Game/A/M.M`).
pub fn bind_montage(b: &MontageBind<'_>, path: &str) -> Result<Montage, KindError> {
    from_value(Value::Obj(montage_value(b, path)?)).map_err(|e| KindError::Failed(format!("{path}: {e}")))
}

// ---- the sets

/// Whether `pkg` (a package path under the character's folder) is under its anim_skip.
pub(super) fn skipped(port: &FighterPort, pkg: &str) -> bool {
    let Some(skip) = port.anim_skip else { return false };
    let rel = pkg.get(port.folder.len() + 1..).unwrap_or_default();
    skip.iter().any(|s| rel == *s || (s.ends_with('/') && rel.starts_with(s)))
}

/// A package's place in the data, whose sets are sorted as Windows file paths: the folder its glob named, then each
/// found part after a `\`, and `.json`, compared by code unit (`+` < `-` < `.` < `A`-`Z` < `\` < `_` < `a`-`z`).
fn file_order(base: &str, pkg: &str) -> String {
    format!("{base}{}.json", pkg.get(base.len()..).unwrap_or_default().replace('/', "\\"))
}

fn sort_by_key(mut found: Vec<(String, String)>) -> Vec<String> {
    found.sort_by(|a, b| js_cmp(&a.0, &b.0));
    found.into_iter().map(|x| x.1).collect()
}

/// JS string comparison (by UTF-16 code unit).
pub(super) fn js_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// The packages of class `cls` under the character's anim folders (anim_dirs, default Animations: each with its
/// subfolders, '.' the folder's own alone), minus anim_skip, in the data's order (file_order).
fn anim_packages(cx: &Cx<'_>, port: &FighterPort, cls: &str) -> Vec<String> {
    let mut found = Vec::new();
    for d in port.anim_dirs.unwrap_or(&["Animations"]) {
        let base = if *d == "." { port.folder.to_owned() } else { format!("{}/{d}", port.folder) };
        for p in cx.catalog.list_packages(&base, *d != ".", Some(cls)) {
            if !skipped(port, &p) {
                found.push((file_order(&base, &p), p));
            }
        }
    }
    sort_by_key(found)
}

/// The character's montage packages: the AnimMontages of its anim folders named *Montage* (any name with
/// montages_by_type), in the data's order.
pub fn montage_packages(cx: &Cx<'_>, port: &FighterPort) -> Vec<String> {
    let l = anim_packages(cx, port, ANIM_MONTAGE);
    if port.montages_by_type {
        return l;
    }
    l.into_iter().filter(|p| p[p.rfind('/').map_or(0, |i| i + 1)..].to_lowercase().contains("montage")).collect()
}

/// The character's sequence packages: the AnimSequences of its anim folders not named *Montage*, in the data's order
/// (seqNotifies' sequences).
pub fn sequence_packages(cx: &Cx<'_>, port: &FighterPort) -> Vec<String> {
    anim_packages(cx, port, ANIM_SEQUENCE)
        .into_iter()
        .filter(|p| !p[p.rfind('/').map_or(0, |i| i + 1)..].contains("Montage"))
        .collect()
}

// ---- the part

/// The montages part: writes seqNotifies, branchGroups, montages and meshComposites.
pub fn bind_montages(bd: &mut Bind<'_, '_>, mb: &MontageBind<'_>) -> Result<(), KindError> {
    let cx = bd.cx;
    let port = bd.port;
    // seqNotifies: every AnimSequence of the anim folders (packages not named *Montage*) with an emitted notify, keyed
    // by the sequence's name.
    let mut seqs = Props::new();
    for p in sequence_packages(cx, port) {
        let o = cx.object(&asset_of(&p))?;
        let name = export_name(o.path()).to_owned();
        let list = seq_notifies_of(mb, &name, Some(&o))?;
        if !list.is_empty() {
            seqs.insert(name, Value::Arr(list));
        }
    }
    bd.out.insert("seqNotifies".into(), Value::Obj(seqs));

    // branchGroups: the BranchGroup assets whose package names start with the port table's prefixes, by name.
    let mut found = Vec::new();
    for pre in port.branch_groups.unwrap_or_default() {
        let cut = pre.rfind('/').unwrap_or(0);
        let dir = &pre[..cut];
        let start = pre[cut + 1..].to_lowercase();
        for p in cx.catalog.list_packages(dir, false, None) {
            if p[p.rfind('/').map_or(0, |i| i + 1)..].to_lowercase().starts_with(&start) {
                found.push((file_order(dir, &p), p));
            }
        }
    }
    let mut groups = Props::new();
    for p in sort_by_key(found) {
        let o = cx.object(&asset_of(&p))?;
        if short_name(o.class()) != "BranchGroup" {
            continue;
        }
        groups.insert(export_name(o.path()).to_owned(), shape(mb, &Value::Obj(properties(&o)?), None)?);
    }
    bd.out.insert("branchGroups".into(), Value::Obj(groups));

    // montages
    let mut mons = Props::new();
    for p in montage_packages(cx, port) {
        let m = montage_value(mb, &asset_of(&p))?;
        let name = str_of(m.get("name")).unwrap_or_default().to_owned();
        mons.insert(name, Value::Obj(m));
    }

    // meshComposites: the AnimComposites a sub-mesh notify plays (Tom_and_Jerry_PaddleBall_Montage's
    // PaddleBall_*Attack_Composite), by name, found anywhere under the character's folder.
    let mut comps = Props::new();
    let mut comp_pkgs: Option<Vec<String>> = None;
    for m in mons.values() {
        for n in arr_of(at(m, "notifies")) {
            let sub = str_of(at(n, "cls")).is_some_and(|c| SUB_MESH_NOTIFIES.contains(&c));
            let Some(a) = sub.then(|| str_of(at(n, "p").and_then(|p| at(p, "Animation")))).flatten() else { continue };
            if comps.contains_key(a) || mons.contains_key(a) {
                continue;
            }
            let pkgs = comp_pkgs.get_or_insert_with(|| {
                sort_by_key(
                    cx.catalog
                        .list_packages(port.folder, true, Some(ANIM_COMPOSITE))
                        .into_iter()
                        .map(|p| (file_order(port.folder, &p), p))
                        .collect(),
                )
            });
            let Some(pkg) =
                pkgs.iter().find(|p| p[p.rfind('/').map_or(0, |i| i + 1)..].to_lowercase() == a.to_lowercase())
            else {
                continue;
            };
            let cp = properties(&cx.object(&asset_of(pkg))?)?;
            let mut segments = Vec::new();
            for sg in arr_of(cp.get("AnimationTrack").and_then(|t| at(t, "AnimSegments"))) {
                let Some(Value::Str(r)) = at(sg, "AnimReference") else { continue };
                let cls = if cx.exists(r) { Some(cx.object(r)?.class().to_owned()) } else { None };
                segments.push(Value::Obj(segment_entry(sg, cls.as_deref())));
            }
            comps.insert(
                a.to_owned(),
                obj([
                    ("length", num(numd(cp.get("SequenceLength"), 0.0))),
                    ("rateScale", num(numd(cp.get("RateScale"), RATE_SCALE))),
                    ("segments", Value::Arr(segments)),
                ]),
            );
        }
    }
    bd.out.insert("montages".into(), Value::Obj(mons));
    if !comps.is_empty() {
        bd.out.insert("meshComposites".into(), Value::Obj(comps));
    }
    Ok(())
}
