//! The main menu's scene: the sublevels streamed with New_MainMenu: PreMatch (camera target, CineCamera lens),
//! PM_BG_TrophyRoom (the MainMenuBackground plane), PM_NewSlants (two planes), and BP_CharacterPreview's glow shell.
//! The Player1 preview actor is the PreMatch entry's; what its fighter takes from a skin (the MainMenu_You placement,
//! HomescreenIdle, the rig) is that skin's preview entry's. The
//! background's Event Lookup Table is read here only (UI/specs/54).

use indexmap::IndexMap;
use mvs_material::menuscene::{
    Glow, GlowKey, GlowTimeline, MainMenuScene, MenuCamera, ParamTrack, SceneMaterial, ScenePlane,
};

use super::material::{Draw, parameter_names, scene_material};
use super::preview::props_of;
use super::util::{
    AObj, Level, Props, Scope, arr, cdo_of, class_inner, fail, inner_at, num, num_or, over_props, rot, str_of, truthy,
    vec,
};
use crate::json::Json;
use crate::kind::KindError;

pub const PREMATCH_WORLD: &str = "/Game/Panda_Main/PreMatch/PreMatch.PreMatch";
pub const NEW_SLANTS_WORLD: &str = "/Game/Panda_Main/PreMatch/Maps/PM_NewSlants.PM_NewSlants";
const TROPHY_ROOM_WORLD: &str = "/Game/Panda_Main/PreMatch/Maps/PM_BG_TrophyRoom.PM_BG_TrophyRoom";
/// The background plane's actor class (MainMenuBackground_C).
pub const BG_CLASS: &str = "/Game/Panda_Main/UI/MainMenu/MainMenuBackground.MainMenuBackground_C";
pub const PREVIEW_CLASS: &str =
    "/Game/Panda_Main/UI/MainMenu/CharacterViewer/BP_CharacterPreview.BP_CharacterPreview_C";
const DISSOLVE_MAT: &str = "/Game/Panda_Main/VFX/Materials/CharacterDissolve_MAT.CharacterDissolve_MAT";
/// The engine Plane, by build object path (the mesh kind's).
pub const PLANE: &str = "/Engine/BasicShapes/Plane.Plane";

/// A camera's lens values (CineCameraComponent): field of view, aspect, the vignette it overrides.
pub fn lens(q: &Props, what: &str) -> Result<(f64, f64, Option<f64>), KindError> {
    let pp = props_of(q.get("PostProcessSettings"));
    let vignette = if truthy(pp.get("bOverride_VignetteIntensity")) { num(pp.get("VignetteIntensity")) } else { None };
    let n = |k: &str| num(q.get(k)).ok_or_else(|| fail(format!("{what}: no {k}")));
    Ok((n("FieldOfView")?, n("AspectRatio")?, vignette))
}

/// A Plane component's mesh, checked.
pub fn is_plane(q: &Props) -> bool {
    str_of(q.get("StaticMesh")) == Some(PLANE)
}

/// A plane's first override material.
pub fn material_of(q: &Props) -> Result<&str, KindError> {
    str_of(arr(q.get("OverrideMaterials")).first()).ok_or_else(|| fail("a plane with no override material"))
}

/// The glow's timeline: a timeline template's float tracks as the Dissolve / Reverse Dissolve parameters.
fn glow_timeline(cls: &AObj<'_>, name: &str) -> Result<GlowTimeline, KindError> {
    let tl = class_inner(cls, name)?;
    let param = |t: &str| match t {
        "Dissolve" => Some("Dissolve"),
        "Fresnel" => Some("Fresnel_Base"),
        "Emissive" => Some("Emissive_Mult"),
        _ => None,
    };
    let mut tracks = Vec::new();
    for t in arr(tl.get("FloatTracks")) {
        let tn = str_of(t.get("TrackName")).unwrap_or_default();
        let Some(p) = param(tn).filter(|_| !truthy(t.get("bIsExternalCurve"))) else {
            return Err(fail(format!("Dissolve timeline: track {tn}")));
        };
        let curve_obj = inner_at(cls, str_of(t.get("CurveFloat")).unwrap_or_default(), "Dissolve curve")?;
        let curve = props_of(curve_obj.get("FloatCurve"));
        for x in ["PreInfinityExtrap", "PostInfinityExtrap"] {
            if let Some(x) = str_of(curve.get(x))
                && !x.is_empty()
                && x != "RCCE_Constant"
            {
                return Err(fail(format!("Dissolve timeline: extrapolation on {tn}")));
            }
        }
        let mut keys = Vec::new();
        for k in arr(curve.get("Keys")) {
            if str_of(k.get("TangentWeightMode")).unwrap_or("RCTWM_WeightedNone") != "RCTWM_WeightedNone" {
                return Err(fail(format!("Dissolve timeline: weighted key on {tn}")));
            }
            keys.push(GlowKey {
                t: num_or(k.get("Time"), 0.0),
                v: num_or(k.get("Value"), 0.0),
                mode: str_of(k.get("InterpMode")).unwrap_or("RCIM_Linear").replacen("RCIM_", "", 1),
                arrive: num_or(k.get("ArriveTangent"), 0.0),
                leave: num_or(k.get("LeaveTangent"), 0.0),
            });
        }
        tracks.push(ParamTrack { param: p.to_owned(), keys });
    }
    if let Some(m) = str_of(tl.get("LengthMode"))
        && !m.is_empty()
        && m != "ETimelineLengthMode::TL_TimelineLength"
        && m != "TL_TimelineLength"
    {
        return Err(fail(format!("Dissolve timeline: length mode {m}")));
    }
    if truthy(tl.get("bLoop"))
        || truthy(tl.get("bIgnoreTimeDilation"))
        || !arr(tl.get("EventTracks")).is_empty()
        || !arr(tl.get("VectorTracks")).is_empty()
        || !arr(tl.get("LinearColorTracks")).is_empty()
    {
        return Err(fail("Dissolve timeline: unexpected settings"));
    }
    Ok(GlowTimeline { length: num_or(tl.get("TimelineLength"), 5.0), tracks })
}

/// BP_CharacterPreview GlowVFX (@29444): CharacterDissolve_MAT on duplicates of the meshes, the Dissolve
/// timeline's tracks; BPI_ClearFighter's Reverse Dissolve (0.75 s) writes Dissolve and Fresnel_Base only.
fn glow(scope: &Scope<'_, '_>) -> Result<Glow, KindError> {
    let cls = scope.load(PREVIEW_CLASS)?;
    let fwd = glow_timeline(&cls, "Dissolve_Template")?;
    let reverse = glow_timeline(&cls, "Reverse Dissolve_Template")?;
    if reverse.tracks.iter().map(|t| t.param.as_str()).collect::<Vec<_>>() != ["Dissolve", "Fresnel_Base"] {
        return Err(fail("Reverse Dissolve timeline: tracks"));
    }
    let mut material = scene_material(scope.cx, DISSOLVE_MAT, Draw::Skin)?;
    let names = parameter_names(scope.cx, DISSOLVE_MAT)?;
    for t in &fwd.tracks {
        if !names.contains(&t.param) {
            return Err(fail(format!("CharacterDissolve_MAT: no parameter {}", t.param)));
        }
    }
    material.tracks = fwd.tracks;
    Ok(Glow { length: fwd.length, reverse, material })
}

/// A level plane's place and material (`local`).
fn plane(scope: &Scope<'_, '_>, q: &Props, name: Option<&str>, sort: bool) -> Result<ScenePlane, KindError> {
    Ok(ScenePlane {
        name: name.map(str::to_owned),
        loc: vec(q.get("RelativeLocation"), Some([0.0; 3]))?,
        rot: rot(q.get("RelativeRotation")),
        scale: vec(q.get("RelativeScale3D"), Some([1.0; 3]))?,
        sort: sort.then(|| sort_of(q)),
        material: scene_material(scope.cx, material_of(q)?, Draw::Plane)?,
    })
}

/// TranslucencySortPriority (`|| 0`).
pub fn sort_of(q: &Props) -> i32 {
    num(q.get("TranslucencySortPriority")).filter(|v| !v.is_nan()).map_or(0, |v| v as i32)
}

/// MainMenuBackground's Event Lookup Table (the class default's; PM_BG_TrophyRoom's placed actor does not override
/// it): each slug's material, drawn as the background's own is (the engine Plane, `local`).
fn event_backgrounds(scope: &Scope<'_, '_>) -> Result<IndexMap<String, SceneMaterial>, KindError> {
    let cdo = scope.load(&cdo_of(BG_CLASS))?;
    let mut out = IndexMap::new();
    for e in arr(cdo.get("Event Lookup Table")) {
        let pair = e.as_array().unwrap_or_default();
        let (Some(slug), Some(path)) = (str_of(pair.first()), str_of(pair.get(1)).filter(|v| !v.is_empty())) else {
            return Err(fail("Event Lookup Table: an entry without a slug or a material"));
        };
        out.insert(slug.to_owned(), scene_material(scope.cx, path, Draw::Plane)?);
    }
    Ok(out)
}

/// The MainMenu entry's data.
pub fn bind_main_menu(scope: &Scope<'_, '_>) -> Result<MainMenuScene, KindError> {
    let pre = Level::load(scope, PREMATCH_WORLD)?;
    let target = pre.comp_of("CameraTarget_MainMenu", "SceneComponent")?;
    let cine = pre.comp_of("CineCamera", "CineCameraComponent")?;
    let (fov, aspect, vignette) = lens(&cine, "PreMatch CineCamera")?;
    let camera = MenuCamera {
        loc: vec(target.get("RelativeLocation"), None)?,
        rot: rot(target.get("RelativeRotation")),
        fov,
        aspect,
        vignette,
    };

    // the background plane: the level's component over its archetype (the class's template)
    let lvl = Level::load(scope, TROPHY_ROOM_WORLD)?;
    let comp = lvl.comps("MainMenuBackground", "StaticMeshComponent").into_iter().next();
    let tpl = match &comp {
        Some(c) => c.template()?,
        None => None,
    };
    let (Some(comp), Some(tpl)) = (comp, tpl) else { return Err(fail("MainMenuBackground: component / template")) };
    let cprops = over_props(Some(&scope.load(&tpl)?.props), Some(&comp.props));
    if !is_plane(&cprops) {
        return Err(fail(format!("MainMenuBackground mesh {:?}", cprops.get("StaticMesh"))));
    }
    let background = plane(scope, &cprops, None, false)?;
    let events = event_backgrounds(scope)?;

    // PM_NewSlants: the Plane actors with an M_BG_Slant instance whose root is visible and unattached
    // (UI_Plane_MainMenu is bVisible false; UI_Plane_BlueTeam, attached to it, lies outside the menu camera's view)
    let sl = Level::load(scope, NEW_SLANTS_WORLD)?;
    let mut slants = Vec::new();
    for name in ["UI_Plane_FFA", "UI_Plane_RedTeam"] {
        let q = sl.comps(name, "StaticMeshComponent").first().map(|c| c.props.clone());
        let Some(q) = q
            .filter(|q| q.get("bVisible") != Some(&Json::Bool(false)) && !truthy(q.get("AttachParent")) && is_plane(q))
        else {
            return Err(fail(format!("PM_NewSlants: {name}")));
        };
        slants.push(plane(scope, &q, Some(name), true)?);
    }
    let glow = glow(scope)?;
    Ok(MainMenuScene { camera, plane: PLANE.to_owned(), background, events, slants, glow })
}
