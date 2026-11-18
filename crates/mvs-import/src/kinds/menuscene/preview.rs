//! What the menu scenes read of a preview actor's skin (BP_CharacterPreview): a montage's segments and sections, its
//! notifies, and the skin's CharacterLighting rig as spot lights.

use indexmap::IndexMap;
use mvs_material::menuscene::{MirrorWhen, MontageNotify, MontageSection, MontageSegment, SceneMontage, SpotLight};
use mvs_math::{js, libm};

use super::tables::RIG_MIRROR;
use super::util::{
    AObj, Props, Scope, arr, asset_name, class_of, content_pkg, fail, inner_at, inner_list, num, num_or, obj_of,
    over_props, short_of, str_of, truthy,
};
use crate::json::{Json, to_vec};
use crate::kind::KindError;

/// What a montage is read for.
#[derive(Clone, Copy, Debug, Default)]
pub struct MontageOpts {
    /// Blend times, slot and notifies (the pre-match's PreviewIdle).
    pub with_blend: bool,
    /// A podium montage: may blend in (its blend time kept).
    pub podium: bool,
}

/// A BlendIn / BlendOut's time: the class default 0.25 s where the export leaves it or its BlendTime out
/// (UAnimMontage's constructor).
fn blend_time(b: Option<&Json>) -> f64 {
    num_or(b.and_then(|b| b.get("BlendTime")), 0.25)
}

/// One slot track's segments and the section chain (rate 1).
pub fn montage(scope: &Scope<'_, '_>, path: &str, opts: MontageOpts) -> Result<SceneMontage, KindError> {
    let o = scope.load(path)?;
    if o.class() != "/Script/Engine.AnimMontage" {
        return Err(fail(format!("{path} is a {}, not a montage", o.class())));
    }
    let mon = &o.props;
    let slots = arr(mon.get("SlotAnimTracks"));
    if (!opts.with_blend && !opts.podium && blend_time(mon.get("BlendIn")) != 0.0)
        || num_or(mon.get("RateScale"), 1.0) != 1.0
        || slots.len() != 1
    {
        return Err(fail(format!("{path}: montage blend / rate / slots")));
    }
    let segs = arr(slots[0].get("AnimTrack").and_then(|t| t.get("AnimSegments")));
    // A podium montage may blend in (Jake_Loss_Montage: no BlendIn, the class default 0.25): its blend time is kept
    // for the podium scene, which blends the montage in over it.
    let blend_in = if opts.with_blend || (opts.podium && blend_time(mon.get("BlendIn")) != 0.0) {
        Some(blend_time(mon.get("BlendIn")))
    } else {
        None
    };
    let (slot, blend_out, notes) = if opts.with_blend {
        (
            Some(str_of(slots[0].get("SlotName")).unwrap_or_default().to_owned()),
            Some(blend_time(mon.get("BlendOut"))),
            Some(notifies(scope, &o)?),
        )
    } else {
        (None, None, None)
    };
    let segments = segs
        .iter()
        .map(|g| MontageSegment {
            clip: short_of(str_of(g.get("AnimReference")).unwrap_or_default()).to_owned(),
            start: num_or(g.get("StartPos"), 0.0),
            from: num_or(g.get("AnimStartTime"), 0.0),
            to: num_or(g.get("AnimEndTime"), 0.0),
            rate: num_or(g.get("AnimPlayRate"), 1.0),
        })
        .collect();
    // A section's time is its link value (FAnimLinkableElement::GetTime, Absolute link: the montage time). A Relative
    // link at value 0 (C014_S03_Emote_Defeat_Montage's Loop) is its segment's start, the cooked SegmentBeginTime,
    // which is that segment's StartPos; another Relative value is refused (Unverified: the rule past 0 is not read).
    let mut sections = Vec::new();
    for c in arr(mon.get("CompositeSections")) {
        let seg = segs.get(num_or(c.get("SegmentIndex"), 0.0) as usize);
        let lm = str_of(c.get("LinkMethod")).unwrap_or("EAnimLinkMethod::Absolute");
        let link = num_or(c.get("LinkValue"), 0.0);
        let begin = num_or(c.get("SegmentBeginTime"), 0.0);
        let relative = lm == "EAnimLinkMethod::Relative"
            && link == 0.0
            && seg.is_some_and(|s| num_or(s.get("StartPos"), 0.0) == begin);
        if lm != "EAnimLinkMethod::Absolute" && !relative {
            return Err(fail(format!(
                "{path}: section link {lm} {}",
                num(c.get("LinkValue")).map_or("undefined".into(), |v| v.to_string())
            )));
        }
        sections.push(MontageSection {
            name: str_of(c.get("SectionName")).unwrap_or_default().to_owned(),
            next: str_of(c.get("NextSectionName")).unwrap_or_default().to_owned(),
            start: if relative { begin } else { link },
        });
    }
    Ok(SceneMontage {
        slot,
        blend_in,
        blend_out,
        notifies: notes,
        name: asset_name(path).to_owned(),
        segments,
        sections,
        length: num_or(mon.get("SequenceLength"), 0.0),
    })
}

/// A notify value: a reference as its package below Content/ ("" for None), structs and arrays member by member.
fn notify_value(v: &Json) -> Json {
    match v {
        Json::Null => Json::Str(String::new()),
        Json::Str(s) => {
            let reference = (s.starts_with("/Game/") || s.starts_with("/Engine/")) && s[1..].contains('.');
            Json::Str(if reference { content_pkg(s) } else { s.clone() })
        }
        Json::Arr(a) => Json::Arr(a.iter().map(notify_value).collect()),
        Json::Obj(m) => Json::Obj(m.iter().map(|(k, x)| (k.clone(), notify_value(x))).collect()),
        x => x.clone(),
    }
}

/// The notifies: `class` the notify's Blueprint class (_C dropped), `state`, `t` the link value, `duration`, the
/// trigger offsets and `props` the notify object's properties over its class default object's own.
fn notifies(scope: &Scope<'_, '_>, mon: &AObj<'_>) -> Result<Vec<MontageNotify>, KindError> {
    let mut out = Vec::new();
    for n in arr(mon.get("Notifies")) {
        if let Some(lm) = str_of(n.get("LinkMethod"))
            && !lm.is_empty()
            && lm != "EAnimLinkMethod::Absolute"
        {
            return Err(fail(format!("{}: notify link {lm}", mon.path())));
        }
        let state_ref = str_of(n.get("NotifyStateClass")).filter(|s| !s.is_empty());
        let r = state_ref.or_else(|| str_of(n.get("Notify")).filter(|s| !s.is_empty()));
        let mut note = MontageNotify {
            name: str_of(n.get("NotifyName")).map(str::to_owned),
            class: None,
            state: state_ref.is_some(),
            t: num_or(n.get("LinkValue"), 0.0),
            duration: if state_ref.is_some() { num_or(n.get("Duration"), 0.0) } else { 0.0 },
            offset: num_or(n.get("TriggerTimeOffset"), 0.0),
            end_offset: num_or(n.get("EndTriggerTimeOffset"), 0.0),
            props: serde_json::Value::Object(serde_json::Map::new()),
        };
        if let Some(r) = r {
            let o = inner_at(mon, r, "notify object")?;
            let Some(tpl) = o.template()? else {
                return Err(fail(format!("{}: notify {r} has no class default", mon.path())));
            };
            let cdo = scope.load(&tpl)?;
            note.class = Some(super::util::class_name(o.class()));
            let mut props = over_props(Some(&cdo.props), Some(&o.props));
            props.retain(|k, _| k != "NotifyColor" && k != "bShouldFireInEditor");
            let v = notify_value(&Json::Obj(props));
            note.props = serde_json::to_value(&v).map_err(|e| fail(format!("{}: notify props: {e}", mon.path())))?;
        }
        out.push(note);
    }
    Ok(out)
}

// ---------------------------------------------------------------- lights

/// A rig component: its class, its properties, the variable it attaches to.
#[derive(Clone)]
struct RigComp {
    ty: String,
    props: Option<Props>,
    parent: Option<String>,
}

/// A rig class's components by SCS variable name: the parent class's (a /Game Blueprint), each overridden by this
/// class's InheritableComponentHandler record (merged with over_props), then its own SCS nodes in their templates'
/// export order, each with the component it attaches to.
fn rig_components(scope: &Scope<'_, '_>, cls: &str) -> Result<IndexMap<String, RigComp>, KindError> {
    let c = scope.load(cls)?;
    let mut comps: IndexMap<String, RigComp> = IndexMap::new();
    if let Some(sup) = c.super_path()?
        && sup.starts_with("/Game/")
    {
        comps = rig_components(scope, &sup)?;
    }
    let ich = c.inner.values().find(|o| class_of(o) == "InheritableComponentHandler");
    for r in ich.map(|i| arr(i.get("Records"))).unwrap_or_default() {
        let v = str_of(r.get("ComponentKey").and_then(|k| k.get("SCSVariableName"))).unwrap_or_default().to_owned();
        let class = short_of(str_of(r.get("ComponentClass")).unwrap_or_default()).to_owned();
        let Some(was) = comps.get(&v).filter(|w| w.ty == class).cloned() else {
            return Err(fail(format!("{cls}: override of {v}")));
        };
        let tpl = inner_at(&c, str_of(r.get("ComponentTemplate")).unwrap_or_default(), "rig override")?;
        comps.insert(v, RigComp { props: Some(over_props(was.props.as_ref(), Some(&tpl.props))), ..was });
    }
    let nodes: Vec<_> = inner_list(&c).into_iter().filter(|o| class_of(o) == "SCS_Node").collect();
    let tpl = |n: &AObj<'_>| inner_at(&c, str_of(n.get("ComponentTemplate")).unwrap_or_default(), "rig template");
    let parent_of = |n: &AObj<'_>| -> Option<String> {
        if let Some(named) = str_of(n.get("ParentComponentOrVariableName"))
            && !named.is_empty()
            && named != "None"
        {
            return Some(named.to_owned());
        }
        nodes
            .iter()
            .find(|m| arr(m.get("ChildNodes")).iter().any(|x| str_of(Some(x)) == Some(n.path())))
            .map(|m| str_of(m.get("InternalVariableName")).unwrap_or_default().to_owned())
    };
    let mut sorted = Vec::with_capacity(nodes.len());
    for n in &nodes {
        sorted.push((tpl(n)?, n.clone()));
    }
    sorted.sort_by_key(|(t, _)| t.index());
    for (t, n) in sorted {
        let v = str_of(n.get("InternalVariableName")).unwrap_or_default().to_owned();
        let props = (!t.props.is_empty()).then(|| t.props.clone());
        if comps.contains_key(&v) {
            // a child class's own DefaultSceneRoot node: the inherited root stands (USimpleConstructionScript)
            if v != "DefaultSceneRoot" || props.is_some() {
                return Err(fail(format!("{cls}: SCS node {v} over an inherited one")));
            }
            continue;
        }
        let ty = short_of(str_of(n.get("ComponentClass")).unwrap_or_default()).to_owned();
        comps.insert(v, RigComp { ty, props, parent: parent_of(&n) });
    }
    Ok(comps)
}

/// The rig's mirror sense: the nearest class of its chain in RIG_MIRROR (tables.rs). A class of the chain whose own
/// functions name AssignLightingChannels and that the table lacks is refused.
pub fn rig_mirror_when(scope: &Scope<'_, '_>, cls: &str) -> Result<MirrorWhen, KindError> {
    let mut cur = Some(cls.to_owned());
    while let Some(c) = cur.filter(|c| c.starts_with("/Game/")) {
        if let Some((_, m)) = RIG_MIRROR.iter().find(|(k, _)| *k == c) {
            return Ok(*m);
        }
        let o = scope.load(&c)?;
        let children = o.native("Children")?.unwrap_or(Json::Arr(Vec::new()));
        let text = to_vec(&children).map_err(|e| fail(e.to_string()))?;
        if text.windows(22).any(|w| w == b"AssignLightingChannels") {
            return Err(fail(format!("{c} defines AssignLightingChannels: not in menuscene tables.rs RIG_MIRROR")));
        }
        cur = o.super_path()?;
    }
    Err(fail(format!("{cls}: no AssignLightingChannels in the chain")))
}

/// FColor to FLinearColor (the 4.26 sRGB table: FLinearColor(const FColor&)). Unverified.
pub fn srgb(c: f64) -> f64 {
    if c / 255.0 <= 0.04045 { c / 255.0 / 12.92 } else { libm::pow((c / 255.0 + 0.055) / 1.055, 2.4) }
}

/// A light property with SPOT_DEFAULTS under the template's values.
fn spot(q: Option<&Props>, k: &str) -> Option<Json> {
    if let Some(v) = q.and_then(|q| q.get(k)) {
        return Some(v.clone());
    }
    let d = match k {
        "Intensity" => Json::Num(5000.0),
        "InnerConeAngle" => Json::Num(0.0),
        "OuterConeAngle" => Json::Num(44.0),
        "AttenuationRadius" => Json::Num(1000.0),
        "SourceRadius" | "SoftSourceRadius" => Json::Num(0.0),
        "SpecularScale" => Json::Num(1.0),
        "CastShadows" | "CastDynamicShadows" => Json::Bool(true),
        "bCastShadowsFromCinematicObjectsOnly" => Json::Bool(false),
        _ => return None,
    };
    Some(d)
}

/// `a || 0` of a vector member.
fn or0(v: Option<&Json>, k: &str) -> f64 {
    let x = num(v.and_then(|v| v.get(k))).unwrap_or(0.0);
    if x.is_nan() { 0.0 } else { x }
}

/// The rig's spot lights. SpawnLighting (BP_CharacterPreview): the rig spawns at the preview actor's transform and
/// its Lights component is scaled (1, -1, 1) on the rig's side (rig_mirror_when). Values the templates leave out are
/// the 4.26 class defaults, the unit conversion and cone clamps FSpotLightSceneProxy's (Unverified). The lights are
/// in the rig's own space (origin 0, unmirrored), each saying whether it sits under Lights (the reader mirrors it).
pub fn rig_lights(scope: &Scope<'_, '_>, rig: &str) -> Result<Vec<SpotLight>, KindError> {
    let d = std::f64::consts::PI / 180.0;
    let comps = rig_components(scope, rig)?;
    let mut out = Vec::new();
    for (v, c) in &comps {
        match c.ty.as_str() {
            "SceneComponent" => {
                // a scene component's own visibility is not its children's; only a location is added to the lights
                // under it
                if c.props.as_ref().is_some_and(|p| {
                    p.keys().any(|k| k != "bVisible" && k != "bHiddenInGame" && k != "RelativeLocation")
                }) {
                    return Err(fail(format!("{rig}: rig scene component transform {v}")));
                }
                continue;
            }
            "SkeletalMeshComponent" => {
                let p = c.props.as_ref();
                if !(truthy(p.and_then(|p| p.get("bHiddenInGame")))
                    || p.and_then(|p| p.get("bVisible")) == Some(&Json::Bool(false)))
                {
                    return Err(fail(format!("{rig}: visible mesh {v}")));
                }
                continue;
            }
            "SpotLightComponent" => {}
            t => return Err(fail(format!("{rig}: rig component {t}"))),
        }
        let mut mirror = false;
        let (mut scaled, mut fixed) = ([0.0f64; 3], [0.0f64; 3]);
        let mut p = c.parent.clone();
        while let Some(pn) = p {
            let Some(pc) = comps.get(&pn).filter(|x| x.ty == "SceneComponent") else {
                return Err(fail(format!("{rig}: light {v} under {pn}")));
            };
            if pn == "Lights" {
                mirror = true;
            }
            if let Some(at) = pc.props.as_ref().and_then(|q| q.get("RelativeLocation"))
                && !matches!(at, Json::Null)
            {
                let x = [or0(Some(at), "X"), or0(Some(at), "Y"), or0(Some(at), "Z")];
                let t = if mirror { &mut fixed } else { &mut scaled };
                for k in 0..3 {
                    t[k] += x[k];
                }
            }
            p = pc.parent.clone();
        }
        if !mirror {
            for k in 0..3 {
                fixed[k] += scaled[k];
                scaled[k] = 0.0;
            }
        }
        if mirror && fixed[1] != 0.0 {
            return Err(fail(format!("{rig}: light {v}: a Y offset above Lights")));
        }
        let q = c.props.as_ref();
        let g = |k: &str| spot(q, k);
        let unhandled = str_of(g("IntensityUnits").as_ref()) != Some("ELightUnits::Candelas")
            || truthy(g("bUseTemperature").as_ref())
            || truthy(g("IESTexture").as_ref())
            || g("bUseInverseSquaredFalloff") == Some(Json::Bool(false))
            || truthy(g("RelativeScale3D").as_ref())
            || truthy(g("SourceLength").as_ref())
            || truthy(g("LightingChannels").as_ref());
        if unhandled {
            return Err(fail(format!("{rig}: {v}: unhandled light property")));
        }
        let n = |k: &str| num(g(k).as_ref()).ok_or_else(|| fail(format!("{rig}: {v}: {k} is no number")));
        let rr = g("RelativeRotation");
        let (pitch, yaw) = (or0(rr.as_ref(), "Pitch") * d, or0(rr.as_ref(), "Yaw") * d);
        let fwd = [libm::cos(pitch) * libm::cos(yaw), libm::cos(pitch) * libm::sin(yaw), libm::sin(pitch)];
        let inner = js::min(js::max(n("InnerConeAngle")?, 0.0), 89.0) * d;
        let outer = js::min(js::max(n("OuterConeAngle")? * d, inner + 0.001), 89.0 * d + 0.001);
        let c0 = n("Intensity")? * 100.0 * 100.0;
        let color = match g("LightColor") {
            Some(l) if truthy(Some(&l)) => {
                let ch = |k: &str| c0 * srgb(num(l.get(k)).unwrap_or(0.0));
                [ch("R"), ch("G"), ch("B")]
            }
            _ => [c0; 3],
        };
        let rl = g("RelativeLocation");
        let rl = |k: &str| num(rl.as_ref().and_then(|r| r.get(k))).unwrap_or(0.0);
        // The interaction's HasShadow for a movable light and a skeletal mesh (141cb0c70): CastShadows &&
        // CastDynamicShadows; bCastShadowsFromCinematicObjectsOnly would need the primitive's bCastCinematicShadow.
        let flag = |k: &str| match g(k) {
            Some(Json::Bool(b)) => Ok(b),
            x => Err(fail(format!("{rig}: {v}: {k} {x:?}"))),
        };
        if flag("bCastShadowsFromCinematicObjectsOnly")? {
            return Err(fail(format!("{rig}: {v}: bCastShadowsFromCinematicObjectsOnly")));
        }
        let cast = flag("CastShadows")? && flag("CastDynamicShadows")?;
        out.push(SpotLight {
            name: v.clone(),
            pos: [fixed[0] + scaled[0] + rl("X"), fixed[1] + scaled[1] + rl("Y"), fixed[2] + scaled[2] + rl("Z")],
            dir: fwd.map(|x| -x),
            color,
            src: [n("SourceRadius")?, n("SoftSourceRadius")?, n("SpecularScale")?],
            att: [1.0 / n("AttenuationRadius")?, libm::cos(outer), 1.0 / (libm::cos(inner) - libm::cos(outer))],
            cast_shadows: cast,
            mirror: Some(mirror),
        });
    }
    if out.is_empty() {
        return Err(fail(format!("{rig}: no lights")));
    }
    Ok(out)
}

/// An object member as props (`?? {}` of an object).
pub fn props_of(v: Option<&Json>) -> Props {
    obj_of(v).cloned().unwrap_or_default()
}
