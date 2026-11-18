//! Trophy's E.D.G.E.'s scenery part (`trophyroom`): its own Blueprint actors.
//!
//! The centre platform (TROPHY_LV03_PlatformCenter_A_01_Blueprint_C, the large variant's gameplay level; data-only):
//! its StaticMesh drawn where the class's template puts it under the placed Scene root. Its Box and Comp_EdgeTrace are
//! the stage data's. The tutorial's (TROPHY_TUTORIAL_PlatformCenter_Blueprint_C, Map_TrophyRoom_Tutorial; a plain Actor
//! of the same mesh, its own slots): the same, under a Scene root that the instance scales (1, 1.1, 1.1). Its Box is the
//! stage data's block.
//! The crate (`crate`, TROPHY_AcmeCrate_A_01_Actor_C in both Background levels, TROPHY_AcmeCrate_A_01_Actor and
//! AcmeCrate_AnimBP): its skeletal mesh is one of the entry's `skeletal`, written with no animation (its AnimBP is one
//! Slot node, DefaultSlot, over an unlinked source: the reference pose). The block has the Blueprint's random ranges
//! and its two montages as the renderer plays them: length, blend times, the slot track's segments (animation, montage
//! start, animation start / end, rate), the notifies (time in the montage, the sound, or the PlayVFX state's system,
//! socket and end), the montage's own and its segments' animations' (an animation's notify fires at its segment's
//! montage time), and the socket on the mesh's skeleton.

use std::cmp::Ordering;

use indexmap::IndexSet;

use crate::json::Json;
use crate::kind::KindError;
use crate::kinds::scenery::part::{
    JsonProps, SceneryBindCtx, SceneryLevelRun, SceneryMeshOut, SceneryPart, SceneryPartRun, SkeletalOpts,
    StaticMeshOut,
};
use crate::objview::{ObjView, short_name};

use super::util::js::{at, fail, first, is_str, num_or, or_num, put, put_opt, req_arr, triple, truthy};
use super::util::{
    cdo_props, class_inner, comp_of, component_props, is_struct, name_of, number, over_props, props_of, reference,
    rot_of, short_ref, vec,
};

const PART: &str = "trophyroom";

const CRATE: &str = "TROPHY_AcmeCrate_A_01_Actor_C";
const CENTER: &str = "TROPHY_LV03_PlatformCenter_A_01_Blueprint_C";
const TUTORIAL_CENTER: &str = "TROPHY_TUTORIAL_PlatformCenter_Blueprint_C";
// AcmeCrate_AnimBP: AnimGraphNode_Root <- AnimGraphNode_Slot DefaultSlot, its Source unlinked (FPoseLink with no node:
// the reference pose); its ubergraph does nothing.
const ANIM_BP: &str = "AcmeCrate_AnimBP_C";
// UAnimMontage's constructor: BlendIn / BlendOut 0.25 s, BlendOutTriggerTime -1, bEnableAutoBlendOut (UAnimMontage ctor
// 142849770); the two montages store none of them.
const MONTAGE_DEFAULT_BLEND: f64 = 0.25;
// The actor's graph (TROPHY_AcmeCrate_A_01_Actor ExecuteUbergraph @0..1562): BeginPlay SetNextBurstTime (now +
// RandomFloatInRange(10, 30) @1443) and QueueNextAnim; QueueNextAnim (@768) plays the burst when the game time has
// reached NextBurstTime (PlayBurstAnim @961: the burst montage, SetNextBurstTime, QueueNextAnim), else
// Delay(RandomFloatInRange(2, 7) @864), which resumes at @44: the shake montage, then QueueNextAnim. Both
// CreateProxyObjectForPlayMontage at rate 1, from 0.
const SHAKE_DELAY: [f64; 2] = [2.0, 7.0];
const BURST_DELAY: [f64; 2] = [10.0, 30.0];
const SHAKE: &str = "/Game/Panda_Main/Maps/TrophyRoom/Trophies/AcmeCrate/TROPHY_AcmeCrate_A_01_Shake_Anim_Montage.TROPHY_AcmeCrate_A_01_Shake_Anim_Montage";
const BURST: &str = "/Game/Panda_Main/Maps/TrophyRoom/Trophies/AcmeCrate/TROPHY_AcmeCrate_A_01_Burst_Anim_Montage.TROPHY_AcmeCrate_A_01_Burst_Anim_Montage";
const PLAY_VFX: &str = "/Game/Panda_Main/Blueprints/AnimNotifies/AnimNotify_PlayVFX.AnimNotify_PlayVFX_C";

// The tutorial level (Map_TrophyRoom_Tutorial_C): the solids its tutorials spawn at run time (TutorialWall_C,
// TutorialWallShort_C, TutorialPlatform_C), each class's StaticMeshComponent written as
// one hidden instance (`tutorialSolids`: class -> instance name) at the component's relative transform from the actor's
// root, which the renderer's block draws once per live solid at the solid's place. No
// lightmap (a spawned component has no build data); channel 0 and the template's CastShadow (UPrimitiveComponent
// defaults otherwise), Movable.
const TUTORIAL_LEVEL: &str = "Map_TrophyRoom_Tutorial_C";
const TUTORIAL_SOLIDS: [(&str, &str); 3] = [
    ("TutorialWall_C", "/Game/Panda_Main/Blueprints/Tutorial/TutorialWall.TutorialWall_C"),
    ("TutorialWallShort_C", "/Game/Panda_Main/Blueprints/Tutorial/TutorialWallShort.TutorialWallShort_C"),
    ("TutorialPlatform_C", "/Game/Panda_Main/GameMode/Platforms/TutorialPlatform.TutorialPlatform_C"),
];

fn tutorial_solids_of<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<JsonProps, KindError> {
    let mut out = JsonProps::new();
    for (cls, path) in TUTORIAL_SOLIDS {
        let q = props_of(&class_inner(ctx, path, "StaticMesh_GEN_VARIABLE")?);
        let over = arr_of(q.get("OverrideMaterials"));
        let (Some(Json::Str(mesh_path)), Some(over)) = (q.get("StaticMesh"), over) else {
            return fail(PART, format!("{cls}: StaticMesh"));
        };
        if over.len() != 1 || truthy(q.get("RelativeLocation")) {
            return fail(PART, format!("{cls}: StaticMesh"));
        }
        let slots = ctx.mesh_header(mesh_path)?.slots.len();
        if slots != 1 {
            return fail(PART, format!("{cls}: mesh slots"));
        }
        let mesh = ctx.add_mesh(mesh_path)?;
        let material = reference(over.first(), &format!("{cls} material"))?;
        let materials = ctx.build_for(&[Some(material)], None, None)?;
        let name = format!("{cls}.StaticMesh");
        let mut i = JsonProps::new();
        put(&mut i, "name", name.as_str());
        put(&mut i, "mesh", mesh as f64);
        put(&mut i, "loc", triple([0.0; 3]));
        put(&mut i, "rot", triple(rot_of(q.get("RelativeRotation"))));
        put(&mut i, "scale", triple(vec(q.get("RelativeScale3D"), [1.0; 3])));
        put(&mut i, "materials", Json::Arr(materials.into_iter().map(|m| m.map_or(Json::Null, Json::Str)).collect()));
        put(&mut i, "channels", Json::Arr(vec![Json::Bool(true), Json::Bool(false), Json::Bool(false)]));
        put(&mut i, "castShadow", q.get("CastShadow") != Some(&Json::Bool(false)));
        put(&mut i, "mobility", "Movable");
        ctx.add_instance(i);
        put(&mut out, cls, name.as_str());
    }
    Ok(out)
}

fn arr_of(v: Option<&Json>) -> Option<&[Json]> {
    match v {
        Some(Json::Arr(a)) => Some(a),
        _ => None,
    }
}

struct Notify {
    at: f64,
    end: Option<f64>,
    sound: Option<String>,
    system: Option<String>,
    socket: Option<String>,
}

impl Notify {
    fn json(&self) -> Json {
        let mut o = JsonProps::new();
        put(&mut o, "at", self.at);
        if let Some(e) = self.end {
            put(&mut o, "end", e);
        }
        if let Some(s) = &self.sound {
            put(&mut o, "sound", s.as_str());
        }
        if let Some(s) = &self.system {
            put(&mut o, "system", s.as_str());
        }
        if let Some(s) = &self.socket {
            put(&mut o, "socket", s.as_str());
        }
        Json::Obj(o)
    }
}

/// A montage as the renderer plays it (see the header): its entry and the sockets its notifies name, in notify order.
struct Montage {
    json: Json,
    sockets: Vec<String>,
}

fn notify_of<'a>(
    ctx: &dyn SceneryBindCtx<'a>,
    name: &str,
    n: &Json,
    at: f64,
    end: Option<f64>,
    out: &mut Vec<Notify>,
) -> Result<(), KindError> {
    if !is_str(n.get("MontageTickType"), "EMontageNotifyTickType::Queued") {
        return fail(PART, format!("{name}: notify tick type"));
    }
    // (the notify object is an inner object of the asset that holds the notify)
    let r = reference(first(n.get("Notify"), n.get("NotifyStateClass")), "a notify")?;
    let o = props_of(&ctx.load(&r)?);
    if is_str(n.get("NotifyName"), "Ak_OneShot_C") {
        if !truthy(o.get("AkEvent")) || o.len() != 1 {
            return fail(PART, format!("{name}: Ak_OneShot"));
        }
        out.push(Notify { at, end: None, sound: Some(short_ref(o.get("AkEvent"))?), system: None, socket: None });
    } else if is_str(n.get("NotifyName"), "PlayVFX_C") {
        // AnimNotify_PlayVFX: on an actor that is not a fighter or a preview, SpawnVFX SpawnEmitterAttached(
        // ParticleTeam0, the mesh, SocketName, Location, Rotation, VFX Scale, KeepRelativeOffset); NotifyEnd with
        // KillOnNotifyEnd deactivates the mesh's child particle components of ParticleTeam0 / ParticleTeam1. The class
        // defaults: Location 0, Rotation 0, VFX Scale 1, KillOnNotifyEnd.
        let q = over_props(&cdo_props(ctx, PLAY_VFX)?, Some(&o));
        const ALLOWED: [&str; 4] = ["ParticleTeam0", "ParticleTeam1", "SocketName", "UseTeamColorParam?"];
        let (l, r, s) = (q.get("Location"), q.get("Rotation"), q.get("VFX Scale"));
        if o.keys().any(|k| !ALLOWED.contains(&k.as_str()))
            || truthy(q.get("UseSkinVFX"))
            || truthy(q.get("UseOverrideAttachParent"))
            || !truthy(q.get("KillOnNotifyEnd"))
            || truthy(q.get("UseTeamColorParam?"))
            || ["X", "Y", "Z"].iter().any(|k| truthy(at_(l, k)))
            || ["Pitch", "Yaw", "Roll"].iter().any(|k| truthy(at_(r, k)))
            || ["X", "Y", "Z"].iter().any(|k| number(at_(s, k)) != 1.0)
        {
            return fail(PART, format!("{name}: PlayVFX"));
        }
        let system = ctx.add_system(&reference(q.get("ParticleTeam0"), "ParticleTeam0")?)?;
        let socket = q.get("SocketName").and_then(Json::as_str).map(str::to_owned);
        out.push(Notify { at, end, sound: None, system: Some(system), socket });
    } else {
        let shown = n.get("NotifyName").and_then(Json::as_str).unwrap_or("undefined");
        return fail(PART, format!("{name}: notify {shown}"));
    }
    Ok(())
}

fn at_<'j>(v: Option<&'j Json>, k: &str) -> Option<&'j Json> {
    at(v, k)
}

fn montage_of<'a>(path: &str, ctx: &dyn SceneryBindCtx<'a>) -> Result<Montage, KindError> {
    let name = &path[path.rfind('.').map_or(0, |i| i + 1)..];
    let m = props_of(&ctx.load(path)?);
    for k in [
        "BlendIn",
        "BlendOut",
        "BlendOutTriggerTime",
        "bEnableAutoBlendOut",
        "RateScale",
        "CompositeSections",
        "BranchingPointMarkers",
    ] {
        if m.contains_key(k) {
            let cs = arr_of(m.get("CompositeSections")).unwrap_or(&[]);
            let single = k == "CompositeSections"
                && cs.len() == 1
                && is_str(cs[0].get("NextSectionName"), "None")
                && !truthy(cs[0].get("LinkValue"));
            if !single {
                return fail(PART, format!("{name}: montage {k}"));
            }
        }
    }
    let tracks = req_arr(PART, m.get("SlotAnimTracks"), "SlotAnimTracks")?;
    if tracks.len() != 1 || !is_str(tracks[0].get("SlotName"), "DefaultSlot") {
        return fail(PART, format!("{name}: slot tracks"));
    }
    let mut notifies = Vec::new();
    // a notify's time: its link value and TriggerTimeOffset (negative: before the link), kept as it is: a step fires a
    // notify when previous < time <= current (the exe's rule, FUN_1428719e0), and a montage starts at 0, so one at or
    // before 0 never fires (the burst's open sound at -0.0001)
    for n in arr_of(m.get("Notifies")).unwrap_or(&[]) {
        let lv = number(n.get("LinkValue"));
        let end = truthy(n.get("Duration")).then(|| lv + number(n.get("Duration")));
        notify_of(ctx, name, n, lv + or_num(n.get("TriggerTimeOffset"), 0.0), end, &mut notifies)?;
    }
    let mut segments = Vec::new();
    for s in req_arr(PART, at(tracks[0].get("AnimTrack"), "AnimSegments"), "AnimSegments")? {
        let anim = reference(s.get("AnimReference"), "a segment")?;
        let a = props_of(&ctx.load(&anim)?);
        let root_lock = a.get("RootMotionRootLock");
        if num_or(s.get("LoopingCount"), 1.0) != 1.0
            || num_or(a.get("RateScale"), 1.0) != 1.0
            || !(truthy(a.get("bEnableRootMotion")) || truthy(a.get("bForceRootLock")))
            || (truthy(root_lock) && !is_str(root_lock, "ERootMotionRootLock::RefPose"))
        {
            return fail(PART, format!("{anim}: segment"));
        }
        let rate = num_or(s.get("AnimPlayRate"), 1.0);
        let pos = num_or(s.get("StartPos"), 0.0);
        let start = num_or(s.get("AnimStartTime"), 0.0);
        let end = number(s.get("AnimEndTime"));
        let mut seg = JsonProps::new();
        put(&mut seg, "anim", ctx.anim_path(&anim, true)?);
        put(&mut seg, "pos", pos);
        put(&mut seg, "start", start);
        put_opt(&mut seg, "end", s.get("AnimEndTime"));
        put(&mut seg, "rate", rate);
        if rate.is_nan() || rate <= 0.0 {
            return fail(PART, format!("{anim}: segment rate"));
        }
        for n in arr_of(a.get("Notifies")).unwrap_or(&[]) {
            let t = number(n.get("LinkValue")) + or_num(n.get("TriggerTimeOffset"), 0.0);
            if t >= start && t < end {
                notify_of(ctx, name, n, pos + (t - start) / rate, None, &mut notifies)?;
            }
        }
        segments.push(Json::Obj(seg));
    }
    notifies.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(Ordering::Equal));
    let mut o = JsonProps::new();
    put_opt(&mut o, "length", m.get("SequenceLength"));
    put(&mut o, "blendIn", MONTAGE_DEFAULT_BLEND);
    put(&mut o, "blendOut", MONTAGE_DEFAULT_BLEND);
    put(&mut o, "segments", Json::Arr(segments));
    put(&mut o, "notifies", Json::Arr(notifies.iter().map(Notify::json).collect()));
    Ok(Montage { json: Json::Obj(o), sockets: notifies.iter().filter_map(|n| n.socket.clone()).collect() })
}

/// The sockets of a skeleton (its SkeletalMeshSocket inner objects) by name.
fn sockets_of(skel: &ObjView<'_>) -> Result<Vec<(String, JsonProps)>, KindError> {
    let mut out = Vec::new();
    for o in skel.inner()? {
        if o.class().ends_with(".SkeletalMeshSocket") {
            let p = props_of(&o);
            let name = p.get("SocketName").and_then(Json::as_str).unwrap_or("undefined").to_owned();
            out.push((name, p));
        }
    }
    Ok(out)
}

pub struct TrophysEdge;

impl SceneryPart for TrophysEdge {
    fn name(&self) -> &'static str {
        "trophyroom"
    }
    fn blueprints(&self) -> &'static [&'static str] {
        &[
            "TROPHY_LV03_PlatformCenter_A_01_Blueprint_C",
            "TROPHY_TUTORIAL_PlatformCenter_Blueprint_C",
            "TROPHY_AcmeCrate_A_01_Actor_C",
        ]
    }
    fn levels(&self) -> &'static [&'static str] {
        &["Map_TrophyRoom_Tutorial_C"]
    }
    fn run<'a>(&self) -> Box<dyn SceneryPartRun<'a> + 'a> {
        Box::new(TrophyRun::default())
    }
}

/// One bind's state.
#[derive(Default)]
struct TrophyRun {
    crate_: Option<JsonProps>,
    /// `tutorialSolids`: class -> instance name.
    solids: Option<JsonProps>,
}

impl<'a> SceneryPartRun<'a> for TrophyRun {
    // The tutorial level's: the solids its tutorials spawn (`tutorialSolids`, above).
    fn level(&mut self, cls: &str, ctx: &dyn SceneryBindCtx<'a>) -> Result<SceneryLevelRun, KindError> {
        if cls != TUTORIAL_LEVEL {
            return fail(PART, format!("level {cls}"));
        }
        if self.solids.is_none() {
            self.solids = Some(tutorial_solids_of(ctx)?);
        }
        Ok(SceneryLevelRun { tags: Vec::new() })
    }

    // TROPHY_AcmeCrate_A_01_Actor_C's skeletal mesh (the actor's DefaultSceneRoot is its place).
    fn skeletal_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        if cls != CRATE {
            return Ok(false);
        }
        if self.crate_.is_some() {
            return fail(PART, format!("{}: two crates in one entry", owner.path()));
        }
        let q = component_props(ctx, e)?;
        if truthy(q.get("RelativeLocation")) || truthy(q.get("RelativeRotation")) || truthy(q.get("RelativeScale3D")) {
            return fail(PART, format!("{}: crate mesh placement", owner.path()));
        }
        ctx.add_skeletal(
            name_of(owner),
            &q,
            &ctx.root_of(owner)?,
            &SkeletalOpts { anim_bp: Some(ANIM_BP.to_owned()), ..Default::default() },
        )?;
        let shake = montage_of(SHAKE, ctx)?;
        let burst = montage_of(BURST, ctx)?;
        // the socket the PlayVFX notify names, on the mesh's skeleton
        let mesh = props_of(&ctx.load(&reference(q.get("SkeletalMesh"), "the crate mesh")?)?);
        if arr_of(mesh.get("Sockets")).is_some_and(|s| !s.is_empty()) {
            return fail(PART, "crate: sockets on the mesh".into());
        }
        let skeleton = ctx.load(&reference(mesh.get("Skeleton"), "the crate skeleton")?)?;
        // The animations' translations as UAnimSequence::GetBonePose retargets them: per bone by the skeleton's
        // BoneTree TranslationRetargetingMode. The renderer plays the keys as they are, mode Animation's rule: another
        // mode stops the bind.
        let skeleton_props = props_of(&skeleton);
        let modes = arr_of(skeleton_props.get("BoneTree")).unwrap_or(&[]);
        if modes.is_empty()
            || modes
                .iter()
                .any(|b| !is_str(b.get("TranslationRetargetingMode"), "EBoneTranslationRetargetingMode::Animation"))
        {
            return fail(PART, "crate: a bone translation retargeting mode other than Animation".into());
        }
        let sockets = sockets_of(&skeleton)?;
        let used: IndexSet<&String> = shake.sockets.iter().chain(&burst.sockets).collect();
        let mut socket = JsonProps::new();
        for s in used {
            // (a Map of the sockets by name keeps the last of a name)
            let k = sockets.iter().rev().find(|(n, _)| n == s).map(|(_, p)| p);
            let Some(k) = k.filter(|k| !truthy(k.get("RelativeScale"))) else {
                return fail(PART, format!("crate: socket {s}"));
            };
            let v = if is_struct(k.get("RelativeLocation")) { k.get("RelativeLocation") } else { None };
            let rr = if is_struct(k.get("RelativeRotation")) { k.get("RelativeRotation") } else { None };
            let n0 = |v: Option<&Json>, key: &str| num_or(at(v, key), 0.0);
            let mut o = JsonProps::new();
            put_opt(&mut o, "bone", k.get("BoneName"));
            put(&mut o, "loc", triple([n0(v, "X"), n0(v, "Y"), n0(v, "Z")]));
            put(&mut o, "rot", triple([n0(rr, "Pitch"), n0(rr, "Yaw"), n0(rr, "Roll")]));
            put(&mut socket, s, Json::Obj(o));
        }
        let mut c = JsonProps::new();
        put(&mut c, "skeletal", name_of(owner));
        put(&mut c, "burstDelay", Json::Arr(BURST_DELAY.into_iter().map(Json::Num).collect()));
        put(&mut c, "shakeDelay", Json::Arr(SHAKE_DELAY.into_iter().map(Json::Num).collect()));
        put(&mut c, "shake", shake.json);
        put(&mut c, "burst", burst.json);
        put(&mut c, "socket", Json::Obj(socket));
        self.crate_ = Some(c);
        Ok(true)
    }

    // The centre platforms' StaticMesh: the template's relative transform under the placed Scene root, neither
    // rotated (the component's world transform: the Scene's location plus its scale times the mesh's location, the
    // scales' product).
    fn static_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
        p: &JsonProps,
    ) -> Result<StaticMeshOut, KindError> {
        if (cls != CENTER && cls != TUTORIAL_CENTER) || name_of(e) != "StaticMesh" {
            return fail(PART, format!("{}: static mesh component of {cls}", owner.path()));
        }
        let scene = comp_of(&ctx.level().objects, owner, "Scene")?;
        let sq = component_props(ctx, scene)?;
        let parent_is_scene = matches!(p.get("AttachParent"), Some(Json::Str(a)) if short_name(a) == "Scene");
        if truthy(sq.get("RelativeRotation")) || !parent_is_scene || truthy(p.get("RelativeRotation")) {
            return fail(PART, format!("{}: centre platform placement", owner.path()));
        }
        let (a, b) = (sq.get("RelativeLocation"), p.get("RelativeLocation"));
        let (sa, sb) = (sq.get("RelativeScale3D"), p.get("RelativeScale3D"));
        let mut loc = JsonProps::new();
        for k in ["X", "Y", "Z"] {
            put(&mut loc, k, num_or(at(a, k), 0.0) + num_or(at(sa, k), 1.0) * num_or(at(b, k), 0.0));
        }
        let mut out = p.clone();
        put(&mut out, "RelativeLocation", Json::Obj(loc));
        if truthy(sq.get("RelativeScale3D")) {
            let mut s = JsonProps::new();
            for k in ["X", "Y", "Z"] {
                put(&mut s, k, num_or(at(sa, k), 1.0) * num_or(at(sb, k), 1.0));
            }
            put(&mut out, "RelativeScale3D", Json::Obj(s));
        }
        out.shift_remove("AttachParent");
        Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: out, extra: JsonProps::new(), placed: true }))
    }

    fn level_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut out = JsonProps::new();
        if let Some(s) = &self.solids {
            put(&mut out, "tutorialSolids", Json::Obj(s.clone()));
        }
        Ok(out)
    }

    fn actor_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut out = JsonProps::new();
        if let Some(c) = &self.crate_ {
            put(&mut out, "crate", Json::Obj(c.clone()));
        }
        Ok(out)
    }

    fn summary(&self) -> String {
        match &self.crate_ {
            Some(c) => {
                let list = |k: &str| match c.get(k) {
                    Some(Json::Arr(a)) => a.iter().map(js_str).collect::<Vec<_>>().join(","),
                    _ => String::new(),
                };
                format!(", crate shake every {} s, burst every {} s", list("shakeDelay"), list("burstDelay"))
            }
            None => String::new(),
        }
    }
}

// A number as JavaScript prints it in a template.
fn js_str(v: &Json) -> String {
    match v {
        Json::Num(n) => {
            let mut b = Vec::new();
            crate::json::write_number(&mut b, *n);
            String::from_utf8(b).unwrap_or_default()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_of_a_crate() {
        let run = TrophyRun::default();
        assert_eq!(<TrophyRun as SceneryPartRun<'_>>::summary(&run), "");
        let mut c = JsonProps::new();
        put(&mut c, "shakeDelay", Json::Arr(vec![Json::Num(2.0), Json::Num(7.0)]));
        put(&mut c, "burstDelay", Json::Arr(vec![Json::Num(10.0), Json::Num(30.0)]));
        let run = TrophyRun { crate_: Some(c), solids: None };
        assert_eq!(<TrophyRun as SceneryPartRun<'_>>::summary(&run), ", crate shake every 2,7 s, burst every 10,30 s");
    }

    #[test]
    fn tables() {
        let part = TrophysEdge;
        assert_eq!(part.blueprints().len(), 3);
        assert_eq!(part.levels(), ["Map_TrophyRoom_Tutorial_C"]);
    }
}
