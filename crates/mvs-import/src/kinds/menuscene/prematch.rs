//! The pre-match level and the per-skin previews: the PreMatch level's actors the queue's pre-match and the VS move
//! read, and per skin its placements, PreviewIdle montage and CharacterLighting rig.
//!
//! The level's: anchors and camera targets, the four preview actors, the banner movers and the seven BP_Banner_PM
//! instances, the level's three timelines and the two camera curves, PM_NewSlants' planes, materials and timelines,
//! MainMenuBackground's map lookup, the four level sequences' tracks and the Emitter actors they bind. Per skin: its
//! ScaleOverrides placements, PreviewIdle montage and CharacterLighting rig.
//!
//! Level-placed actors keep the component values their construction script left in the level package (UE 4.26 runs
//! a construction script on spawn, not on a cooked level's load), so a banner's pieces are the class templates under
//! the level's own values, the serialized dynamic material instances included. Unverified: that rule is the 4.26
//! source, not a native read of this build.

use indexmap::{IndexMap, IndexSet};
use mvs_material::menuscene::{
    AudioSection, Background, BannerInstance, BlobShadow, BoolChannel, BoolKey, Curve, CurveKey, CustomDepth,
    CutSection, FadeSection, LevelEmitter, MapPicture, MaterialSection, Mover, ParticleKey, PieceAsset, Placement,
    Point, PreMatchPreview, PreMatchScene, PreviewEntry, Range, SceneEvent, SceneMaterial, SeqChannel, SeqKey,
    SeqTrack, Sequence, ShadowPlace, ShakeSection, SlantPlane, Slants, Timeline, V3, VisibilitySection,
};

use super::mainmenu::{
    BG_CLASS, NEW_SLANTS_WORLD, PLANE, PREMATCH_WORLD, PREVIEW_CLASS, is_plane, material_of, sort_of,
};
use super::material::{Draw, draw_of, scene_material};
use super::podium::{TRANSFORM_CHANNELS, movie_scene, scale_override};
use super::preview::{MontageOpts, montage, props_of, rig_lights, rig_mirror_when};
use super::skins::character_skins;
use super::tables::director_call;
use super::util::{
    AObj, Level, Props, Scope, arr, asset_name, cdo_of, class_inner, class_name, class_of, dig, fail, inner_at,
    inner_list, is_false, num, num_or, over_props, pkg_of, r6, rot6, short_of, str_of, tail, truthy, vec6,
};
use crate::fighter_port::CHARACTERS;
use crate::json::Json;
use crate::kind::KindError;

const PM: &str = "/Game/Panda_Main/PreMatch/";
/// BP_CharacterPreview's BlobShadowCenter template (SCS_Node_9, under DefaultSceneRoot).
const BLOB_SHADOW_TEMPLATE: &str = "BlobShadowCenter_GEN_VARIABLE";

/// BP_CharacterPreview's BlobShadowCenter: its mesh and material, and the template's values.
pub(super) fn blob_shadow(scope: &Scope<'_, '_>) -> Result<(BlobShadow, Props), KindError> {
    let preview_cls = scope.load(PREVIEW_CLASS)?;
    let tpl = class_inner(&preview_cls, BLOB_SHADOW_TEMPLATE)?.props.clone();
    if !is_plane(&tpl) {
        return Err(fail(format!("BlobShadowCenter mesh {:?}", tpl.get("StaticMesh"))));
    }
    let b = BlobShadow { mesh: PLANE.to_owned(), material: scene_material(scope.cx, material_of(&tpl)?, Draw::Plane)? };
    Ok((b, tpl))
}

/// The BlobShadowCenter of the level's preview actor `actor`: the level's values over the template `tpl`.
pub(super) fn shadow_place(level: &Level<'_>, actor: &str, tpl: &Props) -> Result<ShadowPlace, KindError> {
    let sc = level.comp(actor, "BlobShadowCenter").map(|c| c.props.clone());
    let sq = over_props(Some(tpl), sc.as_ref());
    let parent = str_of(sq.get("AttachParent")).unwrap_or_default();
    if sc.is_none() || !parent.ends_with(&format!("{actor}.DefaultSceneRoot")) || !is_plane(&sq) {
        return Err(fail(format!("{actor} BlobShadowCenter")));
    }
    Ok(ShadowPlace {
        loc: vec6(sq.get("RelativeLocation"), Some([0.0; 3]))?,
        rot: rot6(sq.get("RelativeRotation")),
        scale: vec6(sq.get("RelativeScale3D"), Some([1.0; 3]))?,
        hidden: truthy(sq.get("bHiddenInGame")),
    })
}
const BANNER_CLASS: &str = "/Game/Panda_Main/Maps/Map_PodiumStage/BP_Banner_PM.BP_Banner_PM_C";

/// A package's asset object (`<p>.<last name of p>`).
fn asset(p: &str) -> String {
    format!("{p}.{}", &p[p.rfind('/').map_or(0, |i| i + 1)..])
}

/// `v || d` of a string member.
fn str_or<'v>(v: Option<&'v Json>, d: &'v str) -> &'v str {
    str_of(v).filter(|s| !s.is_empty()).unwrap_or(d)
}

// ---------------------------------------------------------------- curves, timelines

/// An FRichCurve key (curve assets, timeline curves): time and tangents in seconds; weights only on weighted keys.
fn rich_key(k: &Json) -> CurveKey {
    let n = |m: &str| r6(num_or(k.get(m), 0.0));
    let w = tail(str_or(k.get("TangentWeightMode"), "RCTWM_WeightedNone")).replacen("RCTWM_", "", 1);
    let weighted = w != "WeightedNone";
    CurveKey {
        t: n("Time"),
        v: n("Value"),
        i: tail(str_or(k.get("InterpMode"), "RCIM_Linear")).replacen("RCIM_", "", 1),
        at: n("ArriveTangent"),
        lt: n("LeaveTangent"),
        aw: weighted.then(|| n("ArriveTangentWeight")),
        lw: weighted.then(|| n("LeaveTangentWeight")),
        w: weighted.then_some(w),
    }
}

fn rich_curve(fc: Option<&Json>) -> Curve {
    let fc = props_of(fc);
    let extrap = |k: &str| {
        str_of(fc.get(k))
            .filter(|s| !s.is_empty() && tail(s) != "RCCE_Constant")
            .map(|s| tail(s).replacen("RCCE_", "", 1))
    };
    Curve {
        keys: arr(fc.get("Keys")).iter().map(rich_key).collect(),
        pre: extrap("PreInfinityExtrap"),
        post: extrap("PostInfinityExtrap"),
        default: num(fc.get("DefaultValue")).filter(|d| d.abs() < 3e38),
    }
}

fn curve_asset(scope: &Scope<'_, '_>, p: &str) -> Result<Curve, KindError> {
    Ok(rich_curve(scope.load(&asset(p))?.get("FloatCurve")))
}

/// A Blueprint timeline's template: float tracks only. UTimelineTemplate defaults: length 5, TL_TimelineLength.
fn timeline(cls: &AObj<'_>, name: &str) -> Result<Timeline, KindError> {
    let tl = class_inner(cls, &format!("{name}_Template"))?;
    if truthy(tl.get("bLoop"))
        || !arr(tl.get("EventTracks")).is_empty()
        || !arr(tl.get("VectorTracks")).is_empty()
        || !arr(tl.get("LinearColorTracks")).is_empty()
    {
        return Err(fail(format!("timeline {name}: unhandled settings")));
    }
    let mut tracks = IndexMap::new();
    for t in arr(tl.get("FloatTracks")) {
        if truthy(t.get("bIsExternalCurve")) {
            return Err(fail(format!("timeline {name}: external curve")));
        }
        let c = inner_at(cls, str_of(t.get("CurveFloat")).unwrap_or_default(), "timeline curve")?;
        tracks.insert(str_of(t.get("TrackName")).unwrap_or("undefined").to_owned(), rich_curve(c.get("FloatCurve")));
    }
    Ok(Timeline {
        length: num_or(tl.get("TimelineLength"), 5.0),
        length_mode: tail(str_or(tl.get("LengthMode"), "TL_TimelineLength")).replacen("TL_", "", 1),
        ignore_time_dilation: truthy(tl.get("bIgnoreTimeDilation")),
        tracks,
    })
}

// ---------------------------------------------------------------- sequences

const INTERP: [&str; 4] = ["Linear", "Constant", "Cubic", "None"];
const ZERO_GUID: &str = "00000000-00000000-00000000-00000000";

/// One level sequence's binding and track reads.
struct Seq<'s, 'a> {
    name: &'s str,
    seq: &'s AObj<'a>,
    /// Ticks per second.
    tr: f64,
    /// BindingIdToReferences by binding guid.
    refs: IndexMap<String, Vec<Json>>,
    /// Possessables by guid.
    poss: IndexMap<String, Json>,
    paths: std::cell::RefCell<IndexMap<String, Option<Vec<String>>>>,
}

impl Seq<'_, '_> {
    fn sec(&self, f: f64) -> f64 {
        r6(f / self.tr)
    }

    fn obj(&self, p: Option<&Json>) -> Result<Props, KindError> {
        Ok(inner_at(self.seq, str_of(p).unwrap_or_default(), self.name)?.props.clone())
    }

    fn chan(&self, c: &Json) -> Result<SeqChannel, KindError> {
        let constant = |k: &str| str_or(c.get(k), "RCCE_Constant") == "RCCE_Constant";
        if !constant("PreInfinityExtrap") || !constant("PostInfinityExtrap") {
            return Err(fail(format!("{}: channel extrapolation", self.name)));
        }
        let values = arr(c.get("Values"));
        let mut keys = Vec::new();
        for (i, t) in arr(c.get("Times")).iter().enumerate() {
            let v = values.get(i);
            let im = match v.and_then(|v| v.get("InterpMode")) {
                None | Some(Json::Null) => Some(0.0),
                Some(x) => x.as_f64(),
            };
            let interp = im.filter(|m| m.fract() == 0.0 && *m >= 0.0).and_then(|m| INTERP.get(m as usize));
            let tan = v.and_then(|v| v.get("Tangent"));
            let weight = tan.and_then(|t| t.get("TangentWeightMode"));
            let Some(interp) = interp.filter(|_| weight.is_none_or(|w| *w == Json::Num(0.0))) else {
                return Err(fail(format!("{}: key mode", self.name)));
            };
            keys.push(SeqKey {
                t: self.sec(num_or(t.get("Value"), f64::NAN)),
                v: r6(num_or(v.and_then(|v| v.get("Value")), 0.0)),
                i: (*interp).to_owned(),
                at: r6(num_or(tan.and_then(|t| t.get("ArriveTangent")), 0.0) * self.tr),
                lt: r6(num_or(tan.and_then(|t| t.get("LeaveTangent")), 0.0) * self.tr),
            });
        }
        let default = truthy(c.get("bHasDefaultValue")).then(|| r6(num_or(c.get("DefaultValue"), 0.0)));
        Ok(SeqChannel { keys, default })
    }

    fn bool_chan(&self, c: Option<&Json>) -> BoolChannel {
        let values = arr(dig(c, &["Values"]));
        let keys = arr(dig(c, &["Times"]))
            .iter()
            .enumerate()
            .map(|(i, t)| BoolKey {
                t: self.sec(num_or(t.get("Value"), f64::NAN)),
                v: values.get(i) == Some(&Json::Bool(true)),
            })
            .collect();
        let default =
            truthy(dig(c, &["bHasDefaultValue"])).then(|| dig(c, &["DefaultValue"]) == Some(&Json::Bool(true)));
        BoolChannel { keys, default }
    }

    /// A section's range: ERangeBoundTypes Exclusive 0, Inclusive 1, Open 2 (None).
    fn range(&self, s: &Props) -> Range {
        let Some(r) = dig(s.get("SectionRange"), &["Value"]).filter(|r| truthy(Some(r))) else { return [None, None] };
        let b = |k: &str| {
            let bound = r.get(k);
            if dig(bound, &["Type"]) == Some(&Json::Num(2.0)) {
                None
            } else {
                Some(self.sec(num_or(dig(bound, &["Value", "Value"]), f64::NAN)))
            }
        };
        [b("LowerBound"), b("UpperBound")]
    }

    fn ease(&self, s: &Props) -> Result<(), KindError> {
        for (k, v) in props_of(s.get("Easing")) {
            if k != "EaseIn" && k != "EaseOut" && truthy(Some(&v)) {
                return Err(fail(format!("{}: section easing {k}", self.name)));
            }
        }
        Ok(())
    }

    /// A binding's object paths: "<level>:<actor>" from the binding references, "/<component>" for a child
    /// possessable.
    fn path_of(&self, guid: &str) -> Result<Option<Vec<String>>, KindError> {
        if let Some(p) = self.paths.borrow().get(guid) {
            return Ok(p.clone());
        }
        let p = self.poss.get(guid);
        let mut out = Vec::new();
        for x in self.refs.get(guid).map(Vec::as_slice).unwrap_or_default() {
            if let Some(ext) = str_of(x.get("ExternalObjectPath")).filter(|e| !e.is_empty()) {
                let slash = ext.rfind('/').unwrap_or(0);
                let colon = ext[slash..].find(':').map(|c| slash + c);
                let (head, rest) = match colon {
                    Some(c) => (&ext[..c], &ext[c + 1..]),
                    None => (&ext[..ext.len().saturating_sub(1)], ext),
                };
                let rest = rest.strip_prefix("PersistentLevel.").unwrap_or(rest);
                out.push(format!("{}:{rest}", short_of(head)));
                continue;
            }
            let parent = p
                .and_then(|p| str_of(p.get("ParentGuid")))
                .filter(|g| *g != ZERO_GUID)
                .map(|g| self.path_of(g))
                .transpose()?
                .flatten();
            let Some(parent) = parent else {
                let who = p.and_then(|p| str_of(p.get("Name"))).unwrap_or(guid);
                return Err(fail(format!("{}: binding reference of {who}", self.name)));
            };
            let obj = str_of(x.get("ObjectPath")).unwrap_or("undefined");
            out.extend(parent.iter().map(|q| format!("{q}/{obj}")));
        }
        let r = (!out.is_empty()).then_some(out);
        self.paths.borrow_mut().insert(guid.to_owned(), r.clone());
        Ok(r)
    }

    fn track(&self, t: &AObj<'_>, binding: Option<&str>) -> Result<SeqTrack, KindError> {
        let c = class_of(t);
        let ty = c.strip_prefix("MovieScene").unwrap_or(c);
        let ty = ty.strip_suffix("Track").unwrap_or(ty);
        let p = &t.props;
        let list = ["Sections", "CameraShakeSections", "ParticleSections"]
            .iter()
            .find_map(|k| p.get(*k).filter(|v| truthy(Some(v))))
            .map(|v| arr(Some(v)))
            .unwrap_or_default();
        let secs = list.iter().map(|s| self.obj(Some(s))).collect::<Result<Vec<_>, _>>()?;
        let bound = binding.map(|b| self.path_of(b)).transpose()?.flatten();
        let name = binding.and_then(|b| self.poss.get(b)).and_then(|p| str_of(p.get("Name"))).map(str::to_owned);
        for s in &secs {
            self.ease(s)?;
        }
        let binding = bound;
        Ok(match ty {
            "3DTransform" => {
                if secs.len() != 1 {
                    return Err(fail(format!("{}: transform sections", self.name)));
                }
                let sp = &secs[0];
                let mask = num_or(dig(sp.get("TransformMask"), &["Mask"]), 511.0) as u32;
                let mut channels = Vec::new();
                for (i, n) in TRANSFORM_CHANNELS.iter().enumerate() {
                    // a masked-out channel leaves the object's own value
                    channels.push(match sp.get(*n) {
                        Some(c) if (mask >> i) & 1 != 0 && truthy(Some(c)) => Some(self.chan(c)?),
                        _ => None,
                    });
                }
                SeqTrack::Transform { binding, name, range: self.range(sp), mask, channels }
            }
            "ComponentMaterial" => {
                let mut sections = Vec::new();
                for sp in &secs {
                    let any = |k: &str| !arr(sp.get(k)).is_empty();
                    if any("VectorParameterNamesAndCurves")
                        || any("ColorParameterNamesAndCurves")
                        || any("BoolParameterNamesAndCurves")
                    {
                        return Err(fail(format!("{}: material section kinds", self.name)));
                    }
                    let mut scalars = IndexMap::new();
                    for x in arr(sp.get("ScalarParameterNamesAndCurves")) {
                        let curve = x.get("ParameterCurve").cloned().unwrap_or_default();
                        let n = str_of(x.get("ParameterName")).unwrap_or("undefined").to_owned();
                        scalars.insert(n, self.chan(&curve)?);
                    }
                    sections.push(MaterialSection { range: self.range(sp), scalars });
                }
                let index = num(p.get("MaterialIndex")).filter(|v| !v.is_nan()).map_or(0, |v| v as u32);
                SeqTrack::ComponentMaterial { binding, name, material_index: index, sections }
            }
            "Particle" => {
                // EParticleKey: 0 Activate, 1 Deactivate, 2 Trigger
                let mut keys = Vec::new();
                for s in &secs {
                    let pk = s.get("ParticleKeys");
                    let values = arr(dig(pk, &["Values"]));
                    for (i, x) in arr(dig(pk, &["Times"])).iter().enumerate() {
                        let k = num(values.get(i))
                            .and_then(|v| ["Activate", "Deactivate", "Trigger"].get(v as usize).copied())
                            .ok_or_else(|| fail(format!("{}: particle key", self.name)))?;
                        keys.push(ParticleKey { t: self.sec(num_or(x.get("Value"), f64::NAN)), key: k.to_owned() });
                    }
                }
                SeqTrack::Particle { binding, name, keys }
            }
            "Visibility" => {
                // Unverified: the bool's sense against the bHidden property it is bound to
                let property = str_of(dig(p.get("PropertyBinding"), &["PropertyName"])).map(str::to_owned);
                let sections = secs
                    .iter()
                    .map(|s| VisibilitySection { range: self.range(s), bools: self.bool_chan(s.get("BoolCurve")) })
                    .collect();
                SeqTrack::Visibility { binding, name, property, sections }
            }
            "CameraShake" => {
                let sections = secs
                    .iter()
                    .map(|s| {
                        let d = s.get("ShakeData");
                        ShakeSection {
                            range: self.range(s),
                            shake: class_name(str_of(dig(d, &["ShakeClass"])).unwrap_or_default()),
                            play_scale: num_or(dig(d, &["PlayScale"]), 1.0),
                            space: tail(str_or(dig(d, &["PlaySpace"]), "ECameraShakePlaySpace::CameraLocal"))
                                .to_owned(),
                        }
                    })
                    .collect();
                SeqTrack::CameraShake { binding, name, sections }
            }
            "CameraCut" => {
                let mut sections = Vec::new();
                for s in &secs {
                    let g = str_of(dig(s.get("CameraBindingID"), &["Guid"])).unwrap_or_default();
                    sections.push(CutSection { range: self.range(s), camera: self.path_of(g)? });
                }
                SeqTrack::CameraCut { binding, name, sections }
            }
            "AkAudioEvent" => {
                let sections = secs
                    .iter()
                    .map(|s| AudioSection {
                        range: self.range(s),
                        event: str_of(s.get("EventName")).map(str::to_owned),
                        max_duration: num(s.get("MaxSourceDuration")),
                        stop_at_end: truthy(s.get("StopAtSectionEnd")),
                    })
                    .collect();
                SeqTrack::AkAudioEvent { binding, name, sections }
            }
            "Fade" => {
                let mut sections = Vec::new();
                for s in &secs {
                    let fc = s.get("FadeColor").filter(|c| truthy(Some(c)));
                    let color = match fc {
                        Some(c) => ["R", "G", "B", "A"].map(|k| num_or(c.get(k), 0.0)),
                        None => [0.0, 0.0, 0.0, 1.0],
                    };
                    let curve = s.get("FloatCurve").cloned().unwrap_or_default();
                    sections.push(FadeSection {
                        range: self.range(s),
                        fade: self.chan(&curve)?,
                        color,
                        fade_audio: truthy(s.get("bFadeAudio")),
                    });
                }
                SeqTrack::Fade { binding, name, sections }
            }
            "Event" => {
                // a master event track's events go to the player's event contexts (target: the first parameter is
                // the bound "Target"); keys at one time fire in key order. The calls are the director's (tables.rs
                // DIRECTOR_CALLS).
                let mut keys = Vec::new();
                for s in &secs {
                    let c = s.get("EventChannel");
                    let values = arr(dig(c, &["KeyValues"]));
                    for (i, x) in arr(dig(c, &["KeyTimes"])).iter().enumerate() {
                        let f = str_of(dig(values.get(i), &["Ptrs", "Function"])).unwrap_or_default();
                        let d = director_call(self.name, f)?;
                        keys.push(SceneEvent {
                            t: self.sec(num_or(x.get("Value"), f64::NAN)),
                            call: d.call.to_owned(),
                            args: d.args.iter().map(|a| a.value()).collect(),
                            target: Some(d.target),
                        });
                    }
                }
                let position = tail(str_or(p.get("EventPosition"), "EFireEventsAtPosition::AfterSpawn")).to_owned();
                SeqTrack::Event { binding, name, position, keys }
            }
            _ => return Err(fail(format!("{}: track {}", self.name, t.class()))),
        })
    }
}

/// A level sequence's tracks. Times in seconds (frame / TickResolution), tangents per second. A track's binding is
/// the bound objects' paths: "<level>:<actor>" from the binding references, "/<component>" for a child possessable.
fn sequence(scope: &Scope<'_, '_>, name: &str) -> Result<Sequence, KindError> {
    let (seq, ms) = movie_scene(scope, &asset(&format!("{PM}{name}")))?;
    let rate = |v: Option<&Json>| {
        let d = num(dig(v, &["Denominator"])).filter(|d| *d != 0.0 && !d.is_nan()).unwrap_or(1.0);
        num_or(dig(v, &["Numerator"]), f64::NAN) / d
    };
    let refs = arr(dig(seq.get("BindingReferences"), &["BindingIdToReferences"]))
        .iter()
        .filter_map(|e| {
            let pair = e.as_array()?;
            let k = str_of(pair.first())?.to_owned();
            Some((k, arr(dig(pair.get(1), &["References"])).to_vec()))
        })
        .collect();
    let poss = arr(ms.get("Possessables"))
        .iter()
        .map(|x| (str_of(x.get("Guid")).unwrap_or_default().to_owned(), x.clone()))
        .collect();
    let s = Seq { name, seq: &seq, tr: rate(ms.get("TickResolution")), refs, poss, paths: Default::default() };
    let mut tracks = Vec::new();
    for b in arr(ms.get("ObjectBindings")) {
        let guid = str_of(b.get("ObjectGuid")).unwrap_or_default();
        for t in arr(b.get("Tracks")) {
            let o = inner_at(&seq, str_of(Some(t)).unwrap_or_default(), name)?;
            tracks.push(s.track(&o, Some(guid))?);
        }
    }
    for t in arr(ms.get("MasterTracks")) {
        let o = inner_at(&seq, str_of(Some(t)).unwrap_or_default(), name)?;
        tracks.push(s.track(&o, None)?);
    }
    if let Some(c) = str_of(ms.get("CameraCutTrack")).filter(|c| !c.is_empty()) {
        tracks.push(s.track(&*inner_at(&seq, c, name)?, None)?);
    }
    let pr = dig(ms.get("PlaybackRange"), &["Value"]);
    let frame = |b: &str| num_or(dig(pr, &[b, "Value", "Value"]), f64::NAN);
    Ok(Sequence {
        start: s.sec(frame("LowerBound")),
        end: s.sec(frame("UpperBound")),
        display_rate: rate(ms.get("DisplayRate")),
        director: str_of(seq.get("DirectorClass")).filter(|d| !d.is_empty()).map(class_name),
        tracks,
    })
}

// ---------------------------------------------------------------- the level

/// A piece's vector members, rounded.
fn place(q: &Props) -> Result<(V3, V3, V3), KindError> {
    Ok((
        vec6(q.get("RelativeLocation"), Some([0.0; 3]))?,
        rot6(q.get("RelativeRotation")),
        vec6(q.get("RelativeScale3D"), Some([1.0; 3]))?,
    ))
}

/// The materials of one table, each bound once by its short name.
struct Materials<'s, 'c, 'a> {
    scope: &'s Scope<'c, 'a>,
    how: Draw,
    out: IndexMap<String, SceneMaterial>,
}

impl Materials<'_, '_, '_> {
    fn add(&mut self, r: &str) -> Result<String, KindError> {
        let n = short_of(r).to_owned();
        if !self.out.contains_key(&n) {
            let m = scene_material(self.scope.cx, r, self.how)?;
            self.out.insert(n.clone(), m);
        }
        Ok(n)
    }
}

/// The PreMatch entry's data and the particle systems its emitters name.
pub fn bind_pre_match(scope: &Scope<'_, '_>) -> Result<(PreMatchScene, Vec<String>), KindError> {
    let pre = Level::load(scope, PREMATCH_WORLD)?;
    let owner_of = Level::owner_of;

    // ---- anchors, previews, banner movers ----
    let mut anchors = IndexMap::new();
    for a in pre.actors("TargetPoint") {
        let n = Level::actor_of(&a);
        let q = pre.comp(&n, "SceneComp").map(|c| c.props.clone());
        let Some(q) = q.filter(|q| !truthy(q.get("RelativeScale3D")) && !truthy(q.get("AttachParent"))) else {
            return Err(fail(format!("PreMatch: anchor {n}")));
        };
        anchors.insert(
            n,
            Point { loc: vec6(q.get("RelativeLocation"), Some([0.0; 3]))?, rot: rot6(q.get("RelativeRotation")) },
        );
    }
    let mut movers = IndexMap::new();
    for a in pre.actors("Actor").into_iter().filter(|x| Level::actor_of(x).starts_with("BannerMover")) {
        let n = Level::actor_of(&a);
        let q = pre.comp(&n, "DefaultSceneRoot").ok_or_else(|| fail(format!("PreMatch: mover {n}")))?;
        if truthy(q.get("RelativeRotation")) || truthy(q.get("RelativeScale3D")) {
            return Err(fail(format!("PreMatch: mover {n}")));
        }
        let parent = owner_of(str_of(q.get("AttachParent")).unwrap_or_default());
        movers.insert(n, Mover { parent, loc: vec6(q.get("RelativeLocation"), Some([0.0; 3]))? });
    }
    let (blob_shadow, shadow_tpl) = blob_shadow(scope)?;
    let mut previews = Vec::new();
    for a in pre.actors("BP_CharacterPreview_C") {
        let n = Level::actor_of(&a);
        let q = pre.comp(&n, "DefaultSceneRoot").ok_or_else(|| fail(format!("PreMatch: preview {n}")))?;
        let tags: Vec<String> = arr(a.get("Tags")).iter().filter_map(|t| str_of(Some(t))).map(str::to_owned).collect();
        let tag = tags.iter().find(|t| is_player_tag(t)).cloned();
        let placed = ["RelativeLocation", "RelativeRotation", "RelativeScale3D"].iter().any(|k| truthy(q.get(k)));
        let Some(tag) = tag.filter(|_| !placed) else { return Err(fail(format!("PreMatch: preview {n}"))) };
        // BlobShadowCenter: the level's values (the construction script's place and bHiddenInGame) over the template
        let shadow = shadow_place(&pre, &n, &shadow_tpl)?;
        previews.push(PreMatchPreview {
            shadow,
            tag,
            anchor: owner_of(str_of(q.get("AttachParent")).unwrap_or_default()),
            actor: n,
            facing_left: truthy(a.get("FacingLeft")),
            tags,
        });
    }
    // (code-unit order: no collation)
    previews.sort_by(|a, b| a.tag.cmp(&b.tag));
    if previews.iter().map(|p| p.tag.as_str()).collect::<Vec<_>>() != ["Player1", "Player2", "Player3", "Player4"] {
        return Err(fail("PreMatch: preview tags"));
    }

    // ---- banners: BP_Banner_PM's component templates under each instance's level values ----
    let bj = scope.load(BANNER_CLASS)?;
    let bcdo = scope.load(&cdo_of(BANNER_CLASS))?;
    // (Base Element Root and Parallax Rail > Parallax Element Root: where a banner element is attached)
    const PIECES: [&str; 14] = [
        "Banner Placement",
        "Banner Mask",
        "Banner Default Background",
        "Banner Nameplate",
        "Banner Trim Mask",
        "Banner Filter",
        "Banner Highlight",
        "Nametag Root",
        "NametagWidget",
        "CheckMarkPos",
        "PerkInfoPos",
        "Base Element Root",
        "Parallax Rail",
        "Parallax Element Root",
    ];
    let mut ban = Materials { scope, how: Draw::Banner, out: IndexMap::new() };
    let mut banners = Vec::new();
    for a in pre.actors("BP_Banner_PM_C") {
        let an = Level::actor_of(&a);
        let p = over_props(Some(&bcdo.props), Some(&a.props));
        let root = pre.comp(&an, "DefaultSceneRoot").ok_or_else(|| fail(format!("{an}: no DefaultSceneRoot")))?;
        if ["RelativeLocation", "RelativeRotation", "RelativeScale3D"].iter().any(|k| truthy(root.get(k))) {
            return Err(fail(format!("{an}: root transform")));
        }
        let mut pieces = IndexMap::new();
        for n in PIECES {
            let inst = pre.comp(&an, n).ok_or_else(|| fail(format!("{an}: no {n}")))?;
            let q = over_props(Some(&class_inner(&bj, &format!("{n}_GEN_VARIABLE"))?.props), Some(&inst.props));
            let ap = str_of(q.get("AttachParent")).filter(|s| !s.is_empty());
            let Some(parent) = ap.filter(|ap| owner_of(ap) == an).map(|ap| short_of(ap).to_owned()) else {
                return Err(fail(format!("{an}: {n} parent")));
            };
            let (loc, rot, scale) = place(&q)?;
            let mut o = PieceAsset {
                parent,
                loc,
                rot,
                scale,
                hidden_in_game: truthy(q.get("bHiddenInGame")).then_some(true),
                visible: (is_false(q.get("bVisible"))).then_some(false),
                material: None,
                set: None,
                custom_depth: None,
                depth_pass: None,
                sort: None,
                widget: None,
                space: None,
                draw_size: None,
                draw_at_desired_size: None,
                pivot: None,
            };
            match class_of(&inst) {
                "StaticMeshComponent" => {
                    let mats = arr(q.get("OverrideMaterials"));
                    if !is_plane(&q) || mats.len() != 1 {
                        return Err(fail(format!("{an}: {n} mesh")));
                    }
                    let m = str_of(mats.first()).ok_or_else(|| fail(format!("{an}: {n} material")))?;
                    let mo = if m.starts_with(&format!("{}:", pre.world.path())) {
                        Some(inner_at(&pre.world, m, "banner material")?)
                    } else {
                        None
                    };
                    let mut path = m.to_owned();
                    match mo.filter(|mo| class_of(mo) == "MaterialInstanceDynamic") {
                        Some(mid) => {
                            // a dynamic instance the construction script made and the level kept: its parent and its
                            // scalar values
                            if !arr(mid.get("VectorParameterValues")).is_empty()
                                || !arr(mid.get("TextureParameterValues")).is_empty()
                            {
                                return Err(fail(format!("{an}: {n} MID values")));
                            }
                            let parent =
                                str_of(mid.get("Parent")).ok_or_else(|| fail(format!("{an}: {n} MID parent")))?;
                            o.material = Some(ban.add(parent)?);
                            path = parent.to_owned();
                            let mut set = IndexMap::new();
                            for x in arr(mid.get("ScalarParameterValues")) {
                                let k = str_of(dig(Some(x), &["ParameterInfo", "Name"])).unwrap_or("undefined");
                                let v = num(x.get("ParameterValue")).ok_or_else(|| fail(format!("{an}: {n} {k}")))?;
                                set.insert(k.to_owned(), v);
                            }
                            o.set = (!set.is_empty()).then_some(set);
                        }
                        None => o.material = Some(ban.add(m)?),
                    }
                    if truthy(q.get("bRenderCustomDepth")) {
                        o.custom_depth = Some(CustomDepth {
                            stencil: num_or(q.get("CustomDepthStencilValue"), 0.0) as i32,
                            main_pass: !is_false(q.get("bRenderInMainPass")),
                            draw: draw_of(scope.cx, &path, Draw::CustomDepth)?,
                        });
                    }
                    if is_false(q.get("bRenderInDepthPass")) {
                        o.depth_pass = Some(false);
                    }
                    if truthy(q.get("TranslucencySortPriority")) {
                        o.sort = Some(sort_of(&q));
                    }
                }
                "WidgetComponent" => {
                    let pair = |k: &str, d: [f64; 2]| match q.get(k).filter(|v| truthy(Some(v))) {
                        Some(v) => [num_or(v.get("X"), 0.0), num_or(v.get("Y"), 0.0)],
                        None => d,
                    };
                    o.widget = Some(class_name(str_of(q.get("WidgetClass")).unwrap_or_default()));
                    o.space = Some(tail(str_or(q.get("Space"), "EWidgetSpace::World")).to_owned());
                    o.draw_size = Some(pair("DrawSize", [500.0, 500.0]));
                    o.draw_at_desired_size = Some(truthy(q.get("bDrawAtDesiredSize")));
                    o.pivot = Some(pair("Pivot", [0.5, 0.5]));
                }
                "SceneComponent" => {}
                ic => return Err(fail(format!("{an}: {n} type {ic}"))),
            }
            pieces.insert(n.to_owned(), o);
        }
        let mover = owner_of(str_of(root.get("AttachParent")).unwrap_or_default());
        if !movers.contains_key(&mover) {
            return Err(fail(format!("{an}: parent {mover}")));
        }
        banners.push(BannerInstance {
            name: an.clone(),
            mover,
            hidden: truthy(p.get("bHidden")),
            index: num(p.get("Banner Index")).ok_or_else(|| fail(format!("{an}: Banner Index")))? as i32,
            offset: vec6(p.get("Offset"), None)?,
            text_offset: vec6(p.get("TextOffset"), None)?,
            start_hidden: truthy(p.get("Start Hidden")),
            pieces,
        });
    }
    if banners.len() != 7 {
        return Err(fail(format!("PreMatch: banners {}", banners.len())));
    }

    // ---- PM_NewSlants: every Plane, the instances the Move update swaps in, the two timelines ----
    let sl = Level::load(scope, NEW_SLANTS_WORLD)?;
    let mut smat = Materials { scope, how: Draw::Plane, out: IndexMap::new() };
    let mut planes = Vec::new();
    for c in inner_list(&sl.world).into_iter().filter(|x| class_of(x) == "StaticMeshComponent") {
        let q = &c.props;
        let mats = arr(q.get("OverrideMaterials"));
        if !is_plane(q) || mats.len() != 1 {
            return Err(fail(format!("PM_NewSlants: {}", c.path())));
        }
        let (loc, rot, scale) = place(q)?;
        planes.push(SlantPlane {
            name: Level::actor_of(&c),
            parent: str_of(q.get("AttachParent")).filter(|s| !s.is_empty()).map(owner_of),
            loc,
            rot,
            scale,
            visible: !is_false(q.get("bVisible")),
            sort: sort_of(q),
            material: smat.add(str_of(mats.first()).ok_or_else(|| fail(format!("PM_NewSlants: {}", c.path())))?)?,
        });
    }
    for n in [
        "MI_CharSelectSlant",
        "MI_CharSelectSlant_Enemy",
        "MI_CharSelectSlant_AX",
        "MI_CharSelectSlant_AX_Enemy",
        "MI_FFASlant",
        "MI_MainSlant",
    ] {
        smat.add(&asset(&format!("{PM}NewBanners/{n}")))?;
    }
    let sl_pkg = pkg_of(NEW_SLANTS_WORLD);
    let slcdo = scope.load(&format!("{sl_pkg}.Default__PM_NewSlants_C"))?;
    let sl_cls = scope.load(&format!("{sl_pkg}.PM_NewSlants_C"))?;
    let sc = slcdo.get("StartColor");
    let slants = Slants {
        planes,
        materials: smat.out,
        start_color: ["R", "G", "B", "A"].map(|k| num_or(dig(sc, &[k]), 0.0)),
        timelines: IndexMap::from([
            ("Move".to_owned(), timeline(&sl_cls, "Move")?),
            ("Timeline_0".to_owned(), timeline(&sl_cls, "Timeline_0")?),
        ]),
    };

    // ---- MainMenuBackground's Background Lookup Table: map asset path -> picture (a texture of the store) ----
    let bg_cls = scope.load(BG_CLASS)?;
    let bgcdo = scope.load(&cdo_of(BG_CLASS))?;
    let mut map_textures = IndexMap::new();
    let mut missing = Vec::new();
    let mut lookup = IndexMap::new();
    for e in arr(bgcdo.get("Background Lookup Table")) {
        let pair = e.as_array().unwrap_or_default();
        let key = str_of(pair.first()).unwrap_or("undefined").to_owned();
        let Some(value) = str_of(pair.get(1)).filter(|v| !v.is_empty()) else {
            return Err(fail(format!("Background Lookup Table: no picture for {key}")));
        };
        let name = short_of(value).to_owned();
        if !scope.exists(value) {
            missing.push(name);
            lookup.insert(key, None);
            continue;
        }
        if !map_textures.contains_key(&name) {
            let t = scope.load(value)?;
            if t.class() != "/Script/Engine.Texture2D" {
                return Err(fail(format!("map picture: {value} is a {}, not a Texture2D", t.class())));
            }
            let addr = |k: &str| str_of(t.get(k)).map_or("TA_Wrap", tail).to_owned();
            let native = |k: &str| t.native(k);
            map_textures.insert(
                name.clone(),
                MapPicture {
                    path: value.to_owned(),
                    srgb: !is_false(t.get("SRGB")),
                    ax: addr("AddressX"),
                    ay: addr("AddressY"),
                    size: [
                        num(native("SizeX")?.as_ref()).ok_or_else(|| fail(format!("{value}: no SizeX")))?,
                        num(native("SizeY")?.as_ref()).ok_or_else(|| fail(format!("{value}: no SizeY")))?,
                    ],
                    format: str_of(native("PixelFormat")?.as_ref())
                        .ok_or_else(|| fail(format!("{value}: no PixelFormat")))?
                        .to_owned(),
                },
            );
        }
        lookup.insert(key, Some(name));
    }
    let background = Background {
        lookup,
        textures: map_textures,
        missing,
        timelines: IndexMap::from([
            ("TimelineTransition".to_owned(), timeline(&bg_cls, "TimelineTransition")?),
            ("TimelineMapChange".to_owned(), timeline(&bg_cls, "TimelineMapChange")?),
        ]),
    };

    // ---- level sequences, and the level's Emitter actors they bind ----
    let mut sequences = IndexMap::new();
    for n in ["Seq_AnimInRed", "Seq_AnimInFFA_New", "Seq_Clash", "Seq_Clash_FFA_New"] {
        sequences.insert(n.to_owned(), sequence(scope, n)?);
    }
    // each a PreMatch Emitter whose root is its ParticleSystemComponent0, unattached and bAutoActivate false (only
    // the sequences start them): the system, the transform and bVisible. A rotation channel, an instance parameter or
    // any other binding refuses.
    let mut emitters = IndexMap::new();
    let mut systems = IndexSet::new();
    for (sn, s) in &sequences {
        for tr in &s.tracks {
            let (ty, binding) = match tr {
                SeqTrack::Particle { binding, .. } => ("Particle", binding),
                SeqTrack::Transform { binding, channels, .. } => {
                    if channels[3..6].iter().any(Option::is_some) {
                        return Err(fail(format!("{sn}: rotation channel of {binding:?}")));
                    }
                    ("3DTransform", binding)
                }
                _ => continue,
            };
            for b in binding.iter().flatten() {
                let mut parts = b.split(':');
                let (lvl, actor) = (parts.next().unwrap_or_default(), parts.next().unwrap_or("undefined"));
                if lvl != "PreMatch" || actor.contains('/') {
                    return Err(fail(format!("{sn}: {ty} binding {b}")));
                }
                if emitters.contains_key(actor) {
                    continue;
                }
                let a = pre.actor(actor);
                let q = pre.comp(actor, "ParticleSystemComponent0").map(|c| c.props.clone());
                let rooted = a.as_ref().is_some_and(|a| {
                    str_of(a.get("RootComponent"))
                        .is_some_and(|r| r.ends_with(&format!(".{actor}.ParticleSystemComponent0")))
                });
                let (Some(a), Some(q)) = (a, q) else { return Err(fail(format!("{sn}: emitter {actor}"))) };
                if class_of(&a) != "Emitter" || !rooted {
                    return Err(fail(format!("{sn}: emitter {actor}")));
                }
                if truthy(q.get("AttachParent"))
                    || !is_false(q.get("bAutoActivate"))
                    || !arr(q.get("InstanceParameters")).is_empty()
                {
                    return Err(fail(format!("{sn}: emitter {actor} settings")));
                }
                let template =
                    str_of(q.get("Template")).ok_or_else(|| fail(format!("{sn}: emitter {actor} system")))?;
                systems.insert(template.to_owned());
                let (loc, rot, scale) = place(&q)?;
                emitters.insert(
                    actor.to_owned(),
                    LevelEmitter {
                        system: short_of(template).to_owned(),
                        loc,
                        rot,
                        scale,
                        visible: !is_false(q.get("bVisible")),
                    },
                );
            }
        }
    }

    let pre_cls = scope.load(&format!("{}.PreMatch_C", pkg_of(PREMATCH_WORLD)))?;
    let scene = PreMatchScene {
        anchors,
        previews,
        movers,
        banners,
        banner_materials: ban.out,
        timelines: IndexMap::from([
            ("MMToCharSelect".to_owned(), timeline(&pre_cls, "MMToCharSelect")?),
            ("AdjustPreviewActor".to_owned(), timeline(&pre_cls, "AdjustPreviewActor")?),
            ("CameraLerp".to_owned(), timeline(&pre_cls, "CameraLerp")?),
        ]),
        curves: IndexMap::from([
            ("CRV_SCurve".to_owned(), curve_asset(scope, &format!("{PM}CRV_SCurve"))?),
            ("CRV_Exp".to_owned(), curve_asset(scope, &format!("{PM}CRV_Exp"))?),
        ]),
        slants,
        background,
        sequences,
        emitters,
        blob_shadow,
    };
    Ok((scene, systems.into_iter().collect()))
}

/// `/^Player[1-4]$/`.
fn is_player_tag(t: &str) -> bool {
    t.len() == 7 && t.starts_with("Player") && (b'1'..=b'4').contains(&t.as_bytes()[6])
}

// ---------------------------------------------------------------- previews

/// The six ScaleOverrides keys the pre-match reads (LerpSizing goes MainMenu -> CharSelect -> FaceOff).
const PLACEMENTS: [&str; 6] =
    ["MainMenu_You", "MainMenu_Friend", "CharSelect_You", "CharSelect_Friend", "FaceOff_You", "FaceOff_Friend"];

/// One preview the kind writes: its key, its skin's SkinData name ("" the default skin) and object path.
pub struct PreviewJob {
    pub key: String,
    pub skin: String,
    pub path: String,
}

/// Every preview the kind writes: per roster key (CHARACTERS) its default skin and each further skin the character's
/// data builds, by its SkinData name.
pub fn preview_list(scope: &Scope<'_, '_>) -> Result<Vec<PreviewJob>, KindError> {
    let mut out = Vec::new();
    for key in CHARACTERS {
        let c = character_skins(scope, key)?;
        out.push(PreviewJob { key: (*key).to_owned(), skin: String::new(), path: c.skin });
        for s in c.further {
            out.push(PreviewJob { key: (*key).to_owned(), skin: asset_name(&s).to_owned(), path: s });
        }
    }
    Ok(out)
}

/// A skin's preview: placements (each the actor's relative location, Y negated by the reader when FacingLeft, and
/// the mesh's scale per axis: Wonder Woman's are (-s, s, s)), its PreviewIdle and its rig's spot lights in the rig's
/// own space (origin 0, unmirrored: the reader places them at the preview actor and mirrors the lights under Lights
/// by `mirror_when`). A skin with no PreviewIdle (Batman C004_S05): None, nothing plays (BP_CharacterPreview @5999 /
/// @6237 / @6327); a default skin must have one. HomescreenIdle, the montage @5999 picks in the main menu, the same:
/// the build has no fallback to PreviewIdle there.
pub fn bind_preview(
    scope: &Scope<'_, '_>,
    key: &str,
    skin_path: &str,
    is_default: bool,
) -> Result<PreviewEntry, KindError> {
    let skin = scope.load(skin_path)?;
    let mut placements = IndexMap::new();
    for k in PLACEMENTS {
        let Some(ov) = scale_override(&skin, k) else {
            return Err(fail(format!("prematch: {key} has no ScaleOverrides {k}")));
        };
        let r = ov.get("Rotation").filter(|r| truthy(Some(r)));
        if r.is_some_and(|r| ["X", "Y", "Z"].iter().any(|c| num_or(r.get(c), 0.0).abs() > 1e-6)) {
            return Err(fail(format!("prematch: {key} {k} rotation")));
        }
        placements.insert(
            k.to_owned(),
            Placement {
                loc: vec6(ov.get("Translation"), Some([0.0; 3]))?,
                scale: vec6(ov.get("Scale3D"), Some([1.0; 3]))?,
            },
        );
    }
    let idle = str_of(skin.get("PreviewIdle")).filter(|s| !s.is_empty());
    let home = str_of(skin.get("HomescreenIdle")).filter(|s| !s.is_empty());
    for (what, m) in [("PreviewIdle", idle), ("HomescreenIdle", home)] {
        if m.is_none() && is_default {
            return Err(fail(format!("prematch: {key}'s default skin has no {what}")));
        }
    }
    let with_blend = MontageOpts { with_blend: true, ..MontageOpts::default() };
    let rig = str_of(skin.get("CharacterLighting")).ok_or_else(|| fail(format!("prematch: {key} has no rig")))?;
    Ok(PreviewEntry {
        skin: asset_name(skin_path).to_owned(),
        placements,
        preview_idle: idle.map(|i| montage(scope, i, with_blend)).transpose()?,
        homescreen_idle: home.map(|i| montage(scope, i, with_blend)).transpose()?,
        rig: class_name(rig),
        mirror_when: rig_mirror_when(scope, rig)?,
        lights: rig_lights(scope, rig)?,
    })
}
