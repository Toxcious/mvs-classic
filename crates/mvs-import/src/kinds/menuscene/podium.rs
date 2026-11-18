//! The podium: Map_PodiumStage_New's camera targets and lens, background planes, winner text, banner pieces, preview
//! actors and each fighter's end-of-game placements and montages, the level's lights, and the intro sequences.
//!
//! In detail: the camera targets and lens, the three background planes with their materials (the level Blueprint
//! swaps the colour and trophy planes' material for the Blue / Red instance), the winner text, the banner actor's
//! pieces, the four preview actors and each fighter's end-of-game placements and montages, the level's lights, and the
//! intro sequences: Seq_WinnerIntro_Geo's fade, sounds and transform keys, and the event keys of Seq_WinnerIntro_2v2 /
//! _FFA resolved through their directors (tables.rs DIRECTOR_CALLS). Key times in seconds; tangents per second (the
//! channels store them per tick).

use indexmap::IndexMap;
use mvs_material::menuscene::{
    BannerPiece, BannerScene, BannerWidget, Channel, DirLight, Key, PodiumBanner, PodiumCamera, PodiumFighter,
    PodiumFighterSkins, PodiumPlane, PodiumPreview, PodiumScene, PodiumSequence, PodiumSound, Point, ScaledPoint,
    SceneEvent, WeightedKey, WinnerText,
};
use mvs_math::libm;

use super::mainmenu::{PREVIEW_CLASS, is_plane, lens, material_of, sort_of};
use super::material::{Draw, scene_material};
use super::prematch::{blob_shadow, shadow_place};
use super::preview::{MontageOpts, montage, props_of, srgb};
use super::skins::{CharacterSkins, character_skins};
use super::tables::director_call;
use super::util::{
    AObj, Level, Obj, Props, Scope, arr, asset_name, chain_of, class_inner, class_name, class_of, dig, fail, inner_at,
    inner_list, num, num_or, over_props, rot, short_of, str_of, truthy, vec,
};
use crate::fighter_port::CHARACTERS;
use crate::json::Json;
use crate::kind::KindError;

const PD: &str = "/Game/Panda_Main/Maps/Map_PodiumStage/";
const TICKS: f64 = 24000.0;
const CRV_SCURVE: &str = "/Game/Panda_Main/PreMatch/CRV_SCurve.CRV_SCurve";
const BANNER_CLASS: &str = "/Game/Panda_Main/Maps/Map_PodiumStage/BP_Banner_PM.BP_Banner_PM_C";

/// An asset below the podium's folder.
fn at(name: &str) -> String {
    format!("{PD}{name}.{}", &name[name.rfind('/').map_or(0, |i| i + 1)..])
}

/// EMovieSceneTransformChannel bits: translation X Y Z = 1 2 4, rotation 8 16 32, scale 64 128 256 (no mask stored:
/// the class default, all channels). A masked-out channel leaves the actor's own value.
pub const TRANSFORM_CHANNELS: [&str; 9] = [
    "Translation",
    "Translation[1]",
    "Translation[2]",
    "Rotation",
    "Rotation[1]",
    "Rotation[2]",
    "Scale",
    "Scale[1]",
    "Scale[2]",
];

/// A podium channel's cubic keys.
fn keys(c: &Props) -> Result<Vec<Key>, KindError> {
    let constant = |k: &str| str_of(c.get(k)).unwrap_or("RCCE_Constant") == "RCCE_Constant";
    if !constant("PreInfinityExtrap") || !constant("PostInfinityExtrap") {
        return Err(fail("podium: curve extrapolation"));
    }
    let values = arr(c.get("Values"));
    let mut out = Vec::new();
    for (i, t) in arr(c.get("Times")).iter().enumerate() {
        let v = values.get(i).ok_or_else(|| fail("podium: a key with no value"))?;
        let tan = v.get("Tangent");
        let cubic = v.get("InterpMode").is_none_or(|m| *m == Json::Num(2.0));
        let unweighted = tan.and_then(|t| t.get("TangentWeightMode")).is_none_or(|m| *m == Json::Num(0.0));
        if !cubic || !unweighted {
            return Err(fail("podium: key interpolation"));
        }
        out.push(Key {
            t: num_or(t.get("Value"), f64::NAN) / TICKS,
            v: num_or(v.get("Value"), 0.0),
            at: num_or(tan.and_then(|t| t.get("ArriveTangent")), 0.0) * TICKS,
            lt: num_or(tan.and_then(|t| t.get("LeaveTangent")), 0.0) * TICKS,
        });
    }
    Ok(out)
}

fn chan(c: &Props) -> Result<Channel, KindError> {
    Ok(if !arr(c.get("Times")).is_empty() {
        Channel::Keys { keys: keys(c)? }
    } else {
        Channel::Value { value: truthy(c.get("bHasDefaultValue")).then(|| num_or(c.get("DefaultValue"), 0.0)) }
    })
}

/// A level sequence and its MovieScene's properties (its export's inner object).
pub fn movie_scene<'a>(scope: &Scope<'_, 'a>, seq: &str) -> Result<(Obj<'a>, Props), KindError> {
    let o = scope.load(seq)?;
    let ms = inner_at(&o, str_of(o.get("MovieScene")).unwrap_or_default(), "MovieScene")?.props.clone();
    Ok((o, ms))
}

/// A range bound's frame (`<range>.<bound>.Value.Value`).
fn bound(range: Option<&Json>, b: &str) -> Result<f64, KindError> {
    num(dig(range, &[b, "Value", "Value"])).ok_or_else(|| fail(format!("a range's {b}")))
}

fn geo_sequence(scope: &Scope<'_, '_>) -> Result<PodiumSequence, KindError> {
    let (geo, ms) = movie_scene(scope, &at("Seq_WinnerIntro_Geo"))?;
    let obj = |p: &Json| inner_at(&geo, str_of(Some(p)).unwrap_or_default(), "Seq_WinnerIntro_Geo");
    if num(dig(ms.get("TickResolution"), &["Numerator"])) != Some(TICKS) {
        return Err(fail("podium: tick resolution"));
    }
    let possessable: IndexMap<&str, &str> = arr(ms.get("Possessables"))
        .iter()
        .map(|x| (str_of(x.get("Guid")).unwrap_or_default(), str_of(x.get("Name")).unwrap_or("undefined")))
        .collect();
    let mut transforms = IndexMap::new();
    for b in arr(ms.get("ObjectBindings")) {
        for tp in arr(b.get("Tracks")) {
            let tr = obj(tp)?;
            let secs = arr(tr.get("Sections"));
            if class_of(&tr) != "MovieScene3DTransformTrack" || secs.len() != 1 {
                return Err(fail(format!("podium: binding track {}", tr.class())));
            }
            let sp = obj(&secs[0])?;
            let mask = num_or(dig(sp.get("TransformMask"), &["Mask"]), 511.0) as u32;
            let mut ch = Vec::new();
            for (i, n) in TRANSFORM_CHANNELS.iter().enumerate() {
                ch.push(if (mask >> i) & 1 != 0 { Some(chan(&props_of(sp.get(n)))?) } else { None });
            }
            let name = possessable.get(str_of(b.get("ObjectGuid")).unwrap_or_default()).copied().unwrap_or("undefined");
            transforms.insert(name.to_owned(), ch);
        }
    }
    let mut fade = None;
    let mut sounds = Vec::new();
    for tp in arr(ms.get("MasterTracks")) {
        let tr = obj(tp)?;
        for sp in arr(tr.get("Sections")) {
            let s = obj(sp)?;
            match class_of(&s) {
                "MovieSceneFadeSection" => fade = Some(keys(&props_of(s.get("FloatCurve")))?),
                "MovieSceneAkAudioEventSection" => sounds.push(PodiumSound {
                    t: bound(dig(s.get("SectionRange"), &["Value"]), "LowerBound")? / TICKS,
                    ev: str_of(s.get("EventName")).unwrap_or_default().to_owned(),
                }),
                c => return Err(fail(format!("podium: master section {c}"))),
            }
        }
    }
    let range = dig(ms.get("PlaybackRange"), &["Value"]);
    let length = (bound(range, "UpperBound")? - bound(range, "LowerBound")?) / TICKS;
    Ok(PodiumSequence { length, fade, sounds, transforms, events: IndexMap::new() })
}

/// The event keys of a sequence's trigger sections through its director (keys at one time in track order).
fn events(scope: &Scope<'_, '_>, name: &str) -> Result<Vec<SceneEvent>, KindError> {
    let o = scope.load(&at(name))?;
    let mut out = Vec::new();
    for s in inner_list(&o).into_iter().filter(|x| class_of(x) == "MovieSceneEventTriggerSection") {
        let c = s.get("EventChannel");
        let values = arr(dig(c, &["KeyValues"]));
        for (i, t) in arr(dig(c, &["KeyTimes"])).iter().enumerate() {
            let f = str_of(dig(values.get(i), &["Ptrs", "Function"])).unwrap_or_default();
            let d = director_call(name, f)?;
            out.push(SceneEvent {
                t: num_or(t.get("Value"), f64::NAN) / TICKS,
                call: d.call.to_owned(),
                args: d.args.iter().map(|a| a.value()).collect(),
                target: None,
            });
        }
    }
    // (a stable sort: keys at one time keep their order)
    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out)
}

/// The podium's lights: the level's directional lights by lighting channel (1 winners, 2 losers); its point lights
/// are the previews' own, intensity 0. Values the components leave out are the 4.26 class defaults. Unverified:
/// LightColor through FLinearColor(FColor) is 4.26 source; the key light's dynamic shadows are not exported.
fn lights(scope: &Scope<'_, '_>, map: &Level<'_>) -> Result<IndexMap<String, Vec<DirLight>>, KindError> {
    let d = std::f64::consts::PI / 180.0;
    let mut out: IndexMap<String, Vec<DirLight>> = IndexMap::from([("1".into(), Vec::new()), ("2".into(), Vec::new())]);
    let all = inner_list(&map.world);
    for e in &all {
        let c = class_of(e);
        if !c.ends_with("LightComponent") || c == "SkyLightComponent" || c == "PointLightComponent" {
            continue;
        }
        let q = &e.props;
        if q.get("bAffectsWorld") == Some(&Json::Bool(false)) {
            continue;
        }
        if c != "DirectionalLightComponent"
            || truthy(q.get("bUseTemperature"))
            || truthy(q.get("LightFunctionMaterial"))
            || truthy(q.get("Mobility"))
        {
            return Err(fail(format!("podium: light {}", e.path())));
        }
        let ch = q.get("LightingChannels").filter(|c| truthy(Some(c)));
        let channel = |k: &str| ch.and_then(|c| c.get(k));
        if channel("bChannel0") != Some(&Json::Bool(false))
            || truthy(channel("bChannel1")) == truthy(channel("bChannel2"))
        {
            return Err(fail(format!("podium: light channels {}", e.path())));
        }
        let rr = q.get("RelativeRotation");
        let (p, y) = (num_or(dig(rr, &["Pitch"]), 0.0) * d, num_or(dig(rr, &["Yaw"]), 0.0) * d);
        let intensity = num_or(q.get("Intensity"), 10.0);
        let lc = q.get("LightColor").filter(|l| truthy(Some(l)));
        let base = match lc {
            Some(l) => ["R", "G", "B"].map(|k| srgb(num_or(l.get(k), 0.0))),
            None => [1.0; 3],
        };
        let translucent = match q.get("bAffectTranslucentLighting") {
            None | Some(Json::Null) => true,
            Some(Json::Bool(b)) => *b,
            Some(_) => return Err(fail(format!("podium: bAffectTranslucentLighting {}", e.path()))),
        };
        let light = DirLight {
            name: Level::actor_of(e),
            dir: [-libm::cos(p) * libm::cos(y), -libm::cos(p) * libm::sin(y), -libm::sin(p)],
            color: base.map(|v| v * intensity),
            src: [
                libm::sin(0.5 * num_or(q.get("LightSourceAngle"), 0.5357) * d),
                libm::sin(0.5 * num_or(q.get("LightSourceSoftAngle"), 0.0) * d),
                num_or(q.get("SpecularScale"), 1.0),
            ],
            cast_shadows: q.get("CastShadows") != Some(&Json::Bool(false))
                && q.get("CastDynamicShadows") != Some(&Json::Bool(false)),
            // bAffectTranslucentLighting (default true; DirectionalLight_L_Key2 stores False): injected into the
            // translucency lighting volume with no lighting channel test
            translucent,
            guid: q.get("LightGuid").and_then(Json::as_str).filter(|g| !g.is_empty()).map(|g| g.replace('-', "")),
        };
        out[if truthy(channel("bChannel1")) { "1" } else { "2" }].push(light);
    }
    let cls = scope.load(PREVIEW_CLASS)?;
    for e in all.iter().filter(|x| class_of(x) == "PointLightComponent") {
        let name = short_of(e.path());
        let tpl = cls.inner.get(&format!("{name}_GEN_VARIABLE"));
        let off =
            tpl.is_some_and(|t| over_props(Some(&t.props), Some(&e.props)).get("Intensity") == Some(&Json::Num(0.0)));
        if !off {
            return Err(fail(format!("podium: point light {}", e.path())));
        }
    }
    Ok(out)
}

/// A vector member with a default.
fn v3(q: &Props, k: &str, d: f64) -> Result<[f64; 3], KindError> {
    vec(q.get(k), Some([d; 3]))
}

/// The banner actor (BP_Banner_PM, 3 and 13): the component templates of the pieces the end of game shows with the
/// default banner.
fn banner(scope: &Scope<'_, '_>) -> Result<PodiumBanner, KindError> {
    let bj = scope.load(BANNER_CLASS)?;
    let bcomp = |n: &str| class_inner(&bj, &format!("{n}_GEN_VARIABLE")).map(|o| o.props.clone());
    let nodes: Vec<Obj<'_>> = inner_list(&bj).into_iter().filter(|x| class_of(x) == "SCS_Node").collect();
    let scs = |n: &str| {
        let end = format!(":{n}_GEN_VARIABLE");
        nodes
            .iter()
            .find(|x| str_of(x.get("ComponentTemplate")).unwrap_or_default().ends_with(&end))
            .cloned()
            .ok_or_else(|| fail(format!("BP_Banner_PM: no node {n}")))
    };
    let children_of = |n: &str| -> Result<Vec<String>, KindError> {
        let mut out = Vec::new();
        for c in arr(scs(n)?.get("ChildNodes")) {
            let node = inner_at(&bj, str_of(Some(c)).unwrap_or_default(), "BP_Banner_PM node")?;
            out.push(chain_of(str_of(node.get("ComponentTemplate")).unwrap_or_default()).replacen(
                "_GEN_VARIABLE",
                "",
                1,
            ));
        }
        Ok(out)
    };
    let bpiece = |n: &str, how: Draw| -> Result<BannerPiece, KindError> {
        let q = bcomp(n)?;
        if !is_plane(&q) || truthy(q.get("TranslucencySortPriority")) {
            return Err(fail(format!("BP_Banner_PM: piece {n}")));
        }
        Ok(BannerPiece {
            loc: v3(&q, "RelativeLocation", 0.0)?,
            rot: rot(q.get("RelativeRotation")),
            scale: v3(&q, "RelativeScale3D", 1.0)?,
            material: scene_material(scope.cx, material_of(&q)?, how)?,
        })
    };
    let mut mask_children = children_of("Banner Mask")?;
    mask_children.sort();
    let placement = children_of("Banner Placement")?;
    if mask_children.join(",") != "Banner Default Background,Banner Filter,Banner Nameplate,Banner Trim Mask"
        || !placement.iter().any(|c| c == "Banner Mask")
    {
        return Err(fail("BP_Banner_PM: component tree"));
    }
    let bm = bcomp("Banner Mask")?;
    let bp = bcomp("Banner Placement")?;
    if bm.get("bRenderInMainPass") != Some(&Json::Bool(false))
        || !truthy(bm.get("bRenderCustomDepth"))
        || truthy(bp.get("RelativeLocation"))
        || truthy(bp.get("RelativeScale3D"))
    {
        return Err(fail("BP_Banner_PM: mask"));
    }
    let bcdo =
        scope.load(&format!("{}.Default__BP_Banner_PM_C", &BANNER_CLASS[..BANNER_CLASS.rfind('.').unwrap_or(0)]))?;
    let btl = inner_list(&bj)
        .into_iter()
        .find(|x| class_of(x) == "TimelineTemplate")
        .ok_or_else(|| fail("BP_Banner_PM: no timeline"))?;
    let tracks = arr(btl.get("FloatTracks"));
    let curve_ref = str_of(tracks.first().and_then(|t| t.get("CurveFloat"))).unwrap_or_default();
    let curve_obj = inner_at(&bj, curve_ref, "banner curve")?;
    let bcurve = arr(dig(curve_obj.get("FloatCurve"), &["Keys"]));
    let linear = bcurve.iter().all(|k| str_of(k.get("InterpMode")).is_none_or(|m| m.is_empty() || m == "RCIM_Linear"));
    if tracks.len() != 1
        || bcurve.len() != 2
        || !linear
        || num_or(bcurve[0].get("Time"), 0.0) != 0.0
        || num_or(bcurve[0].get("Value"), 0.0) != 0.0
        || bcurve[1].get("Value") != Some(&Json::Num(1.0))
    {
        return Err(fail("BP_Banner_PM: entrance timeline"));
    }
    // the widget components on Banner Placement: EOG Nametag and ToastButton (screen space) and Stat Sheet (world
    // space), drawn at the widget's desired size around the default pivot. A screen space component's rotation and
    // scale do not reach its widget (ToastButton's RelativeScale3D 0.9): a rotation is refused, a scale kept unused.
    let bwidget = |n: &str, screen: bool| -> Result<BannerWidget, KindError> {
        let q = bcomp(n)?;
        if !placement.iter().any(|c| c == n)
            || (str_of(q.get("Space")) == Some("EWidgetSpace::Screen")) != screen
            || !truthy(q.get("bDrawAtDesiredSize"))
            || truthy(q.get("Pivot"))
            || truthy(q.get("TintColorAndOpacity"))
            || truthy(q.get("TranslucencySortPriority"))
            || (screen && truthy(q.get("RelativeRotation")))
        {
            return Err(fail(format!("BP_Banner_PM: widget {n}")));
        }
        Ok(BannerWidget {
            loc: v3(&q, "RelativeLocation", 0.0)?,
            rot: rot(q.get("RelativeRotation")),
            scale: v3(&q, "RelativeScale3D", 1.0)?,
            widget: class_name(str_of(q.get("WidgetClass")).unwrap_or_default()),
        })
    };
    // where a banner element is attached
    let bscene = |n: &str, parent: &str| -> Result<BannerScene, KindError> {
        let q = bcomp(n)?;
        if !children_of(parent)?.iter().any(|c| c == n) {
            return Err(fail(format!("BP_Banner_PM: {n} not on {parent}")));
        }
        Ok(BannerScene {
            parent: parent.to_owned(),
            loc: v3(&q, "RelativeLocation", 0.0)?,
            rot: rot(q.get("RelativeRotation")),
            scale: v3(&q, "RelativeScale3D", 1.0)?,
        })
    };
    Ok(PodiumBanner {
        // (the mask is drawn in its custom depth pass alone: bRenderInMainPass false, checked above)
        mask: bpiece("Banner Mask", Draw::CustomDepth)?,
        background: bpiece("Banner Default Background", Draw::Banner)?,
        filter: bpiece("Banner Filter", Draw::Banner)?,
        trim: bpiece("Banner Trim Mask", Draw::Banner)?,
        nametag: bwidget("EOG Nametag", true)?,
        toast_button: bwidget("ToastButton", true)?,
        stat_sheet: bwidget("Stat Sheet", false)?,
        base_element_root: bscene("Base Element Root", "Banner Placement")?,
        parallax_rail: bscene("Parallax Rail", "Banner Placement")?,
        parallax_element_root: bscene("Parallax Element Root", "Parallax Rail")?,
        slant_angle: num(bcdo.get("Slant Angle")).ok_or_else(|| fail("BP_Banner_PM: no Slant Angle"))?,
        // Entrance Timeline: the track's last key time (played at rate 10 by Start EOG Entrance)
        entrance_length: num_or(bcurve[1].get("Time"), 0.0),
    })
}

/// A ScaleOverrides entry of a skin by its key.
pub fn scale_override<'v>(skin: &'v AObj<'_>, key: &str) -> Option<&'v Json> {
    arr(skin.get("ScaleOverrides"))
        .iter()
        .find(|e| str_of(e.as_array().and_then(|a| a.first())) == Some(key))
        .and_then(|e| e.as_array().and_then(|a| a.get(1)))
}

/// A skin's end-of-game placement (EOG_Winner / EOG_Loser ScaleOverrides): relative location and uniform scale.
fn eog_scale(skin: &AObj<'_>, key: &str) -> Result<ScaledPoint, KindError> {
    let ov = scale_override(skin, key);
    let s = |k: &str| dig(ov, &["Scale3D", k]);
    let Some(ov) = ov.filter(|_| s("X") == s("Y") && s("X") == s("Z")) else {
        return Err(fail(format!("podium: {key}")));
    };
    Ok(ScaledPoint {
        loc: vec(ov.get("Translation"), None)?,
        scale: num(s("X")).ok_or_else(|| fail(format!("podium: {key} scale")))?,
    })
}

/// The podium's fighters: per roster key its default skin (and `skins`: each further skin by its SkinData name),
/// the EOG placements and the VictoryMontageOverride / DefeatMontageOverride when set, else the CharacterData's
/// default montages.
fn fighters(
    scope: &Scope<'_, '_>,
    all: &[CharacterSkins<'_>],
    mesh_yaw: f64,
) -> Result<IndexMap<String, PodiumFighterSkins>, KindError> {
    let mut out = IndexMap::new();
    for c in all {
        // (character_C021 sets neither default: its skin's two overrides are set)
        let has_defaults = c.data.get("DefaultVictoryMontage").is_some();
        let podium_of = |skin_path: &str| -> Result<PodiumFighter, KindError> {
            let skin = scope.load(skin_path)?;
            let pick = |o: &str, d: &str| -> Result<String, KindError> {
                let own = str_of(skin.get(o)).filter(|s| !s.is_empty());
                let def = if has_defaults { str_of(c.data.get(d)).filter(|s| !s.is_empty()) } else { None };
                own.or(def)
                    .map(str::to_owned)
                    .ok_or_else(|| fail(format!("podium: {} {} has no {o} and no {d}", c.key, asset_name(skin_path))))
            };
            let victory = pick("VictoryMontageOverride", "DefaultVictoryMontage")?;
            let defeat = pick("DefeatMontageOverride", "DefaultDefeatMontage")?;
            let podium = MontageOpts { podium: true, ..MontageOpts::default() };
            Ok(PodiumFighter {
                skin: asset_name(skin_path).to_owned(),
                mesh_yaw,
                winner: eog_scale(&skin, "EOG_Winner")?,
                loser: eog_scale(&skin, "EOG_Loser")?,
                victory: montage(scope, &victory, podium)?,
                defeat: montage(scope, &defeat, podium)?,
            })
        };
        let default = podium_of(&c.skin)?;
        let skins = if c.further.is_empty() {
            None
        } else {
            let mut m = IndexMap::new();
            for s in &c.further {
                m.insert(asset_name(s).to_owned(), podium_of(s)?);
            }
            Some(m)
        };
        out.insert(c.key.clone(), PodiumFighterSkins { default, skins });
    }
    Ok(out)
}

/// The Podium entry's data.
pub fn bind_podium(scope: &Scope<'_, '_>) -> Result<PodiumScene, KindError> {
    let map = Level::load(scope, &at("Map_PodiumStage_New"))?;
    let point = |name: &str| -> Result<Point, KindError> {
        let q = map.comp_of(name, "SceneComponent")?;
        Ok(Point { loc: v3(&q, "RelativeLocation", 0.0)?, rot: rot(q.get("RelativeRotation")) })
    };
    let curve_obj = scope.load(CRV_SCURVE)?;
    let curve = arr(dig(curve_obj.get("FloatCurve"), &["Keys"]));
    if curve.iter().any(|k| {
        str_of(k.get("InterpMode")) != Some("RCIM_Cubic")
            || str_of(k.get("TangentWeightMode")) != Some("RCTWM_WeightedBoth")
    }) {
        return Err(fail("CRV_SCurve: key modes"));
    }
    let (fov, aspect, vignette) = lens(&map.comp_of("CineCamera", "CineCameraComponent")?, "podium CineCamera")?;
    let camera = PodiumCamera {
        start: point("CameraTarget_Start")?,
        zoom: point("CameraTarget_Zoom")?,
        fov,
        aspect,
        vignette,
        // the level Blueprint's CameraLerp timeline plays CRV_SCurve at (last key time / 0.2) (ZoomToWInner)
        zoom_length: 0.2,
        zoom_curve: curve
            .iter()
            .map(|k| {
                let n = |m: &str| num_or(k.get(m), 0.0);
                WeightedKey {
                    t: n("Time"),
                    v: n("Value"),
                    at: n("ArriveTangent"),
                    aw: n("ArriveTangentWeight"),
                    lt: n("LeaveTangent"),
                    lw: n("LeaveTangentWeight"),
                }
            })
            .collect(),
    };
    let pplane = |name: &str, variants: &[(&str, &str)]| -> Result<PodiumPlane, KindError> {
        let q = map.comp_of(name, "StaticMeshComponent")?;
        if !is_plane(&q) || truthy(q.get("AttachParent")) {
            return Err(fail(format!("podium plane {name}")));
        }
        let mut materials = IndexMap::new();
        materials.insert("default".to_owned(), scene_material(scope.cx, material_of(&q)?, Draw::Plane)?);
        for (k, n) in variants {
            materials.insert((*k).to_owned(), scene_material(scope.cx, &at(&format!("MapBGs/{n}")), Draw::Plane)?);
        }
        Ok(PodiumPlane {
            name: name.to_owned(),
            loc: v3(&q, "RelativeLocation", 0.0)?,
            rot: rot(q.get("RelativeRotation")),
            scale: v3(&q, "RelativeScale3D", 1.0)?,
            sort: sort_of(&q),
            materials,
        })
    };
    let planes = vec![
        pplane("Plane_2", &[])?,
        pplane("UI_Plane_TeamColorBG", &[("Blue", "M_TeamColorBG_Blue"), ("Red", "M_TeamColorBG_Red")])?,
        pplane("UI_Plane_TrophyBG", &[("Blue", "M_TrophyBG_Blue"), ("Red", "M_TrophyBG_Red")])?,
    ];
    // the winner text: BP_WinningTeam_2 (attached to TeamNameText, which the Geo sequence moves) carries a
    // world-space WidgetComponent of WBP_3dTeam
    let wt_path = at("MapBGs/BP_WinningTeam");
    let wt =
        scope.load(&format!("{}.BP_WinningTeam_C", wt_path.strip_suffix(".BP_WinningTeam").unwrap_or(&wt_path)))?;
    let wc = inner_list(&wt).into_iter().find(|x| class_of(x) == "WidgetComponent").map(|c| c.props.clone());
    let Some(wc) = wc.filter(|wc| {
        !(truthy(wc.get("Space"))
            || truthy(wc.get("Pivot"))
            || truthy(wc.get("RelativeRotation"))
            || truthy(wc.get("RelativeScale3D"))
            || truthy(wc.get("TintColorAndOpacity"))
            || !truthy(wc.get("bDrawAtDesiredSize"))
            || str_of(wc.get("BlendMode")) != Some("EWidgetBlendMode::Transparent")
            || truthy(wc.get("TranslucencySortPriority")))
    }) else {
        return Err(fail("podium: winner text component"));
    };
    let text = WinnerText {
        actor: vec(map.comp_of("TeamNameText", "SceneComponent")?.get("RelativeLocation"), None)?,
        child: vec(map.comp_of("BP_WinningTeam_2", "SceneComponent")?.get("RelativeLocation"), None)?,
        component: v3(&wc, "RelativeLocation", 0.0)?,
        widget: class_name(str_of(wc.get("WidgetClass")).unwrap_or_default()),
    };

    // the four preview actors (BP_CharacterPreview, tags Player1..4), each attached to a TargetPoint; a fighter's
    // skin replaces the actor's relative location and the mesh scale by its EOG_Winner / EOG_Loser entry
    let tp_banner: Vec<Obj<'_>> = map
        .all_comps("SceneComponent")
        .into_iter()
        .filter(|e| {
            let a = Level::actor_of(e);
            a.get(..9).is_some_and(|p| p.eq_ignore_ascii_case("TP_Banner"))
                && a.bytes().skip(9).all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .collect();
    let (blob_shadow, shadow_tpl) = blob_shadow(scope)?;
    let mut previews = Vec::new();
    for actor in map.actors("BP_CharacterPreview_C") {
        let an = Level::actor_of(&actor);
        let root = map.comp(&an, "DefaultSceneRoot").ok_or_else(|| fail(format!("podium: preview {an}")))?;
        let tag = arr(actor.get("Tags"))
            .iter()
            .filter_map(|t| str_of(Some(t)))
            .find(|t| t.len() == 7 && t.starts_with("Player") && (b'1'..=b'4').contains(&t.as_bytes()[6]));
        let tp = Level::owner_of(str_of(root.get("AttachParent")).unwrap_or_default());
        let q = map.comp_of(&tp, "SceneComponent")?;
        let s = root.get("RelativeScale3D");
        let sc = |k: &str| dig(s, &[k]);
        let Some(tag) = tag.filter(|_| {
            !truthy(q.get("RelativeRotation"))
                && !truthy(q.get("RelativeScale3D"))
                && sc("X") == sc("Y")
                && sc("X") == sc("Z")
        }) else {
            return Err(fail(format!("podium: preview {an}")));
        };
        // the banner's spawn point: the TargetPoint attached to the preview's root whose name has "banner"
        // (GetBannerSpawn)
        let end = format!(".{an}.DefaultSceneRoot");
        let bp: Vec<&Obj<'_>> =
            tp_banner.iter().filter(|e| str_of(e.get("AttachParent")).is_some_and(|p| p.ends_with(&end))).collect();
        if bp.len() != 1 || truthy(bp[0].get("RelativeRotation")) || truthy(root.get("RelativeRotation")) {
            return Err(fail(format!("podium: banner point of {an}")));
        }
        let bs = bp[0].get("RelativeScale3D").filter(|s| truthy(Some(s)));
        let b = |k: &str| bs.map_or(1.0, |s| num_or(s.get(k), f64::NAN));
        if (b("X") - b("Y")).abs() > 1e-5 || (b("X") - b("Z")).abs() > 1e-5 {
            return Err(fail("podium: banner point scale"));
        }
        previews.push(PodiumPreview {
            shadow: shadow_place(&map, &an, &shadow_tpl)?,
            tag: tag.to_owned(),
            point: tp.clone(),
            loc: vec(q.get("RelativeLocation"), None)?,
            facing_left: truthy(actor.get("FacingLeft")),
            scale: num(sc("X")).ok_or_else(|| fail(format!("podium: preview {an} scale")))?,
            banner: ScaledPoint { loc: v3(&bp[0].props, "RelativeLocation", 0.0)?, scale: b("X") },
        });
    }
    // (code-unit order: no collation)
    previews.sort_by(|a, b| a.tag.cmp(&b.tag));
    if previews.iter().map(|p| p.tag.as_str()).collect::<Vec<_>>() != ["Player1", "Player2", "Player3", "Player4"] {
        return Err(fail("podium: preview tags"));
    }
    let preview_cls = scope.load(PREVIEW_CLASS)?;
    let mesh_yaw = num(dig(class_inner(&preview_cls, "SkeletalMesh_GEN_VARIABLE")?.get("RelativeRotation"), &["Yaw"]))
        .ok_or_else(|| fail("BP_CharacterPreview: no mesh yaw"))?;
    let all = CHARACTERS.iter().map(|k| character_skins(scope, k)).collect::<Result<Vec<_>, _>>()?;
    let mut sequence = geo_sequence(scope)?;
    sequence.events.insert("2v2".into(), events(scope, "Seq_WinnerIntro_2v2")?);
    sequence.events.insert("FFA".into(), events(scope, "Seq_WinnerIntro_FFA")?);
    Ok(PodiumScene {
        camera,
        planes,
        text,
        banner: banner(scope)?,
        previews,
        fighters: fighters(scope, &all, mesh_yaw)?,
        lights: lights(scope, &map)?,
        sequence,
        blob_shadow,
    })
}
