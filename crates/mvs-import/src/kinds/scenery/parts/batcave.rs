//! The Batcave's scenery part (`batcave`): its own Blueprint actors and level Blueprint.
//!
//! This writes what each draws at its place and pose at BeginPlay, standing still; the block `batcave` names the
//! pieces the sim and the renderer move (the cars' plays and splash, the fans' spin, the six warning screens' bHidden):
//! - BCAVE_ST_main_A_01_BP_C (data-only StaticMeshActor child): its native root StaticMeshComponent0, the centre
//!   floor's mesh (BCAVE_ST_main_A_01 with the level's BCAVE_ST_Main_MI, scale 1.15). Its boxes and Comp_EdgeTrace are
//!   the stage's.
//! - Batcave_FanBlades_C: its blade BCAVE_ST_fanBlade_A_01 under DefaultSceneRoot, at the template's relative
//!   transform (no rotation): the pose before the first tick. Each tick sets the blade's relative rotation to
//!   MakeRotator(Rotation, 0, 0), a roll, with Rotation -= DeltaSeconds x AdjustedRotationRate wrapped at -360
//!   (@208..505), AdjustedRotationRate = RotationRate from BeginPlay (@176): the spin is the motion track's (`fans`:
//!   the instance, RotationRate and the Rotation it starts from).
//! - BatcaveWallDestructible_C (the shared destructible's child), the two Batmobiles: SkeletalMesh (BatMobile_Comic on
//!   the left, BatMobile on the right) with its AnimClass BatMobile_AnimBlueprint_C, written playing the pose its graph
//!   has intact (batmobile_graph), the three montages (destroy, damage, respawn), the lights' sockets (`lights`,
//!   CAR_LIGHTS), every socket named (`sockets`) and the root's place (`root`, the Ak_OneShot notifies' AkComponent).
//! - BatcaveBatsBP_C / FogOcclusionBP_C draw nothing placed: `bats` and `fogs` (bats_of, fogs_of) are the emitters
//!   their graphs spawn, the renderer's from the sim's state.
//! - Map_Batcave_C: BeginPlay ends with SetBatcaveWarningState(Idle), which hides the six warning screens
//!   (BatScreenBats / BatScreenSteam, their Left / Right; SetActorHiddenInGame(true)): written hidden (`screens`, in
//!   the sim's SCREEN_* order), whatever their bHidden in the level (the steam ones are saved hidden, the bats ones
//!   shown).
//! - BatCenterStage_2 (a SkeletalMeshActor, its AnimClass BatCenterStage_Skeleton_AnimBlueprint_C) is bHidden with its
//!   mesh bVisible false and no graph shows it: the core leaves it out as any hidden actor.

use indexmap::IndexSet;

use crate::json::Json;
use crate::kind::KindError;
use crate::kinds::scenery::part::{
    InstanceActor, JsonProps, SceneryBindCtx, SceneryLevelRun, SceneryMeshOut, SceneryPart, SceneryPartRun,
    SkeletalOpts, SkeletalPlay, StaticMeshOut,
};
use crate::objview::{ObjView, short_name};

use super::util::js::{arr, first, is_str, req_arr, truthy};
use super::util::{
    Xform, cdo_props, class_inner, json_truthy, name_of, number, over_props, props_of, props_of_actor, reference,
    vars_of, vec, world_of, xf_mul, xf_out,
};

const FLOOR: &str = "BCAVE_ST_main_A_01_BP_C";
const FAN: &str = "Batcave_FanBlades_C";
const CAR: &str = "BatcaveWallDestructible_C";
const CAR_ANIMBP: &str = "BatMobile_AnimBlueprint_C";
const LEVEL: &str = "Map_Batcave_C";
const MAP: &str = "/Game/Panda_Main/Maps/Map_Batcave/";
const CAR_ANIMBP_CLASS: &str =
    "/Game/Panda_Main/Maps/Map_Batcave/BatMobile/BatMobile_AnimBlueprint.BatMobile_AnimBlueprint_C";
const FAN_CLASS: &str = "/Game/Panda_Main/Maps/Map_Batcave/Blueprints/Batcave_FanBlades.Batcave_FanBlades_C";
const LEVEL_CLASS: &str = "/Game/Panda_Main/Maps/Map_Batcave/Map_Batcave.Map_Batcave_C";
const PLAY_VFX: &str = "/Game/Panda_Main/Blueprints/AnimNotifies/AnimNotify_PlayVFX.AnimNotify_PlayVFX_C";
const ONE_SHOT: &str = "/Game/Panda_Main/Ak_Audio/AkNotify/Ak_OneShot.Ak_OneShot_C";
const BATS: &str = "/Game/Panda_Main/Maps/Props/BatcaveBats/";
// SetBatcaveWarningState's six screens (Map_Batcave @134..167), in the sim's screen order
const SCREENS: [&str; 6] = [
    "BCAVE_BG_BatScreenBats_A_01_8",
    "BCAVE_BG_BatScreenBatsLeft_A_01_6",
    "BCAVE_BG_BatScreenBatsRight_A_01_4",
    "BCAVE_BG_BatScreenSteam_A_01_2",
    "BCAVE_BG_BatScreenSteamLeft_A_01_5",
    "BCAVE_BG_BatScreenSteamRight_A_01_8",
];
// SetTopLights @6788 / SetFrontLights @6157: SpawnEmitterAttached(system, SkeletalMesh, socket, (0,0,0), rot(0,0,0),
// (1,1,1), KeepRelativeOffset, bAutoDestroy) per socket (the systems by name: the renderer's).
const CAR_LIGHTS_TOP: (&str, [&str; 4]) =
    ("PS_BatmobileHit_PlatformLights", ["VFX_Light01", "VFX_Light02", "VFX_Light03", "VFX_Light04"]);
const CAR_LIGHTS_FRONT: (&str, [&str; 2]) = ("PS_BatmobileHit_Lights", ["VFX_CarLightL", "VFX_CarLightR"]);

fn fail<T>(what: impl std::fmt::Display) -> Result<T, KindError> {
    Err(KindError::Failed(format!("scenery batcave: {what}")))
}

pub(super) fn at(dir: &str, n: &str) -> String {
    format!("{dir}{n}.{n}")
}

// The properties' values as a JavaScript bind reads them: `undefined` is `None`, a missing property is not an error
// until the read of a member of it is (a TypeError there, a failure here).

pub(super) fn need<'j>(v: Option<&'j Json>, what: &str) -> Result<&'j Json, KindError> {
    v.map_or_else(|| fail(format!("{what} is missing")), Ok)
}

pub(super) fn is_num(v: Option<&Json>, n: f64) -> bool {
    v.and_then(Json::as_f64) == Some(n)
}

/// `(v ?? d) === n`.
pub(super) fn is_num_or(v: Option<&Json>, d: f64, n: f64) -> bool {
    first(v, None).map_or(d == n, |v| v.as_f64() == Some(n))
}

fn is_str_or(v: Option<&Json>, d: &str, s: &str) -> bool {
    first(v, None).map_or(d == s, |v| v.as_str() == Some(s))
}

/// The short name of a reference property (`tail`).
pub(super) fn tail(v: Option<&Json>) -> Option<String> {
    v.and_then(Json::as_str).map(|s| short_name(s).to_owned())
}

/// A package's name (`/Game/A/B.C` -> `B`), as the renderer names an asset: the object's own can differ in case.
fn pkg_name(p: &str) -> &str {
    let slash = p.rfind('/').map_or(0, |i| i + 1);
    let end = p[slash..].find('.').map_or(p.len(), |i| slash + i);
    &p[slash..end]
}

/// An object of JSON members, those given as `None` left out (a JS `undefined` member).
pub(super) fn obj<const N: usize>(members: [(&str, Option<Json>); N]) -> Json {
    Json::object(members.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))))
}

pub(super) fn num(n: f64) -> Json {
    Json::Num(n)
}

pub(super) fn nums(v: &[f64]) -> Json {
    Json::Arr(v.iter().map(|&n| Json::Num(n)).collect())
}

fn strs(v: &[&str]) -> Json {
    Json::Arr(v.iter().map(|&s| Json::from(s)).collect())
}

/// `[X, Y, Z]` of a struct, `d` when it is not truthy; a missing member is `undefined` (null).
fn xyz(v: Option<&Json>, d: [f64; 3]) -> Json {
    if truthy(v) { Json::Arr(["X", "Y", "Z"].map(|k| num(number(v.and_then(|v| v.get(k))))).into()) } else { nums(&d) }
}

/// v3 of the notifies: `[X ?? 0, Y ?? 0, Z ?? 0]`, `d` when the struct is not truthy.
pub(super) fn v3d(v: Option<&Json>, d: [f64; 3]) -> Json {
    if truthy(v) {
        Json::Arr(
            ["X", "Y", "Z"].map(|k| num(first(v.and_then(|v| v.get(k)), None).map_or(0.0, |n| number(Some(n))))).into(),
        )
    } else {
        nums(&d)
    }
}

pub(super) fn r3(v: Option<&Json>) -> Json {
    if truthy(v) {
        Json::Arr(
            ["Pitch", "Yaw", "Roll"]
                .map(|k| num(first(v.and_then(|v| v.get(k)), None).map_or(0.0, |n| number(Some(n)))))
                .into(),
        )
    } else {
        nums(&[0.0; 3])
    }
}

/// An object of the level by its actor's and its own name.
pub(super) fn level_obj<'v, 'a>(
    ctx: &'v dyn SceneryBindCtx<'a>,
    actor: &ObjView<'_>,
    name: &str,
) -> Option<&'v ObjView<'a>> {
    let path = format!("{}.{name}", actor.path());
    ctx.level().objects.iter().find(|x| x.path() == path)
}

pub(super) fn object_of<'a>(ctx: &dyn SceneryBindCtx<'a>, path: Option<&Json>) -> Result<ObjView<'a>, KindError> {
    ctx.load(&reference(path, "a reference")?)
}

/// The index a `LinkID` names: `(v as Props).LinkID`.
fn link_id(v: Option<&Json>) -> Option<usize> {
    v.and_then(|v| v.get("LinkID")).and_then(Json::as_f64).and_then(|n| usize::try_from(n as i64).ok())
}

/// `a[i]` of an array value by a numeric index value.
fn index<'j>(a: Option<&'j Json>, i: Option<&Json>) -> Option<&'j Json> {
    let i = i.and_then(Json::as_f64)?;
    a?.as_array()?.get(usize::try_from(i as i64).ok()?)
}

// The Batmobile's AnimBP as it draws an intact car (BatMobile_AnimBlueprint; the class default's nodes, a LinkID the
// node's place among the AnimGraphNode_ properties): Root <- Slot DefaultSlot <- BlendListByBool(IsActive, the class
// default true and !Destroyed after the first update: IsActive = Not_PreBool) whose true pose is the state machine
// Idle; its initial state NoWieght plays BatMobile_Idle (looping, rate 1, from 0). Returns that sequence (`idle`, the
// entry's play) and the whole graph as the renderer runs it (`anim`):
// - BlendListByBool's false pose: BatMobile_Retracted_Idle (not looping, rate 1, from 0), blend times 0 (a switch);
// - the state machine Idle (bReinitializeOnBecomingRelevant, bSkipFirstUpdateTransition): NoWieght (BatMobile_Idle,
//   looping) and Wieght (BatMobile_IdleWieght, looping); NoWieght -> Wieght when HasWieght, Wieght -> NoWieght when
//   !HasWieght (the rules' property copies: HasWieght, and HasWieght with LogicalNegateBool), each a custom transition
//   of CrossfadeDuration 0.5 whose blend graph's result is a sequence player alone (BatMobile_WieghtAdd /
//   BatMobile_WieghtLift, not looping, from 0): the pose for the 0.5 s is that player's.
// Every player plays at rate 1 from 0 with no sync group; the sequences carry no root motion and no notifies.
fn batmobile_graph<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<(String, Json), KindError> {
    let g = cdo_props(ctx, CAR_ANIMBP_CLASS)?;
    let nodes: Vec<&str> = g.keys().filter(|k| k.starts_with("AnimGraphNode_")).map(String::as_str).collect();
    let node = |n: &str| g.get(n);
    let want = |cond: bool, what: &str| -> Result<(), KindError> {
        if cond { Ok(()) } else { fail(format!("BatMobile_AnimBlueprint changed: {what}")) }
    };
    let name_at = |i: Option<usize>| i.and_then(|i| nodes.get(i).copied());
    let link = |n: &str, pin: &str| name_at(link_id(node(n).and_then(|x| x.get(pin))));
    want(
        link("AnimGraphNode_Root", "Result") == Some("AnimGraphNode_Slot")
            && is_str(node("AnimGraphNode_Slot").and_then(|s| s.get("SlotName")), "DefaultSlot"),
        "root",
    )?;
    want(link("AnimGraphNode_Slot", "Source") == Some("AnimGraphNode_BlendListByBool"), "slot source")?;
    let bl = need(node("AnimGraphNode_BlendListByBool"), "AnimGraphNode_BlendListByBool")?;
    let poses = req_arr("batcave", bl.get("BlendPose"), "BlendPose")?;
    let blend_times = req_arr("batcave", bl.get("BlendTime"), "BlendTime")?;
    want(
        g.get("IsActive") == Some(&Json::Bool(true))
            && name_at(link_id(poses.first())) == Some("AnimGraphNode_StateMachine")
            && blend_times.iter().all(|t| is_num(Some(t), 0.0))
            && poses.len() == 2
            && !truthy(bl.get("bResetChildOnActivation")),
        "blend by IsActive",
    )?;
    let cls = props_of(&ctx.load(CAR_ANIMBP_CLASS)?);
    let sm_node = need(node("AnimGraphNode_StateMachine"), "AnimGraphNode_StateMachine")?;
    let sm_index = first(sm_node.get("StateMachineIndexInClass"), None);
    let sm =
        need(index(cls.get("BakedStateMachines"), Some(sm_index.unwrap_or(&Json::Num(0.0)))), "the state machine")?;
    let states = req_arr("batcave", sm.get("States"), "States")?;
    let init = need(index(sm.get("States"), sm.get("InitialState")), "the initial state")?;
    let init_players = req_arr("batcave", init.get("PlayerNodeIndices"), "PlayerNodeIndices")?;
    want(
        is_str(sm.get("MachineName"), "Idle") && is_str(init.get("StateName"), "NoWieght") && init_players.len() == 1,
        "state machine",
    )?;
    let mut all_states_plain = true;
    for s in states {
        all_states_plain &= !truthy(s.get("bAlwaysResetOnEntry"))
            && !truthy(s.get("bIsAConduit"))
            && req_arr("batcave", s.get("PlayerNodeIndices"), "PlayerNodeIndices")?.len() == 1
            && req_arr("batcave", s.get("Transitions"), "Transitions")?.len() == 1;
    }
    want(
        states.len() == 2
            && states.get(1).is_some_and(|s| is_str(s.get("StateName"), "Wieght"))
            && truthy(sm_node.get("bReinitializeOnBecomingRelevant"))
            && truthy(sm_node.get("bSkipFirstUpdateTransition"))
            && all_states_plain,
        "states",
    )?;
    // (the baked index counts the nodes from the end)
    let baked = |i: Option<&Json>| -> Option<&str> {
        let i = usize::try_from(i?.as_f64()? as i64).ok()?;
        nodes.get(nodes.len().checked_sub(1 + i)?).copied()
    };
    // a sequence player as the renderer plays it (its animation by build object path)
    let player_of = |name: Option<&str>, looping: bool, what: &str| -> Result<Json, KindError> {
        let p = name.and_then(node);
        let ok = p.is_some_and(|p| {
            p.get("bLoopAnimation") == Some(&Json::Bool(looping))
                && is_num(p.get("PlayRate"), 1.0)
                && is_num_or(p.get("PlayRateBasis"), 1.0, 1.0)
                && !truthy(p.get("StartPosition"))
                && is_str_or(p.get("GroupName"), "None", "None")
        });
        let name = name.unwrap_or("undefined");
        want(ok, &format!("{what} player {name}"))?;
        let seq = p.and_then(|p| p.get("Sequence"));
        let a = props_of(&object_of(ctx, seq)?);
        want(
            arr(a.get("Notifies")).is_empty()
                && !truthy(a.get("bEnableRootMotion"))
                && is_num_or(a.get("RateScale"), 1.0, 1.0)
                && is_str_or(a.get("AdditiveAnimType"), "AAT_None", "AAT_None"),
            &format!("{what} sequence"),
        )?;
        Ok(obj([
            ("anim", Some(Json::from(ctx.anim_path(&reference(seq, name)?, false)?))),
            ("length", a.get("SequenceLength").cloned()),
        ]))
    };
    let player = baked(init_players.first());
    let player_node = need(player.and_then(node), "the idle player")?;
    want(tail(player_node.get("Sequence")).as_deref() == Some("BatMobile_Idle"), "idle player")?;
    // the rules: the copy into each transition's result (EvaluateGraphExposedInputs, PropertyAccessLibrary)
    let lib = need(cls.get("PropertyAccessLibrary"), "PropertyAccessLibrary")?;
    let exposed = req_arr("batcave", cls.get("EvaluateGraphExposedInputs"), "EvaluateGraphExposedInputs")?;
    let rule_of = |result_node: &str| -> Result<bool, KindError> {
        let h = exposed.iter().find(|x| {
            x.get("ValueHandlerNodeProperty")
                .and_then(|v| v.get("Path"))
                .and_then(|p| p.as_array())
                .and_then(|p| p.first())
                .and_then(Json::as_str)
                == Some(result_node)
        });
        let rec = arr(h.and_then(|h| h.get("CopyRecords")));
        want(
            h.is_some_and(|h| is_str(h.get("BoundFunction"), "None")) && rec.len() == 1,
            &format!("rule {result_node}"),
        )?;
        let batch = match lib.get("CopyBatches") {
            Some(Json::Arr(a)) => a.first(),
            other => other,
        };
        let c = need(index(batch.and_then(|b| b.get("Copies")), rec[0].get("CopyIndex")), "a copy")?;
        let start = index(lib.get("SrcPaths"), c.get("AccessIndex")).and_then(|s| s.get("PathSegmentStartIndex"));
        let src = index(lib.get("PathSegments"), start).and_then(|s| s.get("Name"));
        want(is_str(src, "HasWieght"), "rule source")?;
        if is_str(rec[0].get("PostCopyOperation"), "EPostCopyOperation::LogicalNegateBool") {
            return Ok(false);
        }
        want(is_str(rec[0].get("PostCopyOperation"), "EPostCopyOperation::None"), "rule op")?;
        Ok(true)
    };
    let mut transitions: Vec<(bool, Json)> = Vec::new();
    for (k, s) in states.iter().enumerate() {
        let st = need(req_arr("batcave", s.get("Transitions"), "Transitions")?.first(), "a transition")?;
        let tr = need(index(sm.get("Transitions"), st.get("TransitionIndex")), "a transition")?;
        let kk = k as f64;
        want(
            is_num(tr.get("PreviousState"), kk)
                && is_num(tr.get("NextState"), 1.0 - kk)
                && is_str(tr.get("LogicType"), "ETransitionLogicType::TLT_Custom")
                && truthy(st.get("bDesiredTransitionReturnValue"))
                && !truthy(st.get("bAutomaticRemainingTimeRule")),
            &format!("transition {k}"),
        )?;
        let custom = need(baked(st.get("CustomResultNodeIndex")).and_then(node), "a custom result")?;
        let seq = name_at(link_id(custom.get("Result")));
        let when = rule_of(baked(st.get("CanTakeDelegateIndex")).map_or_else(|| fail("a delegate"), Ok)?)?;
        let pose = player_of(seq, false, "transition")?;
        transitions.push((
            when,
            obj([
                ("when", Some(Json::Bool(when))),
                ("time", tr.get("CrossfadeDuration").cloned()),
                ("pose", Some(pose)),
            ]),
        ));
    }
    want(transitions.len() == 2 && transitions[0].0 && !transitions[1].0, "rules")?;
    let mut state_players = Vec::new();
    for s in states {
        let n = baked(req_arr("batcave", s.get("PlayerNodeIndices"), "PlayerNodeIndices")?.first());
        state_players.push(player_of(n, true, s.get("StateName").and_then(Json::as_str).unwrap_or("undefined"))?);
    }
    let retracted = player_of(name_at(link_id(poses.get(1))), false, "retracted")?;
    let anim = Json::object([
        ("retracted", retracted),
        ("states", Json::Arr(state_players)),
        ("add", transitions[0].1.clone()),
        ("lift", transitions[1].1.clone()),
    ]);
    Ok((reference(player_node.get("Sequence"), "the idle")?, anim))
}

// A montage of the car as the renderer plays it: its one section's DefaultSlot segments (montage position `pos`, the
// animation's [start, end] at `rate`), its blend times and its queued notifies at montage time, the slot sequences'
// own among them. A notify `at` its link value and TriggerTimeOffset (fired when previous < at <= current), `end` a
// notify state's end; by kind:
// - AnimNotify_PlayParticleEffect (engine): `particle`, attached (Attached, the default): SpawnEmitterAttached(
//   PSTemplate, the mesh, SocketName, LocationOffset, RotationOffset, Scale, KeepRelativeOffset, bAutoDestroy).
// - AnimNotifyState_TimedParticleEffect: `timed`, the same at its begin with scale 1 and bAutoDestroy =
//   !bDestroyAtEnd; its end destroys (bDestroyAtEnd) or deactivates the first such system still active.
// - AnimNotify_PlayVFX (a non-fighter owner's SpawnVFX @2233: SpawnEmitterAttached(ParticleTeam0, the mesh, SocketName,
//   Location, Rotation, VFX Scale, KeepRelativeOffset); ApplyMirroring is the fighter path's): `vfx`, its end
//   deactivating it with KillOnNotifyEnd.
// - Ak_OneShot (Follow and not Local Only: the class default): `sound`, its event on the owner's root AkComponent
//   (GetAkComponent @329; the car's root, Box, has no sockets: Attach Name is not a place).
// - AnimNotify_TrackByCamera (the retract's): nothing on a car.

/// What a montage's notifies gather: the sockets they name and the notifies at their montage time.
struct Gather<'c> {
    play_vfx: &'c JsonProps,
    one_shot: &'c JsonProps,
    sockets: IndexSet<String>,
    list: Vec<(f64, Json)>,
}

fn only(q: &JsonProps, keys: &[&str], name: &str, n: &Json) -> Result<(), KindError> {
    let extra: Vec<&str> = q.keys().map(String::as_str).filter(|k| !keys.contains(k)).collect();
    if extra.is_empty() {
        Ok(())
    } else {
        fail(format!(
            "{name}: {} {}",
            n.get("NotifyName").and_then(Json::as_str).unwrap_or("undefined"),
            extra.join(",")
        ))
    }
}

fn notify<'a>(
    ctx: &dyn SceneryBindCtx<'a>,
    g: &mut Gather<'_>,
    name: &str,
    n: &Json,
    t: f64,
    end: Option<f64>,
) -> Result<(), KindError> {
    if !is_str(n.get("MontageTickType"), "EMontageNotifyTickType::Queued")
        || !is_num_or(n.get("NotifyTriggerChance"), 1.0, 1.0)
    {
        return fail(format!("{name}: notify tick type"));
    }
    let nr = first(n.get("Notify"), None).or(n.get("NotifyStateClass"));
    let o = match nr {
        Some(Json::Str(path)) => Some(ctx.load(path)?),
        _ => None,
    };
    let q = o.as_ref().map(props_of).unwrap_or_default();
    let kind = o.as_ref().map_or("", |o| short_name(o.class()));
    let notify_name = n.get("NotifyName").and_then(Json::as_str).unwrap_or("undefined");
    let socket_of = |q: &JsonProps| -> Result<String, KindError> {
        match q.get("SocketName") {
            Some(Json::Str(s)) => Ok(s.clone()),
            _ => fail(format!("{name}: {notify_name}: no SocketName")),
        }
    };
    let end = end.map(num);
    if kind == "AnimNotify_PlayParticleEffect" {
        only(&q, &["PSTemplate", "LocationOffset", "RotationOffset", "Scale", "SocketName"], name, n)?;
        let socket = socket_of(&q)?;
        g.sockets.insert(socket.clone());
        let system = ctx.add_system(&reference(q.get("PSTemplate"), "PSTemplate")?)?;
        g.list.push((
            t,
            obj([
                ("at", Some(num(t))),
                ("kind", Some(Json::from("particle"))),
                ("system", Some(Json::from(system))),
                ("socket", Some(Json::from(socket))),
                ("loc", Some(v3d(q.get("LocationOffset"), [0.0; 3]))),
                ("rot", Some(r3(q.get("RotationOffset")))),
                ("scale", Some(v3d(q.get("Scale"), [1.0; 3]))),
            ]),
        ));
    } else if kind == "AnimNotifyState_TimedParticleEffect" {
        only(&q, &["PSTemplate", "LocationOffset", "RotationOffset", "SocketName", "bDestroyAtEnd"], name, n)?;
        let socket = socket_of(&q)?;
        g.sockets.insert(socket.clone());
        let system = ctx.add_system(&reference(q.get("PSTemplate"), "PSTemplate")?)?;
        g.list.push((
            t,
            obj([
                ("at", Some(num(t))),
                ("end", end),
                ("kind", Some(Json::from("timed"))),
                ("system", Some(Json::from(system))),
                ("socket", Some(Json::from(socket))),
                ("loc", Some(v3d(q.get("LocationOffset"), [0.0; 3]))),
                ("rot", Some(r3(q.get("RotationOffset")))),
                ("scale", Some(nums(&[1.0; 3]))),
                ("destroy", Some(Json::Bool(truthy(q.get("bDestroyAtEnd"))))),
            ]),
        ));
    } else if notify_name == "PlayVFX_C" {
        let p = over_props(g.play_vfx, Some(&q));
        only(
            &q,
            &[
                "ParticleTeam0",
                "ParticleTeam1",
                "Location",
                "Rotation",
                "ApplyMirroring",
                "SocketName",
                "KillOnNotifyEnd",
            ],
            name,
            n,
        )?;
        if truthy(p.get("UseSkinVFX"))
            || truthy(p.get("UseOverrideAttachParent"))
            || !truthy(p.get("SocketName"))
            || is_str(p.get("SocketName"), "None")
        {
            return fail(format!("{name}: PlayVFX"));
        }
        let socket = socket_of(&p)?;
        g.sockets.insert(socket.clone());
        let system = ctx.add_system(&reference(p.get("ParticleTeam0"), "ParticleTeam0")?)?;
        g.list.push((
            t,
            obj([
                ("at", Some(num(t))),
                ("end", end),
                ("kind", Some(Json::from("vfx"))),
                ("system", Some(Json::from(system))),
                ("socket", Some(Json::from(socket))),
                ("loc", Some(v3d(p.get("Location"), [0.0; 3]))),
                ("rot", Some(r3(p.get("Rotation")))),
                ("scale", Some(v3d(p.get("VFX Scale"), [1.0; 3]))),
                ("kill", Some(Json::Bool(truthy(p.get("KillOnNotifyEnd"))))),
            ]),
        ));
    } else if notify_name == "Ak_OneShot_C" {
        only(&q, &["AkEvent", "Attach Name"], name, n)?;
        let p = over_props(g.one_shot, Some(&q));
        if !truthy(p.get("Follow")) || truthy(p.get("Local Only")) || !truthy(p.get("AkEvent")) {
            return fail(format!("{name}: Ak_OneShot"));
        }
        g.list.push((
            t,
            obj([
                ("at", Some(num(t))),
                ("kind", Some(Json::from("sound"))),
                ("sound", tail(p.get("AkEvent")).map(Json::from)),
            ]),
        ));
    } else if notify_name != "TrackByCamera_C" {
        return fail(format!("{name}: notify {notify_name}"));
    }
    Ok(())
}

/// The trigger time of a notify: its link value plus TriggerTimeOffset.
pub(super) fn notify_time(n: &Json) -> f64 {
    let offset = n.get("TriggerTimeOffset").filter(|v| json_truthy(v));
    number(n.get("LinkValue")) + offset.map_or(0.0, |v| number(Some(v)))
}

#[allow(clippy::neg_cmp_op_on_partial_ord)] // `!(x > 0)` is true for NaN
fn montage_of<'a>(
    ctx: &dyn SceneryBindCtx<'a>,
    r: Option<&Json>,
    what: &str,
) -> Result<(Json, IndexSet<String>), KindError> {
    let asset = object_of(ctx, r)?;
    let name = pkg_name(asset.path()).to_owned();
    let m = props_of(&asset);
    let tracks = req_arr("batcave", m.get("SlotAnimTracks"), "SlotAnimTracks")?;
    let sections = req_arr("batcave", m.get("CompositeSections"), "CompositeSections")?;
    let known =
        ["BlendIn", "BlendOut", "CompositeSections", "SlotAnimTracks", "Notifies", "SequenceLength", "Skeleton"];
    let other: Vec<&str> = m.keys().map(String::as_str).filter(|k| !known.contains(k)).collect();
    if !asset.class().ends_with(".AnimMontage")
        || !other.is_empty()
        || tracks.len() != 1
        || !is_str(tracks[0].get("SlotName"), "DefaultSlot")
        || sections.len() != 1
        || !is_str(sections[0].get("NextSectionName"), "None")
        || truthy(sections[0].get("LinkValue"))
    {
        return fail(format!("{what}: montage {name} {}", other.join(",")));
    }
    // (UAnimMontage's ctor: 0.25)
    let blend = |b: Option<&Json>| -> Result<Json, KindError> {
        let k: Vec<&str> = b
            .and_then(Json::as_object)
            .map_or(Vec::new(), |o| o.keys().map(String::as_str).filter(|x| *x != "BlendTime").collect());
        if !k.is_empty() {
            return fail(format!("{name}: blend {}", k.join(",")));
        }
        Ok(first(b.and_then(|b| b.get("BlendTime")), None).cloned().unwrap_or(num(0.25)))
    };
    let play_vfx = cdo_props(ctx, PLAY_VFX)?;
    let one_shot = cdo_props(ctx, ONE_SHOT)?;
    let mut g = Gather { play_vfx: &play_vfx, one_shot: &one_shot, sockets: IndexSet::new(), list: Vec::new() };
    for n in arr(m.get("Notifies")) {
        let lv = number(n.get("LinkValue"));
        let end = truthy(n.get("Duration")).then(|| lv + number(n.get("Duration")));
        notify(ctx, &mut g, &name, n, notify_time(n), end)?;
    }
    let mut segments = Vec::new();
    let anim_track = tracks[0].get("AnimTrack");
    for s in arr(anim_track.and_then(|t| t.get("AnimSegments"))) {
        let anim = reference(s.get("AnimReference"), "a segment")?;
        let a = props_of(&ctx.load(&anim)?);
        let rate = first(s.get("AnimPlayRate"), None).map_or(1.0, |v| number(Some(v)));
        if !is_num_or(s.get("LoopingCount"), 1.0, 1.0)
            || !is_num_or(a.get("RateScale"), 1.0, 1.0)
            || truthy(a.get("bEnableRootMotion"))
            || !(rate > 0.0)
        {
            return fail(format!("{}: segment", pkg_name(&anim)));
        }
        let pos = first(s.get("StartPos"), None).map_or(0.0, |v| number(Some(v)));
        let start = first(s.get("AnimStartTime"), None).map_or(0.0, |v| number(Some(v)));
        let end = number(s.get("AnimEndTime"));
        segments.push(obj([
            ("anim", Some(Json::from(ctx.anim_path(&anim, false)?))),
            ("pos", Some(num(pos))),
            ("start", Some(num(start))),
            ("end", s.get("AnimEndTime").cloned()),
            ("rate", Some(num(rate))),
        ]));
        for n in arr(a.get("Notifies")) {
            let t = notify_time(n);
            if t >= start && t < end {
                let nend = truthy(n.get("Duration")).then(|| pos + (t + number(n.get("Duration")) - start) / rate);
                notify(ctx, &mut g, &name, n, pos + (t - start) / rate, nend)?;
            }
        }
    }
    g.list.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let notifies = Json::Arr(g.list.into_iter().map(|(_, j)| j).collect());
    let play = obj([
        ("name", Some(Json::from(name.as_str()))),
        ("length", m.get("SequenceLength").cloned()),
        ("blendIn", Some(blend(m.get("BlendIn"))?)),
        ("blendOut", Some(blend(m.get("BlendOut"))?)),
        ("segments", Some(Json::Arr(segments))),
        ("notifies", Some(notifies)),
    ]);
    Ok((play, g.sockets))
}

/// A skeleton socket (the mesh's skeleton asset: SkeletalMeshSocket), as [bone, loc, rot, scale].
fn socket_of<'a>(ctx: &dyn SceneryBindCtx<'a>, mesh_ref: Option<&Json>, name: &str) -> Result<Json, KindError> {
    let mesh = object_of(ctx, mesh_ref)?;
    let mp = props_of(&mesh);
    if !arr(mp.get("Sockets")).is_empty() {
        return fail(format!("{}: sockets on the mesh", short_name(mesh.path())));
    }
    let skel = object_of(ctx, mp.get("Skeleton"))?;
    let mut found = None;
    for x in skel.inner()? {
        if x.class().ends_with(".SkeletalMeshSocket") {
            let p = props_of(&x);
            if is_str(p.get("SocketName"), name) {
                found = Some(p);
                break;
            }
        }
    }
    let Some(s) = found else { return fail(format!("{}: no socket {name}", short_name(mesh.path()))) };
    let rot = s.get("RelativeRotation");
    let rot = if truthy(rot) {
        Json::Arr(["Pitch", "Yaw", "Roll"].map(|k| num(number(rot.and_then(|r| r.get(k))))).into())
    } else {
        nums(&[0.0; 3])
    };
    Ok(obj([
        ("bone", s.get("BoneName").cloned()),
        ("loc", Some(xyz(s.get("RelativeLocation"), [0.0; 3]))),
        ("rot", Some(rot)),
        ("scale", Some(xyz(s.get("RelativeScale"), [1.0; 3]))),
    ]))
}

// The level's two BatcaveBatsBP_C, as the renderer spawns their emitters from the sim's stamps:
// BatsWarningVisuals @2217 and BatsMainVisuals @2368 each SpawnEmitterAttached(PS_BatcaveBats_Warning /
// PS_BatcaveBats_Full, BoxOfBatsBottom, None, (0,0,-1550), rot 0, scale (1,1,1.2), KeepRelativeOffset, bAutoDestroy):
// the box's world transform composed with that offset (the box does not move). The warning is destroyed by the
// Delay(4.5) @2313 (resuming @15: K2_DestroyComponent), the full swarm deactivated by the Delay(BatsDuration) @2464
// (@49) and destroyed 3 s later (@85 Delay(3.0) -> @179). BatsDuration: the instance's, else the class default's.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // `!(x > 0)` is true for NaN
fn bats_of<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<Vec<Json>, KindError> {
    let mut out = Vec::new();
    for owner in ctx.level().objects.iter().filter(|x| x.class().ends_with(".BatcaveBatsBP_C")) {
        let Some(b) = level_obj(ctx, owner, "BoxOfBatsBottom") else {
            return fail(format!("{}: no BoxOfBatsBottom", name_of(owner)));
        };
        let offset = Xform { t: [0.0, 0.0, -1550.0], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.2] };
        let world = world_of(&ctx.level().objects, b, &props_of_actor(ctx, owner)?)?;
        let t = xf_out(&xf_mul(&offset, &world));
        let vars = vars_of(ctx, owner)?;
        let duration = number(vars.get("BatsDuration"));
        if !(duration > 0.0) {
            return fail(format!("{}: BatsDuration", name_of(owner)));
        }
        out.push(Json::object([
            ("actor", Json::from(name_of(owner))),
            ("warn", Json::from(ctx.add_system(&at(BATS, "PS_BatcaveBats_Warning"))?)),
            ("full", Json::from(ctx.add_system(&at(BATS, "PS_BatcaveBats_Full"))?)),
            ("loc", nums(&t.loc)),
            ("rot", nums(&t.rot)),
            ("scale", nums(&t.scale)),
            ("warnLife", num(4.5)),
            ("duration", num(duration)),
            ("killAfter", num(3.0)),
        ]));
    }
    Ok(out)
}

// The level's two FogOcclusionBP_C: ActivateFogVisuals @6992 spawns PS_SmokeNew (@7110; PS_SmokeSharp_Mobile on a
// mobile feature level only) at DefaultSceneRoot's location + VisualOffset, rot 0, scale (1.25,1.15,1), auto destroy,
// deactivating the one before (@7507), and posts sfx_map_batcave_steam_occlusion on the fog (@7562);
// DeactivateFogVisuals @6586 posts mix_stop_all_fade_onesecond on the fog (@6704), and with Dissolve sets the
// particle's Dissolve 1 (@6822), deactivates it (@6876) and destroys it 1.5 s later (@6926 -> @15); without, destroys
// it at once.
fn fogs_of<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<Vec<Json>, KindError> {
    let smoke = at(&format!("{MAP}VFX/"), "PS_SmokeNew");
    let mut out = Vec::new();
    for owner in ctx.level().objects.iter().filter(|x| x.class().ends_with(".FogOcclusionBP_C")) {
        let Some(root) = level_obj(ctx, owner, "DefaultSceneRoot") else {
            return fail(format!("{}: no DefaultSceneRoot", name_of(owner)));
        };
        let t = world_of(&ctx.level().objects, root, &props_of_actor(ctx, owner)?)?.t;
        let vars = vars_of(ctx, owner)?;
        let o = vec(vars.get("VisualOffset"), [0.0; 3]);
        out.push(Json::object([
            ("actor", Json::from(name_of(owner))),
            ("system", Json::from(ctx.add_system(&smoke)?)),
            ("loc", nums(&[t[0] + o[0], t[1] + o[1], t[2] + o[2]])),
            ("rot", nums(&[0.0; 3])),
            ("scale", nums(&[1.25, 1.15, 1.0])),
            // (the sounds' place: the fog's own AkComponent, at its root)
            ("root", nums(&t)),
            ("sound", Json::from("sfx_map_batcave_steam_occlusion")),
            ("stop", Json::from("mix_stop_all_fade_onesecond")),
            ("destroyAfter", num(1.5)),
        ]));
    }
    Ok(out)
}

// ---- what moves on its own ----
// The fans' spin and speed-up and the level script's calls of it: per fan in the sim's VentsState.fans order
// (the left cycle's Batcave_FanBlades_2, then the right's Batcave_FanBlades2), the
// actor's rates, the StartSpeedup arguments the level script passes (the vent air's place), the SpeedUp timeline's
// curve and length, and PS_Batcave_VentAir.
// - BeginPlay @176: AdjustedRotationRate = RotationRate. ReceiveTick @208..505: Rotation -= DeltaSeconds x
//   AdjustedRotationRate, + 360 when <= -360, the blade's relative rotation MakeRotator(Rotation, 0, 0) (a roll).
// - StartSpeedup (-> @562): SpawnEmitterAtLocation(PS_Batcave_VentAir, EmitterLocation, EmitterRotation, scale 1, auto
//   destroy), then @139 SpeedUp.PlayFromStart. StopSpeedup (-> @632) -> @102 SpeedUp.ReverseFromEnd. The update @15:
//   AdjustedRotationRate = Lerp(RotationRate, SmokeRotationRate, Alpha) (@70); the finish @10 does nothing (goto 637).
// - Map_Batcave: TurnOnLeftFog @1190 Batcave_FanBlades_2.StartSpeedup(FanBladesEmitterPosition,
//   FanBladeEmitterRotation), @68 its StopSpeedup; TurnOnRightFog @5106 Batcave_FanBlades2.StartSpeedup(
//   FanBlade2EmitterPosition, FanBlade2EmitterRotation), @284 its StopSpeedup (Default__Map_Batcave_C's values).
const FAN_SIDES: [(&str, &str, &str); 2] = [
    ("Batcave_FanBlades_2", "FanBladesEmitterPosition", "FanBladeEmitterRotation"),
    ("Batcave_FanBlades2", "FanBlade2EmitterPosition", "FanBlade2EmitterRotation"),
];

struct MotionEmitter {
    actor: &'static str,
    loc: Json,
    rot: Json,
    smoke_rate: Option<Json>,
}

struct MotionLevel {
    speed_up: Json,
    vent_air: String,
    emitters: Vec<MotionEmitter>,
}

fn motion_level<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<MotionLevel, KindError> {
    let tl = props_of(&class_inner(ctx, FAN_CLASS, "SpeedUp_Template")?);
    let tracks = tl.get("FloatTracks").and_then(Json::as_array);
    if tracks.map(<[Json]>::len) != Some(1)
        || ["EventTracks", "VectorTracks", "LinearColorTracks", "bLoop", "LengthMode"]
            .iter()
            .any(|k| truthy(tl.get(*k)))
    {
        return fail("Batcave_FanBlades: SpeedUp changed");
    }
    let curve_obj = object_of(ctx, tracks.and_then(|t| t[0].get("CurveFloat")))?;
    let curve_props = props_of(&curve_obj);
    let curve = req_arr("batcave", curve_props.get("FloatCurve").and_then(|c| c.get("Keys")), "the SpeedUp curve")?;
    if curve.iter().any(|k| !is_str(k.get("InterpMode"), "RCIM_Linear")) {
        return fail("Batcave_FanBlades: SpeedUp curve not linear");
    }
    let length = tl.get("TimelineLength");
    let cdo = cdo_props(ctx, LEVEL_CLASS)?;
    let fan_cdo = cdo_props(ctx, FAN_CLASS)?;
    let mut emitters = Vec::new();
    for (side, loc, rot) in FAN_SIDES {
        let Some(actor) = ctx.level().objects.iter().find(|x| x.class() == FAN_CLASS && name_of(x) == side) else {
            return fail(format!("no fan {side}"));
        };
        // the placed actor's timeline component: its length the template's, not playing at load
        let timeline = level_obj(ctx, actor, "SpeedUp").and_then(|o| props_of(o).get("TheTimeline").cloned());
        let ok = timeline.as_ref().is_some_and(|t| {
            t.get("Length") == length
                && !truthy(t.get("bPlaying"))
                && !truthy(t.get("Position"))
                && t.get("PlayRate").is_none_or(|v| is_num(Some(v), 1.0))
        });
        if !ok {
            return fail(format!("{side}: SpeedUp component"));
        }
        let (Some(l), Some(r)) =
            (cdo.get(loc).filter(|v| v.as_object().is_some()), cdo.get(rot).filter(|v| v.as_object().is_some()))
        else {
            return fail(format!("{LEVEL}: {loc} / {rot}"));
        };
        let own = props_of(actor);
        emitters.push(MotionEmitter {
            actor: side,
            loc: xyz(Some(l), [0.0; 3]),
            rot: Json::Arr(["Pitch", "Yaw", "Roll"].map(|k| num(number(r.get(k)))).into()),
            smoke_rate: first(own.get("SmokeRotationRate"), None).or(fan_cdo.get("SmokeRotationRate")).cloned(),
        });
    }
    let keys = Json::Arr(curve.iter().map(|k| nums(&[number(k.get("Time")), number(k.get("Value"))])).collect());
    let speed_up = obj([("length", length.cloned()), ("keys", Some(keys))]);
    Ok(MotionLevel { speed_up, vent_air: ctx.add_system(&at(&format!("{MAP}VFX/"), "PS_Batcave_VentAir"))?, emitters })
}

struct Fan {
    actor: String,
    instance: String,
    rotation_rate: Option<Json>,
    rotation: Json,
}

/// The block `batcaveMotion`: fans in the sim's order, each with its blade instance.
fn motion_block(level: Option<&MotionLevel>, fans: &[Fan]) -> Result<JsonProps, KindError> {
    let mut b = JsonProps::new();
    let Some(level) = level else { return Ok(b) };
    let mut out = Vec::new();
    for em in &level.emitters {
        let Some(f) = fans.iter().find(|x| x.actor == em.actor) else { return fail(format!("no fan {}", em.actor)) };
        out.push(obj([
            ("actor", Some(Json::from(f.actor.as_str()))),
            ("instance", Some(Json::from(f.instance.as_str()))),
            ("rotationRate", f.rotation_rate.clone()),
            ("smokeRate", em.smoke_rate.clone()),
            ("rotation", Some(f.rotation.clone())),
            ("emitter", Some(Json::object([("loc", em.loc.clone()), ("rot", em.rot.clone())]))),
        ]));
    }
    if fans.len() != out.len() {
        return fail(format!("{} fans", fans.len()));
    }
    let block = Json::object([
        ("fans", Json::Arr(out)),
        ("speedUp", level.speed_up.clone()),
        ("ventAir", Json::from(level.vent_air.as_str())),
    ]);
    b.insert("batcaveMotion".into(), block);
    Ok(b)
}

pub struct Batcave;

impl SceneryPart for Batcave {
    fn name(&self) -> &'static str {
        "batcave"
    }
    fn blueprints(&self) -> &'static [&'static str] {
        &["BCAVE_ST_main_A_01_BP_C", "Batcave_FanBlades_C", "BatcaveWallDestructible_C"]
    }
    fn levels(&self) -> &'static [&'static str] {
        &["Map_Batcave_C"]
    }
    fn ignore(&self) -> &'static [(&'static str, &'static str)] {
        &[
            (
                "BatcaveBatsBP_C",
                "draws nothing placed: boxes and a child audio actor; its emitters (`bats`: batsOf) are spawned from the sim stamps",
            ),
            (
                "FogOcclusionBP_C",
                "draws nothing placed: a root and an overlap sphere; its smoke (`fogs`: fogsOf) is spawned from the sim state",
            ),
            ("Map_Batcave_Godrays_VFX_C", "a data-only level Blueprint (no graph)"),
            ("Map_BatcaveNew_VFX_C", "a data-only level Blueprint (no graph)"),
        ]
    }
    fn anim_blueprints(&self) -> &'static [&'static str] {
        &[]
    }
    fn gameplay_particles(&self) -> &'static [&'static str] {
        &["BatcaveWallDestructible_C"]
    }
    fn run<'a>(&self) -> Box<dyn SceneryPartRun<'a> + 'a> {
        Box::new(Run::default())
    }
}

#[derive(Default)]
struct Run {
    graph: Option<(String, Json)>,
    fans: Vec<Fan>,
    /// A car by its actor's name: its entry as the block writes it.
    cars: indexmap::IndexMap<String, JsonProps>,
    screens: [Option<String>; 6],
    floor: Option<String>,
    motion: Option<MotionLevel>,
    bats: Option<Vec<Json>>,
    fogs: Option<Vec<Json>>,
}

impl Run {
    fn car_of(&mut self, owner: &ObjView<'_>) -> &mut JsonProps {
        self.cars.entry(name_of(owner).to_owned()).or_insert_with(|| {
            JsonProps::from([
                ("actor".to_owned(), Json::from(name_of(owner))),
                ("skeletal".to_owned(), Json::Null),
                ("splash".to_owned(), Json::Null),
                ("montages".to_owned(), Json::Null),
            ])
        })
    }
}

impl<'a> SceneryPartRun<'a> for Run {
    fn level(&mut self, cls: &str, ctx: &dyn SceneryBindCtx<'a>) -> Result<SceneryLevelRun, KindError> {
        if cls != LEVEL {
            return fail(format!("level {cls}"));
        }
        self.motion = Some(motion_level(ctx)?);
        self.bats = Some(bats_of(ctx)?);
        self.fogs = Some(fogs_of(ctx)?);
        Ok(SceneryLevelRun::default())
    }

    fn instance(&mut self, actor: InstanceActor<'_, 'a>) -> Result<JsonProps, KindError> {
        let mut out = JsonProps::new();
        let Some(k) = SCREENS.iter().position(|s| *s == actor.name()) else { return Ok(out) };
        self.screens[k] = Some(actor.name().to_owned());
        out.insert("batcaveScreen".into(), num(k as f64));
        out.insert("hidden".into(), Json::Bool(true));
        Ok(out)
    }

    /// A hidden StaticMeshActor this level Blueprint shows: the steam screens.
    fn shows(&mut self, actor: &ObjView<'a>) -> Result<Option<JsonProps>, KindError> {
        Ok(SCREENS.contains(&name_of(actor)).then(JsonProps::new))
    }

    fn static_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
        p: &JsonProps,
    ) -> Result<StaticMeshOut, KindError> {
        let mut placed = p.clone();
        placed.extend(ctx.place_of(e, Some(&props_of_actor(ctx, owner)?))?);
        if cls == FLOOR {
            if name_of(e) != "StaticMeshComponent0" || self.floor.is_some() {
                return fail(format!("{}: floor component {}", name_of(owner), name_of(e)));
            }
            self.floor = Some(format!("{}.{}", name_of(owner), name_of(e)));
            return Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: placed, extra: JsonProps::new(), placed: true }));
        }
        if cls == FAN {
            if name_of(e) != "BCAVE_ST_fanBlade_A_01" || truthy(p.get("RelativeRotation")) {
                return fail(format!("{}: fan component {}", name_of(owner), name_of(e)));
            }
            let v = vars_of(ctx, owner)?;
            self.fans.push(Fan {
                actor: name_of(owner).to_owned(),
                instance: format!("{}.{}", name_of(owner), name_of(e)),
                rotation_rate: v.get("RotationRate").cloned(),
                rotation: first(v.get("Rotation"), None).cloned().unwrap_or(num(0.0)),
            });
            let extra = JsonProps::from([("batcaveFan".to_owned(), num((self.fans.len() - 1) as f64))]);
            return Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: placed, extra, placed: true }));
        }
        fail(format!("{}: static mesh component {} of {cls}", name_of(owner), name_of(e)))
    }

    fn particle_system(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        if cls != CAR || name_of(e) != "PS_BatcaveWaterfallForeGround_Splashing" {
            return Ok(false);
        }
        let props = props_of_actor(ctx, owner)?;
        let q = props.get(e)?;
        let label = format!("{}.{}", name_of(owner), name_of(e));
        if q.get("bAutoActivate") == Some(&Json::Bool(false))
            || q.get("bVisible") == Some(&Json::Bool(false))
            || truthy(q.get("bHiddenInGame"))
            || !truthy(q.get("Template"))
            || truthy(q.get("RelativeRotation"))
            || truthy(q.get("RelativeScale3D"))
        {
            return fail(format!("{label}: particle component"));
        }
        let pp = reference(q.get("AttachParent"), "AttachParent")?;
        if !pp.starts_with(&format!("{}.", owner.path())) {
            return fail(format!("{label}: attached to {pp}"));
        }
        let Some(parent) = ctx.level().objects.iter().find(|x| x.path() == pp) else { return fail(format!("no {pp}")) };
        let v = need(q.get("RelativeLocation"), "RelativeLocation")?;
        let system = ctx.add_system(&reference(q.get("Template"), "Template")?)?;
        let socket_name = q.get("AttachSocketName").and_then(Json::as_str).unwrap_or("undefined");
        let socket = socket_of(ctx, props.get(parent)?.get("SkeletalMesh"), socket_name)?;
        let splash = obj([
            ("system", Some(Json::from(system))),
            ("socket", Some(socket)),
            ("loc", Some(Json::Arr(["X", "Y", "Z"].map(|k| num(number(v.get(k)))).into()))),
            ("warmup", Some(first(q.get("WarmupTime"), None).cloned().unwrap_or(num(0.0)))),
        ]);
        self.car_of(owner).insert("splash".into(), splash);
        Ok(true)
    }

    fn skeletal_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        if cls != CAR || name_of(e) != "SkeletalMesh" {
            return Ok(false);
        }
        let props = props_of_actor(ctx, owner)?;
        let p = props.get(e)?.clone();
        let name = format!("{}.{}", name_of(owner), name_of(e));
        if self.graph.is_none() {
            self.graph = Some(batmobile_graph(ctx)?);
        }
        let (idle, anim) = self.graph.clone().unwrap_or_default();
        let opts = SkeletalOpts {
            anim_bp: Some(CAR_ANIMBP.to_owned()),
            play: Some(SkeletalPlay { anim: idle, rate: 1.0 }),
            ..SkeletalOpts::default()
        };
        ctx.add_skeletal(&name, &p, &ctx.place_of(e, Some(&props))?, &opts)?;
        let own = props_of(owner);
        let montage = |k: &str| tail(own.get(k)).map_or(Json::Null, Json::from);
        // the AnimBP's graph, the three montages, the lights and every socket they name (the drawing's)
        let montages = Json::object([
            ("destroy", montage("DestroyMontage")),
            ("damage", montage("DamageMontage")),
            ("respawn", montage("RespawnMontage")),
        ]);
        let mut sockets: IndexSet<String> =
            CAR_LIGHTS_TOP.1.iter().chain(&CAR_LIGHTS_FRONT.1).map(|s| (*s).to_owned()).collect();
        let mut play = |k: &str| -> Result<Json, KindError> {
            let (play, found) = montage_of(ctx, own.get(k), name_of(owner))?;
            sockets.extend(found);
            Ok(play)
        };
        let plays = Json::object([
            ("destroy", play("DestroyMontage")?),
            ("damage", play("DamageMontage")?),
            ("respawn", play("RespawnMontage")?),
        ]);
        let lights = Json::object([
            ("top", Json::object([("system", Json::from(CAR_LIGHTS_TOP.0)), ("sockets", strs(&CAR_LIGHTS_TOP.1))])),
            (
                "front",
                Json::object([("system", Json::from(CAR_LIGHTS_FRONT.0)), ("sockets", strs(&CAR_LIGHTS_FRONT.1))]),
            ),
        ]);
        let mut named = Json::obj();
        for s in &sockets {
            named.set(s.as_str(), socket_of(ctx, p.get("SkeletalMesh"), s)?);
        }
        // the root (Box): the place of the root AkComponent the montages' Ak_OneShot post on
        let box_props = level_obj(ctx, owner, "Box").map(|b| props.get(b)).transpose()?;
        let Some(bp) = box_props
            .filter(|bp| !truthy(bp.get("AttachParent")) && tail(own.get("RootComponent")).as_deref() == Some("Box"))
        else {
            return fail(format!("{}: root", name_of(owner)));
        };
        let l = need(bp.get("RelativeLocation"), "RelativeLocation")?;
        let root = Json::Arr(["X", "Y", "Z"].map(|k| num(number(l.get(k)))).into());
        let c = self.car_of(owner);
        c.insert("skeletal".into(), Json::from(name));
        c.insert("montages".into(), montages);
        c.insert("anim".into(), anim);
        c.insert("plays".into(), plays);
        c.insert("lights".into(), lights);
        c.insert("sockets".into(), named);
        c.insert("root".into(), root);
        Ok(true)
    }

    fn level_blocks(&mut self) -> Result<JsonProps, KindError> {
        motion_block(self.motion.as_ref(), &self.fans)
    }

    fn actor_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut b = JsonProps::new();
        let Some(floor) = &self.floor else { return Ok(b) };
        if self.screens.iter().any(Option::is_none) {
            let shown: Vec<&str> = self.screens.iter().map(|s| s.as_deref().unwrap_or("")).collect();
            return fail(format!("screens {}", shown.join(",")));
        }
        for c in self.cars.values() {
            if !truthy(c.get("skeletal")) || !truthy(c.get("splash")) {
                return fail(format!("car {}", c.get("actor").and_then(Json::as_str).unwrap_or("")));
            }
        }
        let (Some(bats), Some(fogs)) = (&self.bats, &self.fogs) else { return fail("bats / fogs") };
        if bats.len() != 2 || fogs.len() != 2 {
            return fail("bats / fogs");
        }
        let fans = self
            .fans
            .iter()
            .map(|f| {
                obj([
                    ("actor", Some(Json::from(f.actor.as_str()))),
                    ("instance", Some(Json::from(f.instance.as_str()))),
                    ("rotationRate", f.rotation_rate.clone()),
                    ("rotation", Some(f.rotation.clone())),
                ])
            })
            .collect();
        let block = Json::object([
            ("floor", Json::from(floor.as_str())),
            ("fans", Json::Arr(fans)),
            ("cars", Json::Arr(self.cars.values().map(|c| Json::Obj(c.clone())).collect())),
            ("screens", Json::Arr(self.screens.iter().map(|s| Json::from(s.as_deref().unwrap_or(""))).collect())),
            ("bats", Json::Arr(bats.clone())),
            ("fogs", Json::Arr(fogs.clone())),
        ]);
        b.insert("batcave".into(), block);
        Ok(b)
    }

    fn summary(&self) -> String {
        format!(
            ", Batcave floor, {} fans, {} cars, {} screens, {} swarms, {} fogs",
            self.fans.len(),
            self.cars.len(),
            self.screens.iter().flatten().count(),
            self.bats.as_ref().map_or(0, Vec::len),
            self.fogs.as_ref().map_or(0, Vec::len)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(s: &str) -> Json {
        Json::parse(s.as_bytes()).unwrap()
    }

    #[test]
    fn names_and_values() {
        assert_eq!(pkg_name("/Game/A/Batmobile_Hit.Batmobile_Hit"), "Batmobile_Hit");
        assert_eq!(pkg_name("Bare"), "Bare");
        // a struct is `[X ?? 0, ..]` in a notify, `[X, ..]` (null when missing) in a socket
        assert_eq!(v3d(Some(&j(r#"{"X":2}"#)), [9.0; 3]), nums(&[2.0, 0.0, 0.0]));
        assert_eq!(v3d(None, [1.0; 3]), nums(&[1.0; 3]));
        let shown = crate::json::to_vec(&xyz(Some(&j(r#"{"X":2}"#)), [9.0; 3])).unwrap();
        assert_eq!(String::from_utf8(shown).unwrap(), "[2,null,null]");
        assert_eq!(r3(Some(&j(r#"{"Yaw":90}"#))), nums(&[0.0, 90.0, 0.0]));
        // an undefined member is left out
        assert_eq!(obj([("a", Some(num(1.0))), ("b", None)]), j(r#"{"a":1}"#));
        assert!(is_num_or(None, 1.0, 1.0) && !is_num_or(Some(&num(2.0)), 1.0, 1.0));
        assert_eq!(link_id(Some(&j(r#"{"LinkID":3}"#))), Some(3));
        assert_eq!(notify_time(&j(r#"{"LinkValue":1.5,"TriggerTimeOffset":0.25}"#)), 1.75);
    }

    #[test]
    fn blocks_and_summary() {
        let mut run = Run::default();
        assert_eq!(run.summary(), ", Batcave floor, 0 fans, 0 cars, 0 screens, 0 swarms, 0 fogs");
        assert!(run.actor_blocks().unwrap().is_empty() && run.level_blocks().unwrap().is_empty());
    }
}
