//! Cromulons's scenery part (`m009`; Map_M009, Map_M009_small: the keys m009 and m009_small): its own Blueprint actors,
//! every one placed in the gameplay level.
//!
//! This writes what each draws at its place and pose at BeginPlay and keeps, in the block `cromulons`, what the
//! renderer needs to draw them and move them:
//! - Cromulon_Head_C, 7 heads: Head (a single-node Cromulun_Head_Idle_01, looping from BeginPlay; Head2's has
//!   bSavedPlaying false: written paused at 0, the level Blueprints play it once the match has started, Map_M009
//!   @1188: `start` in its entry of `heads`), its material the construction script's dynamic instance of Cromulun_MAT
//!   (UserConstructionScript @0..286: Head.CreateDynamicMaterialInstance(0, GetMaterial, None), Color, Contrast and
//!   UseColorAlpha from the actor's Color / Contrast / UseColor), read from the level's saved MaterialInstanceDynamic:
//!   `heads` (the skeletal mesh, the parameters), and its Comp_Parallax (`parallax` on the skeletal entry).
//! - SchwiftyHelicopter_Blueprint_C, 2: SkeletalMesh (one looping animation) under StaticMeshComponent0
//!   (helicopters_01, bVisible false: not drawn), its Comp_Parallax (`parallax`) and MoveHelicopter's variables
//!   (`helicopters`: the velocity it adds to the parallax start each tick, the bounds and heights of its wrap), and its
//!   AK (`event`), which BeginPlay gives its Audio child's Env_Audio_Emit and starts (@10..225; the child's place is the
//!   stage data's audio emitter of the actor).
//! - CromulonAudioVisualizer_C: the 20 planes its construction script adds (NODE_AddStaticMeshComponent-0_<n>, at the
//!   places the level saved), each with its own dynamic instance of SchwiftyAudioVisualizer_MAT with no parameter set,
//!   the material's own until its Tick sets Scroll. Kept: `visualizer` on each instance (its index) and the material's
//!   preshaders (keepParams), for the renderer's instances by Scroll.
//! - M009_DestructiblePlatformBP_C / _Child_C (the speakers; WallBP_C children, the sim's): SkeletalMesh
//!   (SpeakerSkelMesh, its AnimBP SpeakerSkelMesh_Skeleton_AnimBlueprint_C at the state machine's NoDamage pose:
//!   SpeakerSkelMesh_Anim looping, see `speaker_play`), PS_SchwiftyThruster / PS_SchwiftyThruster1 on its bones Engine
//!   / Engine1 (`speakers`: thrusters, spawned by the renderer's block); the hidden StaticMesh (left_speaker /
//!   right_speaker), the empty StaticMesh1 / StaticMesh2 and the hidden PS_SchwiftyDamageSparks (its place: `sparks`,
//!   shown by the renderer) are not drawn here; the damage drawing's assets: `speakerArt` (speaker_anim_bp,
//!   damage_meshes).
//!
//! The level Blueprints (Map_M009_C, Map_M009_small_C: the cloud groups, Head2's idle, the Schwifty ground's AnimBP
//! inputs) are not read: the gameplay level's, which the core reads only for an entered one.

use crate::json::Json;
use crate::kind::KindError;
use crate::kinds::scenery::part::{
    JsonProps, SceneryBindCtx, SceneryMeshOut, SceneryPart, SceneryPartRun, SkeletalOpts, SkeletalPlay, StaticMeshOut,
};
use crate::objview::{ObjView, short_name};

use super::util::js::{self, at, first, is_str, nn, num_or, number_of, put, put_opt, triple, truthy};
use super::util::{
    cdo_props, component_props, name_of, outer_name, over_props, parallax_of, props_of, props_of_actor, reference,
    rot_of, vec,
};

const HEAD: &str = "Cromulon_Head_C";
const HELICOPTER: &str = "SchwiftyHelicopter_Blueprint_C";
const VISUALIZER: &str = "CromulonAudioVisualizer_C";
const SPEAKERS: [&str; 2] = ["M009_DestructiblePlatformBP_C", "M009_DestructiblePlatformBP_Child_C"];
const PLATFORMS: &str = "/Game/Panda_Main/Maps/M009/TestGeo/Platforms/";
const SPEAKER_ANIMBP_CLASS: &str = "/Game/Panda_Main/Maps/M009/TestGeo/Platforms/SpeakerSkelMesh_Skeleton_AnimBlueprint.SpeakerSkelMesh_Skeleton_AnimBlueprint_C";
const SPEAKER_ANIMBP: &str = "SpeakerSkelMesh_Skeleton_AnimBlueprint_C";
const VISUALIZER_MAT: &str = "/Game/Panda_Main/Maps/M009/VFX/SchwiftyAudioVisualizer_MAT.SchwiftyAudioVisualizer_MAT";
const PLAY_VFX: &str = "/Game/Panda_Main/Blueprints/AnimNotifies/AnimNotify_PlayVFX.AnimNotify_PlayVFX_C";

// The speaker's damage drawing (the child forwards BeginPlay only):
// - BeginPlay @2829..3184: CreateDynamicMaterialInstance(0 / 1 / 2, SpeakerAudioVisulizer_MAT) into DynamicMATs;
// - UpdateMesh @4332, @469..1013: SetSkeletalMesh(NewMesh, reinit), SetMaterial(0, NewMaterial), SetMaterial(1 / 2 / 3,
//   DynamicMATs[0 / 1 / 2]), DynamicMATs[0].break = 1, with Stage2 DynamicMATs[2].break2 = 1; @1063 the sparks shown,
//   PS_SpeakerDamage1 at GetSocketLocation(root);
// - stage 1 @2511: UpdateMesh Multicast(SpeakerSkelMesh_DamageState1, speaker_platform_LightDamage_Mat, false); stage 2
//   @2674: (SpeakerSkelMesh_DamageState3, speaker_platform_HeavyDamage_Mat, true), SpeakerDamageState2Multicast @4932:
//   PS_SpeakerDamage2 attached to SkeletalMesh at Root, (0, 0, 0);
// - Tick @2149..2356: DynamicMATs[i].Scroll = FMax(NormalizeToRange(FrequencyAmount[i], -30, 0), 0.05).
const SPEAKER_VISUALIZER: &str =
    "/Game/Panda_Main/Maps/M009/TestGeo/Platforms/SpeakerAudioVisulizer_MAT.SpeakerAudioVisulizer_MAT";
const DAMAGE: [(&str, &str); 2] = [
    ("SpeakerSkelMesh_DamageState1", "speaker_platform_LightDamage_Mat"),
    ("SpeakerSkelMesh_DamageState3", "speaker_platform_HeavyDamage_Mat"),
];
const DAMAGE1: &str = "/Game/Panda_Main/Maps/M009/VFX/PS_SpeakerDamage1.PS_SpeakerDamage1";
const DAMAGE2: &str = "/Game/Panda_Main/Maps/M009/VFX/PS_SpeakerDamage2.PS_SpeakerDamage2";
const HIT_MONTAGES: [&str; 3] =
    ["Speaker_Hit_Anim_Montage", "Speaker_Hit_Anim_DamageState1_Montage", "Speaker_Hit_Anim_DamageState2_Montage"];
const DESTROY_MONTAGE: &str = "Speaker_Destroyed_Montage";

fn path_of(dir: &str, n: &str) -> String {
    format!("{dir}{n}.{n}")
}

fn fail<T>(msg: String) -> Result<T, KindError> {
    js::fail("m009", msg)
}

fn want(cond: bool, what: &str) -> Result<(), KindError> {
    if cond { Ok(()) } else { fail(format!("speaker AnimBP changed: {what}")) }
}

fn speaker_graph<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<JsonProps, KindError> {
    Ok(props_of(&ctx.load(&format!("{PLATFORMS}SpeakerSkelMesh_Skeleton_AnimBlueprint.Default__{SPEAKER_ANIMBP}"))?))
}

/// The speaker's entry: its state machine's NoDamage sequence (SpeakerSkelMesh_Anim, looping); the renderer's speakers
/// view drives the whole AnimBP from `speakerArt` (`speaker_anim_bp` below).
fn speaker_play<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<SkeletalPlay, KindError> {
    let g = speaker_graph(ctx)?;
    let state = g.iter().any(|(k, v)| k.starts_with("AnimGraphNode_StateResult") && is_str(v.get("Name"), "NoDamage"));
    let player = g.get("AnimGraphNode_SequencePlayer");
    let Some(player) = player.filter(|_| state) else { return fail("speaker AnimBP changed".into()) };
    let seq = reference(player.get("Sequence"), "the speaker sequence")?;
    if short_name(&seq) != "SpeakerSkelMesh_Anim" || player.get("bLoopAnimation") == Some(&Json::Bool(false)) {
        return fail("speaker AnimBP changed".into());
    }
    Ok(SkeletalPlay { anim: seq, rate: num_or(player.get("PlayRate"), 1.0) })
}

/// A component's material override: the level's saved MaterialInstanceDynamic's parent and parameters, or the
/// reference as it is.
struct Dynamic {
    reference: Json,
    scalars: JsonProps,
    vectors: JsonProps,
}

/// A component's material overrides that are the level's saved MaterialInstanceDynamic objects: their parents, and
/// their parameters ({ scalars, vectors } by name).
fn dynamic_of<'a>(ctx: &dyn SceneryBindCtx<'a>, refs: Option<&Json>) -> Result<Vec<Dynamic>, KindError> {
    let mut out = Vec::new();
    for r in js::arr(refs) {
        let owned;
        let mid: Option<&ObjView<'a>> = match r {
            Json::Str(path) => match ctx.level().objects.iter().find(|o| o.path() == path) {
                Some(o) => Some(o),
                None => {
                    owned = ctx.load(path)?;
                    Some(&owned)
                }
            },
            _ => None,
        };
        let Some(mid) = mid.filter(|m| m.class().ends_with(".MaterialInstanceDynamic")) else {
            out.push(Dynamic { reference: r.clone(), scalars: JsonProps::new(), vectors: JsonProps::new() });
            continue;
        };
        let q = props_of(mid);
        if !js::arr(q.get("TextureParameterValues")).is_empty() {
            return fail(format!("{}: texture parameters", mid.path()));
        }
        let mut scalars = JsonProps::new();
        for s in js::arr(q.get("ScalarParameterValues")) {
            let n = at(s.get("ParameterInfo"), "Name").and_then(Json::as_str).unwrap_or("undefined");
            scalars.insert(n.to_owned(), s.get("ParameterValue").cloned().unwrap_or(Json::Null));
        }
        let mut vectors = JsonProps::new();
        for s in js::arr(q.get("VectorParameterValues")) {
            let n = at(s.get("ParameterInfo"), "Name").and_then(Json::as_str).unwrap_or("undefined");
            let v = s.get("ParameterValue");
            vectors.insert(
                n.to_owned(),
                Json::Arr(["R", "G", "B", "A"].iter().map(|k| Json::Num(number_of(at(v, k)))).collect()),
            );
        }
        out.push(Dynamic { reference: q.get("Parent").cloned().unwrap_or(Json::Null), scalars, vectors });
    }
    Ok(out)
}

/// An additive sequence the renderer applies (its cooked delta): a local space one over a frame of its base.
fn additive_of(a: &JsonProps, name: &str) -> Result<(), KindError> {
    if !is_str(a.get("AdditiveAnimType"), "AAT_LocalSpaceBase") || !is_str(a.get("RefPoseType"), "ABPT_AnimFrame") {
        return fail(format!("{name}: additive base"));
    }
    Ok(())
}

/// A montage of the speaker's (the hit montages, Speaker_Destroyed_Montage) as the renderer plays it: its length and
/// blends, the slot track's segments (animation by build object path, montage start, animation start / end, rate) and
/// the PlayVFX notify states (time, end, system, socket, kill), as Tree Fort's `montage_of`.
fn montage_of<'a>(ctx: &dyn SceneryBindCtx<'a>, name: &str) -> Result<Json, KindError> {
    let m = props_of(&ctx.load(&path_of(PLATFORMS, name))?);
    const KNOWN: [&str; 8] = [
        "BlendIn",
        "BlendOut",
        "CompositeSections",
        "SlotAnimTracks",
        "Notifies",
        "SequenceLength",
        "RateScale",
        "Skeleton",
    ];
    let other: Vec<&str> = m.keys().map(String::as_str).filter(|k| !KNOWN.contains(k)).collect();
    let tracks = js::arr(m.get("SlotAnimTracks"));
    let sections = js::arr(m.get("CompositeSections"));
    if !other.is_empty()
        || tracks.len() != 1
        || sections.len() != 1
        || !is_str(sections[0].get("NextSectionName"), "None")
    {
        return fail(format!("{name}: montage {}", other.join(",")));
    }
    let blend = |b: Option<&Json>| -> Result<Json, KindError> {
        match b {
            Some(Json::Obj(o)) if o.len() == 1 && o.contains_key("BlendTime") => Ok(o["BlendTime"].clone()),
            _ => fail(format!("{name}: blend")),
        }
    };
    let cdo = cdo_props(ctx, PLAY_VFX)?;
    let mut notifies = Vec::new();
    for n in js::arr(m.get("Notifies")) {
        if !is_str(n.get("NotifyName"), "PlayVFX_C")
            || !is_str(n.get("MontageTickType"), "EMontageNotifyTickType::Queued")
        {
            return fail(format!(
                "{name}: notify {}",
                n.get("NotifyName").and_then(Json::as_str).unwrap_or("undefined")
            ));
        }
        let notify = reference(first(n.get("Notify"), n.get("NotifyStateClass")), "a notify")?;
        let q = over_props(&cdo, Some(&props_of(&ctx.load(&notify)?)));
        // (ApplyMirroring acts only with IsFacingLeft, which AnimNotify_PlayVFX sets for a BP_FighterCharacter owner:
        // false on the speaker; with no offset and no rotation it changes nothing anyway)
        let zero = |v: Option<&Json>| !truthy(v) || ["X", "Y", "Z"].iter().all(|k| !truthy(at(v, k)));
        let (r, s) = (q.get("Rotation"), q.get("VFX Scale"));
        if truthy(q.get("UseSkinVFX"))
            || truthy(q.get("UseOverrideAttachParent"))
            || q.get("ParticleTeam0") != q.get("ParticleTeam1")
            || !zero(q.get("Location"))
            || !zero(q.get("FaceLeftOffset"))
            || !zero(q.get("FaceRightOffset"))
            || ["Pitch", "Yaw", "Roll"].iter().any(|k| truthy(at(r, k)))
            || ["X", "Y", "Z"].iter().any(|k| number_of(at(s, k)) != 1.0)
        {
            return fail(format!("{name}: PlayVFX"));
        }
        let lv = number_of(n.get("LinkValue"));
        let mut o = JsonProps::new();
        put(&mut o, "at", lv + js::or_num(n.get("TriggerTimeOffset"), 0.0));
        if truthy(n.get("Duration")) {
            put(&mut o, "end", lv + number_of(n.get("Duration")));
        }
        put(&mut o, "system", ctx.add_system(&reference(q.get("ParticleTeam0"), "ParticleTeam0")?)?);
        put_opt(&mut o, "socket", q.get("SocketName"));
        put(&mut o, "kill", truthy(q.get("KillOnNotifyEnd")));
        notifies.push(Json::Obj(o));
    }
    let t = &tracks[0];
    let mut segments = Vec::new();
    for s in js::req_arr("m009", at(t.get("AnimTrack"), "AnimSegments"), "AnimSegments")? {
        let anim = reference(s.get("AnimReference"), "a segment")?;
        if num_or(s.get("LoopingCount"), 1.0) != 1.0 {
            return fail(format!("{anim}: segment"));
        }
        let a = props_of(&ctx.load(&anim)?);
        if !is_str(first(a.get("AdditiveAnimType"), Some(&Json::Str("AAT_None".into()))), "AAT_None") {
            additive_of(&a, short_name(&anim))?;
        }
        let mut o = JsonProps::new();
        put(&mut o, "anim", ctx.anim_path(&anim, false)?);
        put(&mut o, "pos", nn(s.get("StartPos"), 0.0));
        put(&mut o, "start", nn(s.get("AnimStartTime"), 0.0));
        put_opt(&mut o, "end", s.get("AnimEndTime"));
        put(&mut o, "rate", nn(s.get("AnimPlayRate"), 1.0));
        segments.push(Json::Obj(o));
    }
    let mut o = JsonProps::new();
    put(&mut o, "montage", name);
    put_opt(&mut o, "slot", t.get("SlotName"));
    put_opt(&mut o, "length", m.get("SequenceLength"));
    put(&mut o, "rateScale", nn(m.get("RateScale"), 1.0));
    put(&mut o, "blendIn", blend(m.get("BlendIn"))?);
    put(&mut o, "blendOut", blend(m.get("BlendOut"))?);
    put(&mut o, "segments", Json::Arr(segments));
    put(&mut o, "notifies", Json::Arr(notifies));
    Ok(Json::Obj(o))
}

/// The speaker's AnimBP (SpeakerSkelMesh_Skeleton_AnimBlueprint_C) as the renderer evaluates it. The node links (a
/// LinkID is the node's place among the class default's AnimGraphNode_ properties; the baked state machine numbers its
/// nodes the other way round):
///   Root <- Slot "Override" <- ApplyAdditive(alpha 1, a bool alpha that is always on)
///     Base: ComponentToLocal <- ModifyBone (Speaker_Visulizer) <- _1 (Speaker_Visulizer4) <- _2 (3) <- _3 (2) <- _4 (1)
///           <- LocalToComponent <- LayeredBoneBlend(base: the state machine DamageStates; blend 1: Speaker_Engine_Shake,
///              looping, on Engine and Engine1 at depth 0, local space)
///     Additive: Slot "DefaultSlot" <- Speaker_Idle_Additive (not looping)
///   DamageStates: NoDamage (SpeakerSkelMesh_Anim, looping) -> DamageState1 (Speaker_Damage_State_1, once) when
///   DamageState > 0 -> DamageState2 (Speaker_Damage_State_2, once) when DamageState > 1; crossfades 0.
///   The ModifyBone nodes: translation and scale replaced in bone space, rotation ignored; Speaker_Visulizer and _1 take
///   TheBeat / SpeakerScale, _2.._4 SmallBeat / SmallSpeakerScale (the ubergraph's exposed inputs @10 .. @807).
/// The transition rules: DamageState > 0 (@968) and > 1 (@1035), DamageState = the owner's CurrentStage (@270).
fn speaker_anim_bp<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<Json, KindError> {
    let g = speaker_graph(ctx)?;
    let nodes: Vec<&str> = g.keys().map(String::as_str).filter(|k| k.starts_with("AnimGraphNode_")).collect();
    let node = |n: &str| g.get(n);
    let link_of = |v: Option<&Json>| -> &str {
        let i = number_of(at(v, "LinkID"));
        if i >= 0.0 { nodes.get(i as usize).copied().unwrap_or("") } else { "" }
    };
    let pin = |n: &str, p: &str| link_of(at(node(n), p));
    want(
        pin("AnimGraphNode_Root", "Result") == "AnimGraphNode_Slot"
            && is_str(at(node("AnimGraphNode_Slot"), "SlotName"), "Override"),
        "root",
    )?;
    want(pin("AnimGraphNode_Slot", "Source") == "AnimGraphNode_ApplyAdditive", "override source")?;
    let add = node("AnimGraphNode_ApplyAdditive");
    want(
        number_of(at(add, "Alpha")) == 1.0
            && is_str(at(add, "AlphaInputType"), "EAnimAlphaInputType::Bool")
            && truthy(at(add, "bAlphaBoolEnabled"))
            && number_of(at(at(add, "AlphaBoolBlend"), "BlendInTime")) == 0.0,
        "additive alpha",
    )?;
    want(
        pin("AnimGraphNode_ApplyAdditive", "Additive") == "AnimGraphNode_Slot_1"
            && is_str(at(node("AnimGraphNode_Slot_1"), "SlotName"), "DefaultSlot"),
        "additive input",
    )?;
    want(pin("AnimGraphNode_Slot_1", "Source") == "AnimGraphNode_SequencePlayer_3", "default slot source")?;
    // the ModifyBone chain from ComponentToLocal down to LocalToComponent
    want(pin("AnimGraphNode_ApplyAdditive", "Base") == "AnimGraphNode_ComponentToLocalSpace", "additive base")?;
    let mut modify: Vec<Json> = Vec::new();
    let mut n = pin("AnimGraphNode_ComponentToLocalSpace", "ComponentPose");
    let large = |n: &str| -> Option<bool> {
        match n {
            "AnimGraphNode_ModifyBone" | "AnimGraphNode_ModifyBone_4" => Some(true),
            "AnimGraphNode_ModifyBone_1" | "AnimGraphNode_ModifyBone_2" | "AnimGraphNode_ModifyBone_3" => Some(false),
            _ => None,
        }
    };
    while n.starts_with("AnimGraphNode_ModifyBone") {
        let b = node(n);
        want(
            is_str(at(b, "TranslationMode"), "BMM_Replace")
                && is_str(at(b, "ScaleMode"), "BMM_Replace")
                && is_str(at(b, "RotationMode"), "BMM_Ignore")
                && is_str(at(b, "TranslationSpace"), "BCS_BoneSpace")
                && is_str(at(b, "ScaleSpace"), "BCS_BoneSpace")
                && number_of(at(b, "Alpha")) == 1.0
                && is_str(at(b, "AlphaInputType"), "EAnimAlphaInputType::Float")
                && large(n).is_some(),
            n,
        )?;
        let mut m = JsonProps::new();
        put_opt(&mut m, "bone", at(at(b, "BoneToModify"), "BoneName"));
        put(&mut m, "large", large(n).unwrap_or(false));
        modify.insert(0, Json::Obj(m));
        n = pin(n, "ComponentPose");
    }
    want(
        modify.len() == 5
            && n == "AnimGraphNode_LocalToComponentSpace"
            && pin(n, "LocalPose") == "AnimGraphNode_LayeredBoneBlend",
        "modify chain",
    )?;
    let lb = node("AnimGraphNode_LayeredBoneBlend");
    let poses = js::arr(at(lb, "BlendPoses"));
    want(
        pin("AnimGraphNode_LayeredBoneBlend", "BasePose") == "AnimGraphNode_StateMachine"
            && poses.len() == 1
            && link_of(Some(&poses[0])) == "AnimGraphNode_SequencePlayer_4"
            && number_of(js::arr(at(lb, "BlendWeights")).first()) == 1.0
            && !truthy(at(lb, "bMeshSpaceRotationBlend"))
            && !truthy(at(lb, "bMeshSpaceScaleBlend")),
        "layered blend",
    )?;
    let filters = js::arr(at(js::arr(at(lb, "LayerSetup")).first(), "BranchFilters"));
    want(filters.iter().all(|f| number_of(f.get("BlendDepth")) == 0.0), "layered blend depth")?;
    // the sequence players: the asset by build object path, looping or not, rate 1, from 0
    let player = |name: &str| -> Result<JsonProps, KindError> {
        let p = node(name);
        want(
            number_of(at(p, "PlayRate")) == 1.0
                && number_of(at(p, "StartPosition")) == 0.0
                && num_or(at(p, "PlayRateBasis"), 1.0) == 1.0,
            name,
        )?;
        let mut o = JsonProps::new();
        put(&mut o, "anim", ctx.anim_path(&reference(at(p, "Sequence"), name)?, false)?);
        put(&mut o, "loop", at(p, "bLoopAnimation") != Some(&Json::Bool(false)));
        Ok(o)
    };
    // the state machine (BakedStateMachines: states in order, their player, the transitions' rules)
    let class_props = props_of(&ctx.load(SPEAKER_ANIMBP_CLASS)?);
    let sm = js::arr(class_props.get("BakedStateMachines")).first();
    let states = js::arr(at(sm, "States"));
    let transitions = js::arr(at(sm, "Transitions"));
    let names: Vec<&str> =
        states.iter().map(|s| s.get("StateName").and_then(Json::as_str).unwrap_or("undefined")).collect();
    want(
        number_of(at(sm, "InitialState")) == 0.0 && names.join(",") == "NoDamage,DamageState1,DamageState2",
        "states",
    )?;
    want(
        transitions.len() == 2
            && transitions.iter().enumerate().all(|(k, t)| {
                number_of(t.get("PreviousState")) == k as f64
                    && number_of(t.get("NextState")) == (k + 1) as f64
                    && number_of(t.get("CrossfadeDuration")) == 0.0
                    && is_str(t.get("LogicType"), "ETransitionLogicType::TLT_StandardBlend")
            }),
        "transitions",
    )?;
    let machine = node("AnimGraphNode_StateMachine");
    want(
        at(machine, "bSkipFirstUpdateTransition") == Some(&Json::Bool(true))
            && number_of(at(machine, "MaxTransitionsPerFrame")) >= 2.0,
        "state machine",
    )?;
    let mut players = Vec::new();
    for s in states {
        let idx = js::arr(s.get("PlayerNodeIndices"));
        let state_name = s.get("StateName").and_then(Json::as_str).unwrap_or("undefined");
        want(idx.len() == 1 && !truthy(s.get("bAlwaysResetOnEntry")), state_name)?;
        // (the baked index counts the nodes from the end)
        let i = number_of(idx.first());
        let name = if i >= 0.0 && (i as usize) < nodes.len() { nodes[nodes.len() - 1 - i as usize] } else { "" };
        players.push(Json::Obj(player(name)?));
    }
    let idle = player("AnimGraphNode_SequencePlayer_3")?;
    let idle_seq = reference(at(node("AnimGraphNode_SequencePlayer_3"), "Sequence"), "the speaker idle")?;
    additive_of(&props_of(&ctx.load(&idle_seq)?), "the speaker idle")?;
    let mut hit = JsonProps::new();
    for m in HIT_MONTAGES {
        hit.insert(m.to_owned(), montage_of(ctx, m)?);
    }
    let mut shake = player("AnimGraphNode_SequencePlayer_4")?;
    put(
        &mut shake,
        "bones",
        Json::Arr(filters.iter().map(|f| f.get("BoneName").cloned().unwrap_or(Json::Null)).collect()),
    );
    let mut out = JsonProps::new();
    put(&mut out, "states", Json::Arr(players));
    put(&mut out, "shake", Json::Obj(shake));
    put(&mut out, "idle", Json::Obj(idle));
    put(&mut out, "modify", Json::Arr(modify));
    put(&mut out, "hit", Json::Obj(hit));
    put(&mut out, "destroy", montage_of(ctx, DESTROY_MONTAGE)?);
    Ok(Json::Obj(out))
}

/// The meshes UpdateMesh swaps in, with the materials it leaves on them: NewMaterial on slot 0, the visualizer's
/// dynamic instances on 1..3 (the renderer's own, `dynamic`), the mesh's own past them (SpeakerSkelMesh_DamageState3's
/// lambert1).
fn damage_meshes<'a>(ctx: &dyn SceneryBindCtx<'a>, name: &str) -> Result<Json, KindError> {
    let mut out = Vec::new();
    for (mesh, mat) in DAMAGE {
        let h = ctx.mesh_header(&path_of(PLATFORMS, mesh))?;
        if h.lods[0].sections.iter().enumerate().any(|(i, x)| x.slot != i as f64) {
            return fail(format!("{mesh}: section materials"));
        }
        let mut materials = Vec::new();
        for (i, x) in h.slots.iter().enumerate() {
            let m = if i == 0 {
                Some(path_of(PLATFORMS, mat))
            } else if i <= 3 {
                Some(SPEAKER_VISUALIZER.to_owned())
            } else {
                x.material.clone().filter(|m| !m.is_empty())
            };
            materials.push(match m {
                Some(m) => ctx.skin_material(&m, name)?.map_or(Json::Null, Json::Str),
                None => Json::Null,
            });
        }
        let mut o = JsonProps::new();
        put(&mut o, "mesh", path_of(PLATFORMS, mesh));
        put(&mut o, "materials", Json::Arr(materials));
        out.push(Json::Obj(o));
    }
    Ok(Json::Arr(out))
}

/// Comp_Parallax on a Blueprint actor ([Y, Z]).
fn parallax_of_actor<'a>(ctx: &dyn SceneryBindCtx<'a>, owner: &ObjView<'a>) -> Result<JsonProps, KindError> {
    let prefix = format!("{}.", owner.path());
    let c = ctx.level().objects.iter().find(|x| {
        x.class().ends_with(".Comp_Parallax_C") && outer_name(x) == name_of(owner) && x.path().starts_with(&prefix)
    });
    match c {
        Some(c) => parallax_of(&component_props(ctx, c)?, ctx),
        None => Ok(JsonProps::new()),
    }
}

pub struct Cromulons;

impl SceneryPart for Cromulons {
    fn name(&self) -> &'static str {
        "m009"
    }
    fn blueprints(&self) -> &'static [&'static str] {
        &[
            "Cromulon_Head_C",
            "SchwiftyHelicopter_Blueprint_C",
            "CromulonAudioVisualizer_C",
            "M009_DestructiblePlatformBP_C",
            "M009_DestructiblePlatformBP_Child_C",
        ]
    }
    // the planes' material: the visualizer's Tick sets Scroll on each plane's dynamic instance (the renderer's block)
    fn keep_params(&self) -> &'static [&'static str] {
        &["/Game/Panda_Main/Maps/M009/VFX/SchwiftyAudioVisualizer_MAT.SchwiftyAudioVisualizer_MAT"]
    }
    fn run<'a>(&self) -> Box<dyn SceneryPartRun<'a> + 'a> {
        Box::new(CromulonRun::default())
    }
}

/// One bind's state.
#[derive(Default)]
struct CromulonRun {
    heads: Vec<Json>,
    helicopters: Vec<Json>,
    speakers: Vec<Json>,
    speaker_art: Option<Json>,
    planes: usize,
}

/// `^NODE_AddStaticMeshComponent-0(?:_(\d+))?$`: the plane's number after the first, or `None` for the first; the
/// outer `None` when the name is not one.
fn plane_name(n: &str) -> Option<Option<u32>> {
    let rest = n.strip_prefix("NODE_AddStaticMeshComponent-0")?;
    if rest.is_empty() {
        return Some(None);
    }
    let digits = rest.strip_prefix('_')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().map(Some)
}

impl<'a> SceneryPartRun<'a> for CromulonRun {
    fn skeletal_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        let props = props_of_actor(ctx, owner)?;
        let p = props.get(e)?.clone();
        if cls == HEAD {
            let mut mids = dynamic_of(ctx, p.get("OverrideMaterials"))?.into_iter();
            let (Some(mid), None) = (mids.next(), mids.next()) else {
                return fail(format!("{}: head materials", owner.path()));
            };
            if name_of(e) != "Head" {
                return fail(format!("{}: head materials", owner.path()));
            }
            let paused = at(p.get("AnimationData"), "bSavedPlaying") == Some(&Json::Bool(false));
            let mut comp = p.clone();
            comp.insert("OverrideMaterials".into(), Json::Arr(vec![mid.reference.clone()]));
            let place = ctx.place_of(e, Some(&props))?;
            let fields = parallax_of_actor(ctx, owner)?;
            ctx.add_skeletal(
                name_of(owner),
                &comp,
                &place,
                &SkeletalOpts { paused, fields: Some(fields), ..Default::default() },
            )?;
            let mut rec = JsonProps::new();
            put(&mut rec, "skeletal", name_of(owner));
            if paused {
                put(&mut rec, "start", true);
            }
            let skin = ctx.skin_material(&reference(Some(&mid.reference), "the head material")?, name_of(owner))?;
            put(&mut rec, "material", skin.map_or(Json::Null, Json::Str));
            put(&mut rec, "scalars", Json::Obj(mid.scalars));
            put(&mut rec, "vectors", Json::Obj(mid.vectors));
            self.heads.push(Json::Obj(rec));
            return Ok(true);
        }
        if cls == HELICOPTER {
            let mut v = cdo_props(ctx, owner.class())?;
            for (k, x) in props_of(owner) {
                v.insert(k, x);
            }
            let place = ctx.place_of(e, Some(&props))?;
            let fields = parallax_of_actor(ctx, owner)?;
            ctx.add_skeletal(name_of(owner), &p, &place, &SkeletalOpts { fields: Some(fields), ..Default::default() })?;
            let mut rec = JsonProps::new();
            put(&mut rec, "skeletal", name_of(owner));
            put(&mut rec, "velocity", triple(vec(v.get("HelicopterVelocity"), [0.0; 3])));
            put(&mut rec, "height", Json::Arr(vec![nn(v.get("HeightMin"), 0.0), nn(v.get("HeightMax"), 0.0)]));
            put(&mut rec, "bounds", Json::Arr(vec![nn(v.get("HeliLeftBound"), 0.0), nn(v.get("HeliRightBound"), 0.0)]));
            if let Some(Json::Str(ak)) = v.get("AK") {
                put(&mut rec, "event", short_name(ak));
            }
            self.helicopters.push(Json::Obj(rec));
            return Ok(true);
        }
        if SPEAKERS.contains(&cls) {
            if name_of(e) != "SkeletalMesh" {
                return Ok(false);
            }
            let prefix = format!("{}.", owner.path());
            let mine: Vec<&ObjView<'a>> = ctx
                .level()
                .objects
                .iter()
                .filter(|x| x.path().starts_with(&prefix) && outer_name(x) == name_of(owner))
                .collect();
            // the thrusters on its bones (the mesh has no sockets: AttachSocketName is a bone), their place under it
            let mut thrusters = Vec::new();
            for x in mine.iter().filter(|o| {
                o.class().ends_with(".ParticleSystemComponent") && name_of(o).starts_with("PS_SchwiftyThruster")
            }) {
                let q = props.get(x)?;
                if q.get("bVisible") == Some(&Json::Bool(false)) || !truthy(q.get("AttachSocketName")) {
                    return fail(format!("{}.{}: thruster", owner.path(), name_of(x)));
                }
                let mut t = JsonProps::new();
                put(&mut t, "system", ctx.add_system(&reference(q.get("Template"), "a thruster")?)?);
                put_opt(&mut t, "bone", q.get("AttachSocketName"));
                put(&mut t, "loc", triple(vec(q.get("RelativeLocation"), [0.0; 3])));
                put(&mut t, "rot", triple(rot_of(q.get("RelativeRotation"))));
                put(&mut t, "scale", triple(vec(q.get("RelativeScale3D"), [1.0; 3])));
                thrusters.push(Json::Obj(t));
            }
            let sp = mine.iter().find(|x| name_of(x) == "PS_SchwiftyDamageSparks").copied();
            let sq = match sp {
                Some(sp) => Some(props.get(sp)?),
                None => None,
            };
            let (Some(sp), Some(sq)) = (sp, sq) else {
                return fail(format!("{}: speaker effects", owner.path()));
            };
            if thrusters.len() != 2 || sq.get("bVisible") != Some(&Json::Bool(false)) {
                return fail(format!("{}: speaker effects", owner.path()));
            }
            let play = speaker_play(ctx)?;
            ctx.add_skeletal(
                name_of(owner),
                &p,
                &ctx.place_of(e, Some(&props))?,
                &SkeletalOpts { anim_bp: Some(SPEAKER_ANIMBP.to_owned()), play: Some(play), ..Default::default() },
            )?;
            // the sparks' place (under CollisionBox), the damage drawing's assets once for both
            let w = ctx.place_of(sp, Some(&props))?;
            let mut sparks = JsonProps::new();
            put(&mut sparks, "system", ctx.add_system(&reference(sq.get("Template"), "the sparks")?)?);
            put(&mut sparks, "loc", triple(vec(w.get("RelativeLocation"), [0.0; 3])));
            put(&mut sparks, "rot", triple(rot_of(w.get("RelativeRotation"))));
            put(&mut sparks, "scale", triple(vec(w.get("RelativeScale3D"), [1.0; 3])));
            if self.speaker_art.is_none() {
                let visualizer = ctx.skin_material(SPEAKER_VISUALIZER, name_of(owner))?;
                let mut art = JsonProps::new();
                put(&mut art, "visualizer", visualizer.map_or(Json::Null, Json::Str));
                put(&mut art, "base", triple([0.0, 1.0, 2.0]));
                put(&mut art, "damaged", triple([1.0, 2.0, 3.0]));
                put(&mut art, "damage", damage_meshes(ctx, name_of(owner))?);
                put(&mut art, "stage1", ctx.add_system(DAMAGE1)?);
                put(&mut art, "stage2", ctx.add_system(DAMAGE2)?);
                put(&mut art, "animBP", speaker_anim_bp(ctx)?);
                self.speaker_art = Some(Json::Obj(art));
            }
            let mut rec = JsonProps::new();
            put(&mut rec, "skeletal", name_of(owner));
            put(&mut rec, "child", cls == SPEAKERS[1]);
            put(&mut rec, "thrusters", Json::Arr(thrusters));
            put(&mut rec, "sparks", Json::Obj(sparks));
            self.speakers.push(Json::Obj(rec));
            return Ok(true);
        }
        Ok(false)
    }

    fn static_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
        p: &JsonProps,
    ) -> Result<StaticMeshOut, KindError> {
        if cls == VISUALIZER {
            let mut mids = dynamic_of(ctx, p.get("OverrideMaterials"))?.into_iter();
            let (Some(mid), None) = (mids.next(), mids.next()) else {
                return fail(format!("{}.{}: visualizer material", owner.path(), name_of(e)));
            };
            if mid.reference != Json::Str(VISUALIZER_MAT.into()) || !mid.scalars.is_empty() || !mid.vectors.is_empty() {
                return fail(format!("{}.{}: visualizer material", owner.path(), name_of(e)));
            }
            // the plane's index in the construction script's order (CromulonAudioVisualizer UCS @5, j =
            // 1..NumberOfBeats; the Tick walks BeatMeshes in it): NODE_AddStaticMeshComponent-0 is 0, -0_k is k + 1,
            // not the export order
            let Some(k) = plane_name(name_of(e)) else {
                return fail(format!("{}.{}: visualizer plane name", owner.path(), name_of(e)));
            };
            self.planes += 1;
            let mut out = p.clone();
            for (key, v) in ctx.place_of(e, Some(&props_of_actor(ctx, owner)?))? {
                out.insert(key, v);
            }
            out.insert("OverrideMaterials".into(), Json::Arr(vec![mid.reference]));
            let mut extra = JsonProps::new();
            put(&mut extra, "visualizer", k.map_or(0.0, |k| f64::from(k) + 1.0));
            return Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: out, extra, placed: true }));
        }
        // StaticMeshComponent0 (helicopters_01) and the speakers' StaticMesh: hidden, never shown; StaticMesh1 / 2: no mesh
        if (cls == HELICOPTER || SPEAKERS.contains(&cls))
            && (p.get("bVisible") == Some(&Json::Bool(false)) || !truthy(p.get("StaticMesh")))
        {
            return Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: p.clone(), extra: JsonProps::new(), placed: true }));
        }
        fail(format!("{}: static mesh component {} of {cls}", owner.path(), name_of(e)))
    }

    fn level_blocks(&mut self) -> Result<JsonProps, KindError> {
        Ok(JsonProps::new())
    }

    fn actor_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut out = JsonProps::new();
        if !self.heads.is_empty() || !self.helicopters.is_empty() || !self.speakers.is_empty() || self.planes > 0 {
            let mut c = JsonProps::new();
            put(&mut c, "heads", Json::Arr(self.heads.clone()));
            put(&mut c, "helicopters", Json::Arr(self.helicopters.clone()));
            put(&mut c, "speakers", Json::Arr(self.speakers.clone()));
            put(&mut c, "planes", self.planes as f64);
            if let Some(a) = &self.speaker_art {
                put(&mut c, "speakerArt", a.clone());
            }
            put(&mut out, "cromulons", Json::Obj(c));
        }
        Ok(out)
    }

    fn summary(&self) -> String {
        if self.heads.is_empty() {
            return String::new();
        }
        format!(
            ", {} heads, {} helicopters, {} speakers, {} visualizer planes",
            self.heads.len(),
            self.helicopters.len(),
            self.speakers.len(),
            self.planes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visualizer_plane_names() {
        assert_eq!(plane_name("NODE_AddStaticMeshComponent-0"), Some(None));
        assert_eq!(plane_name("NODE_AddStaticMeshComponent-0_19"), Some(Some(19)));
        assert_eq!(plane_name("NODE_AddStaticMeshComponent-0_"), None);
        assert_eq!(plane_name("NODE_AddStaticMeshComponent-1"), None);
        assert_eq!(plane_name("NODE_AddStaticMeshComponent-0_x"), None);
    }

    #[test]
    fn the_speakers_idle_when_there_are_no_heads() {
        let run = CromulonRun::default();
        assert_eq!(<CromulonRun as SceneryPartRun<'_>>::summary(&run), "");
    }
}
