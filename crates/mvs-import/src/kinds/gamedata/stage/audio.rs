//! The stage data's sound part: `audio`, the Audio sub-level's emitters and reverb volumes and its level Blueprint's
//! music flow, ported here as constants per level, cited by statement offset.

use mvs_data::stage::{ReverbVolume, StageAudio, StageAudioEmitter, StageMusic};
use mvs_data::{Props, Value};

use super::super::shape::{cdo_path, fail, short_name};
use super::context::{StageCtx, level_path, s, vec, vec0};
use crate::kind::KindError;
use crate::objview::ObjView;
use crate::stage_levels::StageLevel;

/// How an Audio sub-level's level Blueprint draws its skin.
enum Flow {
    /// BeginPlay -> Delay 0.1 -> gameflow map state, BPI_SetMusicMap(<map>) @318, a skin drawn with Array_Random @866
    /// over the level script's MapMusicData (its class default), BPI_SetMusicSkinSwitch, map state intro (3) -> Delay 1.0
    /// -> main_a (1).
    Random,
    /// The same tail, the skin the player saved for the map (PreferencesManager LoadStoredMapMusicAssetData @197, keyed
    /// by the name in `stored`), else MapMusic_Default (@669). The port keeps no saved map music (preferences are
    /// parked), so the skin is MapMusic_Default's.
    Stored,
}

/// The map switch and the skin rule of an Audio sub-level's level Blueprint, by level.
fn audio_level_music(level: &str) -> Option<(&'static str, Flow)> {
    Some(match level {
        "M008/Map_M008_Audio" => ("mus_map_gotthroneroom", Flow::Stored), // @644 (ThroneRoom)
        "M009/Map_M009_Audio" => ("mus_map_cromulon", Flow::Random),      // @318
        "M010/Map_M010_Audio" => ("mus_map_M010", Flow::Stored),          // @642 (SpaceJam)
        "M011/Map_M011_Audio" => ("mus_map_M011", Flow::Random),          // @318
        "Map_Batcave/Map_Batcave_Audio" => ("mus_map_batcave", Flow::Random), // @318
        "Map_TreeHouse/Map_TreeHouse_Audio" => ("mus_map_treehouse", Flow::Random), // @318
        "ScoobyMansion/Map_ScoobyDoo_Audio" | "ScoobyMansion/Map_ScoobyDoo_Audio_NoRoof" => {
            ("mus_map_scoobydoomansion", Flow::Stored) // @648 (Scooby Mansion)
        }
        "TrophyRoom/Map_TrophyRoom_Audio" => ("mus_map_medley", Flow::Stored), // @645 (Trophy Room)
        _ => return None,
    })
}

/// The map switch of a gameplay level with no Audio sub-level, its own level Blueprint's (the Lab's flow, BeginPlay ->
/// Delay 0.1 -> gameflow map state, BPI_SetMusicMap, BPI_GetMusicSkin / BPI_SetMusicSkin (the game instance's Mus_Skin, 0
/// without the interface), map state intro (3) -> Delay 1.0 -> main_a (1); no MapMusicData, so no skin is drawn).
fn level_music(level: &str) -> Option<&'static str> {
    Some(match level {
        "TrainingRoomSmall/Map_TrainingRoomSmall" => "mus_map_lionwatch", // @358
        "TrainingRoomLarge/Map_TrainingRoomLarge" => "mus_map_lionwatch", // @182
        "Map_Classic3Platform/Map_Classic3Platform_Large" => "mus_map_lionwatch", // @182
        "Map_Classic3Platform/Map_Classic3Platform_Small" => "mus_map_lionwatch", // @358
        _ => return None,
    })
}

/// The music the runtime plays for a stage with no `audio` block: the Lab's map switch and the game instance's Mus_Skin
/// left at 0 (unstored on Default__PandaGameInstance_C; SetMusicSkin @122 posts mus_skin-mus_base for it). A level
/// whose own flow is this one gets no block (port-made).
const FALLBACK_MAP: &str = "mus_map_lionwatch";
const FALLBACK_SKIN: &str = "mus_base";

const MAP_MUSIC_DEFAULT: &str = "/Game/Panda_Main/Blueprints/Rewards/MapMusic/MapMusic_Default.MapMusic_Default";

/// A MapMusic asset's skin: its MusicSkinSwitch, `mus_skin-<skin>`.
fn skin_of(ctx: &StageCtx<'_>, path: &str) -> Result<String, KindError> {
    let o = ctx.load(path)?;
    let p = ctx.properties(&o)?;
    let skin = s(&p, "MusicSkinSwitch")
        .and_then(|sw| short_name(sw).strip_prefix("mus_skin-"))
        .filter(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    match skin {
        Some(k) => Ok(k.to_owned()),
        None => fail(format!("{path}: MusicSkinSwitch {:?}", p.get("MusicSkinSwitch"))),
    }
}

/// An Env_Audio_Emit actor as the stage data's emitter (Env_Audio_Emit: BeginPlay posts AkEvent on its AkComponent unless
/// IsManualStart, @240; ManualStart posts it, @10..235), or none for one that posts nothing. A child actor (a placed
/// actor's ChildActorComponent spawns it; its AkComponent is attached to that component) names its owner (`actor`) and
/// the component (`component`): its owner's graph sets its event or starts it (`manual`), or it posts at BeginPlay as a
/// level's does. `at`: the AkComponent's world location at level start. A level's own emitter that waits for a
/// ManualStart nobody calls stops the bind.
fn emitter_of(ctx: &StageCtx<'_>, a: &ObjView<'_>) -> Result<Option<StageAudioEmitter>, KindError> {
    let ap = ctx.with_templates(a)?;
    let n = short_name(a.path()).to_owned();
    if s(&ap, "RootComponent").map(short_name) != Some("AkAudioComponent0") {
        return fail(format!("{n}: emitter shape"));
    }
    // (an AkComponent the level left at its template's values has no properties: Trophy's bed and birds, at the origin;
    // Env_Audio_Emit's own component template stores none either)
    let Some(ak) = ctx.comps(a, Some("AkComponent")).into_iter().next() else {
        return fail(format!("{n}: no AkComponent"));
    };
    let comp: std::rc::Rc<Props> = ctx.with_templates(&ak)?;
    let odd: Vec<&str> = comp
        .keys()
        .map(String::as_str)
        .filter(|k| !matches!(*k, "RelativeLocation" | "AttenuationScalingFactor" | "AttachParent"))
        .collect();
    if !odd.is_empty() {
        return fail(format!("{n}: AkComponent {}", odd.join(", ")));
    }
    // (the owner and its ChildActorComponent: the attach parent's chain, PersistentLevel.<actor>.<component>)
    let chain: Option<(String, String)> = match comp.get("AttachParent") {
        Some(Value::Str(par)) => {
            const MARK: &str = "PersistentLevel.";
            let Some(i) = par.find(MARK) else { return fail(format!("{n}: AkComponent attached to {par}")) };
            let parts: Vec<&str> = par[i + MARK.len()..].split('.').collect();
            if parts.len() != 2 {
                return fail(format!("{n}: AkComponent attached to {par}"));
            }
            Some((parts[0].to_owned(), parts[1].to_owned()))
        }
        _ => None,
    };
    let manual = super::context::js_truthy(ap.get("IsManualStart"));
    if manual && chain.is_none() {
        return fail(format!("{n}: a level's emitter that waits for a ManualStart"));
    }
    // (no event, the class default None: The Court's amb_map_M010_emit. Lib_Audio's PF_Post_Event_On_AkComponent posts
    // nothing for an invalid event, IsValid(AkEvent) @85-181; a child's event its owner may set)
    let event = if super::context::js_truthy(ap.get("AkEvent")) { s(&ap, "AkEvent") } else { None };
    if !super::context::js_truthy(ap.get("AkEvent")) && chain.is_none() {
        return Ok(None);
    }
    let at = if chain.is_some() { ctx.world(&ak)?.loc } else { vec0(comp.get("RelativeLocation")) };
    // UAkComponent's AttenuationScalingFactor (1 unless the level sets it): Trophy's waterfalls set 2. The runtime does
    // not read it yet; written only where the level sets it.
    let att = match comp.get("AttenuationScalingFactor") {
        None | Some(Value::Null) => 1.0,
        Some(Value::Num(x)) => *x,
        Some(_) => f64::NAN,
    };
    Ok(Some(StageAudioEmitter {
        name: n,
        event: event.map(|e| short_name(e).to_owned()),
        at: at.to_vec(),
        actor: chain.as_ref().map(|c| c.0.clone()),
        component: chain.as_ref().map(|c| c.1.clone()),
        manual: manual.then_some(true),
        attenuation_scale: (att != 1.0).then_some(att),
    }))
}

/// The Audio sub-level's emitters, then the other levels' (the placed actors' child emitters: Cromulons' speakers and
/// helicopters, Batcave's bats and walls, Tree Fort's trampoline), its reverb volumes and its music.
fn audio_of(ctx: &StageCtx<'_>, lv: &StageLevel<'_>) -> Result<StageAudio, KindError> {
    let mut emitters = Vec::new();
    let mut reverb = Vec::new();
    for &i in &lv.actors {
        let a = &lv.objects[i];
        let t = short_name(a.class());
        let n = short_name(a.path());
        if t == "Env_Audio_Emit_C" {
            let em = emitter_of(ctx, a)?;
            if em.as_ref().is_some_and(|e| e.manual.is_some() || e.actor.is_some()) {
                return fail(format!("{n}: an Audio level's emitter with an owner or a ManualStart"));
            }
            emitters.extend(em);
        } else if t == "AkReverbVolume" {
            let late = ctx.comps(a, Some("AkLateReverbComponent")).into_iter().next();
            let brush = ctx.comps(a, Some("BrushComponent")).into_iter().next();
            let (Some(late), Some(brush)) = (late, brush) else { return fail(format!("{n}: reverb volume shape")) };
            let (lp, bp) = (ctx.properties(&late)?, ctx.properties(&brush)?);
            // (not used by the runtime yet:)
            let Some(bus) = s(&lp, "AuxBusName") else { return fail(format!("{n}: no AuxBusName")) };
            reverb.push(ReverbVolume {
                name: n.to_owned(),
                bus: bus.to_owned(),
                loc: vec0(bp.get("RelativeLocation")).to_vec(),
                scale: vec(bp.get("RelativeScale3D"), [1.0; 3]).to_vec(),
            });
        }
    }
    let Some((map, flow)) = audio_level_music(&lv.level) else {
        return fail(format!("{}: an Audio level whose music flow (its level Blueprint) is not ported", lv.level));
    };
    let skins = match flow {
        Flow::Stored => vec![skin_of(ctx, MAP_MUSIC_DEFAULT)?],
        Flow::Random => {
            let base = lv.level.rsplit('/').next().unwrap_or(&lv.level);
            let cdo = ctx.load(&cdo_path(&format!("{}.{base}_C", level_path(&lv.level))))?;
            let cp = ctx.properties(&cdo)?;
            let data = match cp.get("MapMusicData") {
                Some(Value::Arr(d)) if !d.is_empty() => d,
                _ => return fail(format!("{}: no MapMusicData on its level script's class default", lv.level)),
            };
            let mut skins = Vec::new();
            for p in data {
                let Some(p) = p.as_str() else {
                    return fail(format!("{}: a MapMusicData entry that is no path", lv.level));
                };
                skins.push(skin_of(ctx, p)?);
            }
            skins
        }
    };
    emitters.extend(level_emitters(ctx)?);
    Ok(StageAudio { emitters, reverb, music: StageMusic { map: map.to_owned(), skins } })
}

/// The Env_Audio_Emit actors of the gameplay, lighting and scenery levels, in their order.
fn level_emitters(ctx: &StageCtx<'_>) -> Result<Vec<StageAudioEmitter>, KindError> {
    let mut out = Vec::new();
    for a in ctx.actors().into_iter().chain(ctx.scenery_actors()) {
        if short_name(a.class()) != "Env_Audio_Emit_C" {
            continue;
        }
        out.extend(emitter_of(ctx, &a)?);
    }
    Ok(out)
}

/// The audio part (AUDIO_PART.bind).
pub fn bind(ctx: &StageCtx<'_>) -> Result<Option<StageAudio>, KindError> {
    if let Some(lv) = &ctx.levels.audio {
        return Ok(Some(audio_of(ctx, lv)?));
    }
    let level = &ctx.level(0).level;
    let Some(map) = level_music(level) else {
        return fail(format!(
            "{level}: a level with no Audio sub-level whose music flow (its level Blueprint) is not ported"
        ));
    };
    let emitters = level_emitters(ctx)?;
    if map != FALLBACK_MAP || !emitters.is_empty() {
        return Ok(Some(StageAudio {
            emitters,
            reverb: Vec::new(),
            music: StageMusic { map: map.to_owned(), skins: vec![FALLBACK_SKIN.to_owned()] },
        }));
    }
    Ok(None)
}
