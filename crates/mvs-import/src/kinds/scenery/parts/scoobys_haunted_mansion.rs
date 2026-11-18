//! Scooby's Haunted Mansion's scenery part (`scooby`; Map_ScoobyDoo, Map_ScoobyDoo_NoRoof: the keys scoobydoo and
//! scoobydoo_noroof): its own Blueprint actors.
//!
//! This writes what each draws at its place and pose at BeginPlay, standing still; the block `scooby` names the pieces
//! the sim moves (the switches', trapdoors', walls', gears' and chains' plays and effects):
//! - BP_ScoobyWeb_C (BP_ScoobyWeb, UserConstructionScript @0..332: ScoobyWeb.SetStaticMesh(New Mesh[MeshChoice]),
//!   ScoobyWeb.CreateDynamicMaterialInstance(0, SCOOB_Webs_A_01_M), SetScalarParameterValue(Emissive_Mult, Emissive_Mult)
//!   and (WPO, WPO)), the Background levels' webs and No Roof's gameplay BP_ScoobyWeb18: ScoobyWeb, the mesh saved in the
//!   level with the construction script's dynamic instance (read from the level's saved MaterialInstanceDynamic,
//!   checked against the actor's variables). The instance keeps `web` (those two scalars) and the renderer draws it
//!   with an instance of the material with them (keepParams).
//! - ScoobyCandleabra_C / ScoobyCandleabraDim_C (UserConstructionScript @0..156: SCOOB_MG_Candelabra_A_01.SetStaticMesh(
//!   CandleB_atoggle ? SCOOB_MG_Candelabra_B_01 : _A_01)): the level's saved mesh, checked, and its three particle
//!   components on the mesh's sockets VFX01..03 (PS_ScoobyMansion_CandleFires; the Dim class's PS_CandleSmoke), placed
//!   emitters (bAutoActivate, the component's default).
//! - BP_ScoobyDustMote_C / BP_ScoobyDustMoteBlue_C (data-only): PS_DustMotes, a placed emitter (its template:
//!   PS_DustMotes, _Tall or PS_MansionDustMotesBlue); the Box is NoCollision and draws nothing.
//! - BP_Mansion_LightningStrike_C: PS_MansionLightningStrike, a placed emitter (bAutoActivate, the component's
//!   default. Unverified: the auto activation at BeginPlay of a component whose tick starts off, 4.26 from memory, not
//!   read in the exe). Its loop (BeginPlay @327: RandomFloatInRange(30, 40) s, the global
//!   amb_map_scoobydoo_emit_lightning, SFX Delay 0.2 s, SpawnEmitterAttached(PS_MansionLightningStrike) on that
//!   component @129) is `scoobyMotion`'s `lightning` (with the component's place).
//! - ScoobyElevator_C, the gameplay level's two: TrapDoorSkelMesh (no AnimClass: the reference pose), its slot
//!   GearElementNum given BeginPlay's dynamic instance of GearGlowSourceMaterial (@410..506:
//!   CreateDynamicMaterialInstance(GearElementNum, GearGlowSourceMaterial) then SetMaterial(GearElementNum, it), the
//!   parent's values until a switch sets Emissive): `elevators` keeps the slot and the material.
//! - ScoobyElevatorSwitch_C: SkeletalMesh (no AnimClass: the reference pose, until the first move's PlayAnimation),
//!   with the level's material SCOOB_Gears_A_01_Shimmer_M. BeginPlay destroys it only with hazards off (@2871..2997;
//!   the match's AllowHazards is not read by the scenery: drawn); `switches`.
//! - ScoobyWall_C: ScoobyWallSkelMesh, Gear1SkelMesh, Gear2SkelMesh (no AnimClass: the reference pose); `walls`.
//! - the statues (SkeletalMeshActors of the Animation levels, their AnimClass ScoobyMansion_Statue_AnimBP_C /
//!   ScoobyMansion_StatueB_AnimBP_C): the graph is Root <- ComponentToLocal <- LookAt(statueHead) <- the reference pose
//!   (an unlinked pose; LookAtPoint = the target's K2_GetActorLocation in its ubergraph). Written at the reference
//!   pose (the head not turned. Deviation: LookAt's turn toward LookAtPoint, the world origin until the graph finds a
//!   fighter, is the statues' drawing's); `scoobyMotion`'s `statues` keeps the node's settings and the place.
//!
//! The chains (SkeletalMeshActors of the gameplay level with no AnimClass) and the curtains (single-node idles) are the
//! core's own entries. The block `scoobyMotion` holds what moves on its own: the lightning's loop and the statues'
//! LookAt, the level scripts' ghost walks (Map_ScoobyDoo_C / Map_ScoobyDoo_NoRoof_C, `motion_level`) and the
//! elevators' PS_Mansion_Dustfall (spawned from their sim state).

use indexmap::IndexMap;

use crate::json::Json;
use crate::kind::KindError;
use crate::kinds::scenery::part::{
    JsonProps, SceneryBindCtx, SceneryLevelRun, SceneryMeshOut, SceneryPart, SceneryPartRun, SkeletalOpts,
    StaticMeshOut,
};
use crate::objview::{ObjView, short_name};

use super::util::js::{self, at, fail, first, is_str, nn, num_or, or_num, put, put_opt, req_arr, triple, truthy};
use super::util::{
    ActorProps, Xform, cdo_props, outer_name, over_props, props_of, props_of_actor, reference, rel_of, vars_of,
    world_of, xf_mul, xf_out,
};

const PART: &str = "scooby";

const WEB: &str = "BP_ScoobyWeb_C";
const CANDLES: [&str; 2] = ["ScoobyCandleabra_C", "ScoobyCandleabraDim_C"];
const DUST: [&str; 2] = ["BP_ScoobyDustMote_C", "BP_ScoobyDustMoteBlue_C"];
const LIGHTNING: &str = "BP_Mansion_LightningStrike_C";
const ELEVATOR: &str = "ScoobyElevator_C";
const SWITCH: &str = "ScoobyElevatorSwitch_C";
const WALL: &str = "ScoobyWall_C";
const STATUES: [&str; 2] = ["ScoobyMansion_Statue_AnimBP_C", "ScoobyMansion_StatueB_AnimBP_C"];
const ANIMATIONS: &str = "/Game/Panda_Main/Maps/ScoobyMansion/Animations/";
const WEB_MAT: &str = "/Game/Panda_Main/Maps/ScoobyMansion/Materials/SCOOB_Webs_A_01_M.SCOOB_Webs_A_01_M";
const MESHES: &str = "/Game/Panda_Main/Maps/ScoobyMansion/Meshes/";
const VFX: &str = "/Game/Panda_Main/Maps/ScoobyMansion/VFX/";
const PLAY_VFX: &str = "/Game/Panda_Main/Blueprints/AnimNotifies/AnimNotify_PlayVFX.AnimNotify_PlayVFX_C";

/// `<dir><n>.<n>`: an asset's object path.
fn path_of(dir: &str, n: &str) -> String {
    format!("{dir}{n}.{n}")
}

fn obj_of<'a>(ctx: &dyn SceneryBindCtx<'a>, path: Option<&Json>) -> Result<ObjView<'a>, KindError> {
    ctx.load(&reference(path, "a reference")?)
}

/// A reference's last name, or `None` for none.
fn tail(v: Option<&Json>) -> Option<String> {
    v.and_then(Json::as_str).map(|s| short_name(s).to_owned())
}

/// A component's world transform under its owner's chain (util `world_of`), the last link on a static mesh's socket
/// when it is attached to one: the socket's relative transform (UStaticMeshSocket RelativeLocation / RelativeRotation /
/// RelativeScale) under the mesh component's world one.
fn place_on_socket<'a>(
    ctx: &dyn SceneryBindCtx<'a>,
    owner: &ObjView<'a>,
    e: &ObjView<'a>,
    props: &ActorProps,
) -> Result<Xform, KindError> {
    let objects = &ctx.level().objects;
    let q = props.get(e)?;
    if !truthy(q.get("AttachSocketName")) || is_str(q.get("AttachSocketName"), "None") {
        return world_of(objects, e, props);
    }
    let pp = reference(q.get("AttachParent"), &format!("{}.{} AttachParent", owner.path(), name(e)))?;
    let Some(parent) = objects.iter().find(|x| x.path() == pp) else {
        return fail(PART, format!("{}.{}: no parent {pp}", owner.path(), name(e)));
    };
    if !parent.class().ends_with(".StaticMeshComponent") {
        return fail(PART, format!("{}.{}: socket on a {}", owner.path(), name(e), parent.class()));
    }
    let pq = props.get(parent)?;
    let mesh = obj_of(ctx, pq.get("StaticMesh"))?;
    let want = q.get("AttachSocketName");
    let mut sock = None;
    for x in mesh.inner()? {
        if x.class().ends_with(".StaticMeshSocket") {
            let p = props_of(&x);
            if p.get("SocketName") == want {
                sock = Some(p);
                break;
            }
        }
    }
    let Some(s) = sock else {
        return fail(
            PART,
            format!(
                "{}.{}: no socket {} on {}",
                owner.path(),
                name(e),
                want.and_then(Json::as_str).unwrap_or("undefined"),
                short_name(mesh.path())
            ),
        );
    };
    let mut place = JsonProps::new();
    put_opt(&mut place, "RelativeLocation", s.get("RelativeLocation"));
    put_opt(&mut place, "RelativeRotation", s.get("RelativeRotation"));
    put_opt(&mut place, "RelativeScale3D", s.get("RelativeScale"));
    let socket = rel_of(&place);
    Ok(xf_mul(&xf_mul(&rel_of(q), &socket), &world_of(objects, parent, props)?))
}

fn name<'o>(o: &'o ObjView<'_>) -> &'o str {
    short_name(o.path())
}

/// The statue AnimBP's graph (its class default's nodes): Root <- ComponentToLocal <- LookAt on an unlinked pose.
fn statue_graph<'a>(ctx: &dyn SceneryBindCtx<'a>, cls: &str) -> Result<Json, KindError> {
    let file = cls.strip_suffix("_C").unwrap_or(cls);
    let g = props_of(&ctx.load(&format!("{ANIMATIONS}{file}.Default__{cls}"))?);
    let nodes: Vec<&str> = g.keys().map(String::as_str).filter(|k| k.starts_with("AnimGraphNode_")).collect();
    let l = g.get("AnimGraphNode_LookAt");
    let lid = |v: Option<&Json>| js::number_of(at(v, "LinkID"));
    if nodes.join(",") != "AnimGraphNode_Root,AnimGraphNode_ComponentToLocalSpace,AnimGraphNode_LookAt"
        || lid(at(g.get("AnimGraphNode_Root"), "Result")) != 1.0
        || lid(at(g.get("AnimGraphNode_ComponentToLocalSpace"), "ComponentPose")) != 2.0
        || lid(at(l, "ComponentPose")) != -1.0
        || !is_str(at(at(l, "LookAtTarget"), "BoneReference").and_then(|b| b.get("BoneName")), "None")
        || js::number_of(at(l, "Alpha")) != 1.0
        || !is_str(at(l, "AlphaInputType"), "EAnimAlphaInputType::Float")
    {
        return fail(PART, format!("{cls} changed"));
    }
    let v =
        |a: Option<&Json>| triple([js::number_of(at(a, "X")), js::number_of(at(a, "Y")), js::number_of(at(a, "Z"))]);
    let axis = at(l, "LookAt_Axis");
    let mut o = JsonProps::new();
    put_opt(&mut o, "bone", at(at(l, "BoneToModify"), "BoneName"));
    put(&mut o, "axis", v(at(axis, "Axis")));
    put_opt(&mut o, "axisLocal", at(axis, "bInLocalSpace"));
    put(&mut o, "up", if truthy(at(l, "bUseLookUpAxis")) { v(at(at(l, "LookUp_Axis"), "Axis")) } else { Json::Null });
    put_opt(&mut o, "clamp", at(l, "LookAtClamp"));
    put_opt(&mut o, "interp", at(l, "InterpolationTime"));
    Ok(Json::Obj(o))
}

/// A FTransform variable as written for the renderer (loc, rot as a rotator in degrees, scale).
fn transform_of(vars: &JsonProps, key: &str) -> JsonProps {
    let v = vars.get(key);
    let q = first(at(v, "Rotation"), None);
    let t = first(at(v, "Translation"), None);
    let s = first(at(v, "Scale3D"), None);
    let n = |v: Option<&Json>, k: &str| js::number_of(at(v, k));
    let (qx, qy, qz, qw) =
        if q.is_some() { (n(q, "X"), n(q, "Y"), n(q, "Z"), n(q, "W")) } else { (0.0, 0.0, 0.0, 1.0) };
    let (sx, sy, sz) = if s.is_some() { (n(s, "X"), n(s, "Y"), n(s, "Z")) } else { (1.0, 1.0, 1.0) };
    let x = xf_out(&Xform {
        t: [or_num(at(t, "X"), 0.0), or_num(at(t, "Y"), 0.0), or_num(at(t, "Z"), 0.0)],
        q: [qx, qy, qz, qw],
        s: [sx, sy, sz],
    });
    let mut o = JsonProps::new();
    put(&mut o, "loc", triple(x.loc));
    put(&mut o, "rot", triple(x.rot));
    put(&mut o, "scale", triple(x.scale));
    o
}

// ---- What the switches', trapdoors' and walls' moves draw (from the sim's state) ----
// Every animation the three classes play is SkeletalMeshComponent.PlayAnimation(asset, false) on a component with no
// AnimClass: the single node instance: a montage plays by montage rules, its first slot track; a sequence (the switch's
// end poses) from 0 at rate 1, held at its end. `play` is either as the renderer samples it: its length and RateScale,
// the track's segments (animation by build object path, montage start, animation start / end, rate) and its
// AnimNotify_PlayVFX notifies (a non-fighter owner's path of SpawnVFX: @2233 -> @3113 SpawnEmitterAttached(
// ParticleTeam0, the mesh, SocketName, Location, Rotation, VFX Scale, KeepRelativeOffset); NotifyEnd's KillOnNotifyEnd
// deactivates it, @523..1409).

/// One bind: the run's PlayVFX class defaults, read once.
struct Bind<'c, 'a> {
    ctx: &'c dyn SceneryBindCtx<'a>,
    play_vfx: &'c mut Option<JsonProps>,
}

fn play_of(b: &mut Bind<'_, '_>, anim: Option<&Json>, mesh: Option<&Json>, what: &str) -> Result<Json, KindError> {
    let ctx = b.ctx;
    if !truthy(anim) {
        return fail(PART, format!("{what}: no animation"));
    }
    let asset = obj_of(ctx, anim)?;
    // (the package's name, as the renderer has it: WallChain_L_Retract_montage.WallChain_L_Retract_Montage)
    let ap = asset.path();
    let nm = &ap[ap.rfind('/').map_or(0, |i| i + 1)..ap.rfind('.').unwrap_or(ap.len())];
    let m = props_of(&asset);
    if m.get("Skeleton") != props_of(&obj_of(ctx, mesh)?).get("Skeleton") {
        return fail(PART, format!("{what}: {nm} is of another skeleton"));
    }
    let seq_of = |k: &str| -> Result<JsonProps, KindError> {
        let a = props_of(&ctx.load(k)?);
        if !js::arr(a.get("Notifies")).is_empty()
            || !is_str(js::first(a.get("AdditiveAnimType"), Some(&Json::Str("AAT_None".into()))), "AAT_None")
        {
            return fail(PART, format!("{what}: {} has notifies or is additive", short_name(k)));
        }
        Ok(a)
    };
    if asset.class().ends_with(".AnimSequence") {
        seq_of(asset.path())?;
        let mut seg = JsonProps::new();
        put(&mut seg, "anim", ctx.anim_path(asset.path(), false)?);
        put(&mut seg, "pos", 0.0);
        put(&mut seg, "start", 0.0);
        put_opt(&mut seg, "end", m.get("SequenceLength"));
        put(&mut seg, "rate", 1.0);
        let mut o = JsonProps::new();
        put(&mut o, "name", nm);
        put_opt(&mut o, "length", m.get("SequenceLength"));
        put(&mut o, "rate", nn(m.get("RateScale"), 1.0));
        put(&mut o, "segments", Json::Arr(vec![Json::Obj(seg)]));
        put(&mut o, "notifies", Json::Arr(Vec::new()));
        return Ok(Json::Obj(o));
    }
    if !asset.class().ends_with(".AnimMontage") {
        return fail(PART, format!("{what}: {nm} is a {}", asset.class()));
    }
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
        return fail(PART, format!("{what}: montage {nm} {}", other.join(",")));
    }
    if b.play_vfx.is_none() {
        *b.play_vfx = Some(cdo_props(ctx, PLAY_VFX)?);
    }
    let play_vfx = b.play_vfx.as_ref().expect("read above");
    let mut notifies = Vec::new();
    for n in js::arr(m.get("Notifies")) {
        let r = first(n.get("NotifyStateClass"), n.get("Notify"));
        if !is_str(n.get("NotifyName"), "PlayVFX_C")
            || !is_str(n.get("MontageTickType"), "EMontageNotifyTickType::Queued")
            || !matches!(r, Some(Json::Str(_)))
        {
            return fail(
                PART,
                format!("{nm}: notify {}", n.get("NotifyName").and_then(Json::as_str).unwrap_or("undefined")),
            );
        }
        let q = over_props(play_vfx, Some(&props_of(&obj_of(ctx, r)?)));
        let (s, l, ro) = (q.get("VFX Scale"), q.get("Location"), q.get("Rotation"));
        if truthy(q.get("UseSkinVFX"))
            || truthy(q.get("UseOverrideAttachParent"))
            || !truthy(q.get("SocketName"))
            || is_str(q.get("SocketName"), "None")
            || ["X", "Y", "Z"].iter().any(|k| js::number_of(at(s, k)) != 1.0)
        {
            return fail(PART, format!("{nm}: PlayVFX"));
        }
        let t = js::number_of(n.get("LinkValue")) + or_num(n.get("TriggerTimeOffset"), 0.0);
        let mut o = JsonProps::new();
        put(&mut o, "at", t);
        if truthy(n.get("Duration")) {
            put(&mut o, "end", t + js::number_of(n.get("Duration")));
        }
        put(&mut o, "system", ctx.add_system(&reference(q.get("ParticleTeam0"), "ParticleTeam0")?)?);
        put_opt(&mut o, "socket", q.get("SocketName"));
        put(&mut o, "loc", triple(["X", "Y", "Z"].map(|k| js::number_of(at(l, k)))));
        put(&mut o, "rot", triple(["Pitch", "Yaw", "Roll"].map(|k| js::number_of(at(ro, k)))));
        put(&mut o, "kill", truthy(q.get("KillOnNotifyEnd")));
        notifies.push(Json::Obj(o));
    }
    let mut segments = Vec::new();
    for s in req_arr(PART, at(tracks[0].get("AnimTrack"), "AnimSegments"), "AnimSegments")? {
        let a = reference(s.get("AnimReference"), "a segment")?;
        if num_or(s.get("LoopingCount"), 1.0) != 1.0 {
            return fail(PART, format!("{nm}: segment"));
        }
        seq_of(&a)?;
        let mut o = JsonProps::new();
        put(&mut o, "anim", ctx.anim_path(&a, false)?);
        put(&mut o, "pos", nn(s.get("StartPos"), 0.0));
        put(&mut o, "start", nn(s.get("AnimStartTime"), 0.0));
        put_opt(&mut o, "end", s.get("AnimEndTime"));
        put(&mut o, "rate", nn(s.get("AnimPlayRate"), 1.0));
        segments.push(Json::Obj(o));
    }
    let mut o = JsonProps::new();
    put(&mut o, "name", nm);
    put_opt(&mut o, "length", m.get("SequenceLength"));
    put(&mut o, "rate", nn(m.get("RateScale"), 1.0));
    put(&mut o, "segments", Json::Arr(segments));
    put(&mut o, "notifies", Json::Arr(notifies));
    Ok(Json::Obj(o))
}

/// A SkeletalMeshActor of the level (the chains): its SkeletalMesh.
fn actor_mesh<'a>(ctx: &dyn SceneryBindCtx<'a>, actor: &str) -> Result<Json, KindError> {
    let c =
        ctx.level().objects.iter().find(|x| x.class().ends_with(".SkeletalMeshComponent") && outer_name(x) == actor);
    match c.and_then(|c| props_of(c).get("SkeletalMesh").cloned()).filter(|v| !matches!(v, Json::Null)) {
        Some(v) => Ok(v),
        None => fail(PART, format!("{actor}: no skeletal mesh component")),
    }
}

/// SpawnEmitterAtLocation(system, a transform variable's location / rotation / scale).
fn emitter_at<'a>(ctx: &dyn SceneryBindCtx<'a>, vars: &JsonProps, sys: &str, xf: &str) -> Result<Json, KindError> {
    let mut o = JsonProps::new();
    put(&mut o, "system", ctx.add_system(&reference(vars.get(sys), sys)?)?);
    for (k, v) in transform_of(vars, xf) {
        o.insert(k, v);
    }
    Ok(Json::Obj(o))
}

// ScoobyElevator_C: MoveDownAnim / MoveUpAnim on TrapDoorSkelMesh (ScoobyElevatorSwitch @979:
// ConnectedScoobyElevator.TrapDoorSkelMesh.PlayAnimation(switch: MoveDownAnim / MoveUpAnim)), the BoxCollision the root
// moves (its StageData solid's name). (Its PS_Mansion_Dustfall: the block scoobyMotion's.)
fn elevator_art(
    b: &mut Bind<'_, '_>,
    owner: &ObjView<'_>,
    vars: &JsonProps,
    mesh: Option<&Json>,
) -> Result<JsonProps, KindError> {
    let mut o = JsonProps::new();
    put(&mut o, "solid", format!("{}.BoxCollision", name(owner)));
    put(&mut o, "moveDown", play_of(b, vars.get("MoveDownAnim"), mesh, name(owner))?);
    put(&mut o, "moveUp", play_of(b, vars.get("MoveUpAnim"), mesh, name(owner))?);
    Ok(o)
}

// ScoobyElevatorSwitch_C: its own Release / Retract montage (@788) and Down / Up end pose (@48), the trap chains'
// montages (PlayTrapChainAnims @2413 / @2521), the wall chain's (PlayWallChainVisuals @2231) and its two particles
// (@2151 at WallChainParticle0Transform, @327 at WallChainParticle1Transform).
fn switch_art(
    b: &mut Bind<'_, '_>,
    owner: &ObjView<'_>,
    vars: &JsonProps,
    mesh: Option<&Json>,
) -> Result<JsonProps, KindError> {
    let ctx = b.ctx;
    let o = name(owner);
    let own = props_of(owner);
    let chain = |v: &str| -> Result<String, KindError> {
        tail(own.get(v)).map_or_else(|| fail(PART, format!("{o}: no {v}")), Ok)
    };
    let trap = [chain("TrapChainSkelMesh0")?, chain("TrapChainSkelMesh1")?];
    let wall = chain("WallChainSkelMeshActor")?;
    let trap_mesh = actor_mesh(ctx, &trap[0])?;
    if actor_mesh(ctx, &trap[1])? != trap_mesh {
        return fail(PART, format!("{o}: trap chains of two meshes"));
    }
    let mut out = JsonProps::new();
    put(&mut out, "release", play_of(b, vars.get("ElevatorSwitchReleaseAnimation"), mesh, o)?);
    put(&mut out, "retract", play_of(b, vars.get("ElevatorSwitchRetractAnimation"), mesh, o)?);
    put(&mut out, "downPose", play_of(b, vars.get("ElevatorSwitchDownPose"), mesh, o)?);
    put(&mut out, "upPose", play_of(b, vars.get("ElevatorSwitchUpPose"), mesh, o)?);
    put(&mut out, "trapChains", Json::Arr(trap.iter().map(|t| Json::Str(t.clone())).collect()));
    put(&mut out, "trapRelease", play_of(b, vars.get("TrapChainReleaseAnim"), Some(&trap_mesh), o)?);
    put(&mut out, "trapRetract", play_of(b, vars.get("TrapChainRetractAnim"), Some(&trap_mesh), o)?);
    put(&mut out, "wallChain", wall.as_str());
    put(&mut out, "wallRelease", play_of(b, vars.get("WallChainReleaseAnim"), Some(&actor_mesh(ctx, &wall)?), o)?);
    put(&mut out, "wallRetract", play_of(b, vars.get("WallChainRetractAnim"), Some(&actor_mesh(ctx, &wall)?), o)?);
    put(&mut out, "particle0", emitter_at(ctx, vars, "WallChainParticle0", "WallChainParticle0Transform")?);
    put(&mut out, "particle1", emitter_at(ctx, vars, "WallChainParticle1", "WallChainParticle1Transform")?);
    Ok(out)
}

// ScoobyWall_C: ScoobyWallImpactVFX at ScoobyWallImpactVFXTransform with ScoobyWallImpactAnim (@56, @117),
// ScoobyWallDestroyVFX at ScoobyWallDestroyVFXTransform (@334), the gears' Release / Retract (@571, @657).
fn wall_art(
    b: &mut Bind<'_, '_>,
    owner: &ObjView<'_>,
    vars: &JsonProps,
    props: &ActorProps,
) -> Result<JsonProps, KindError> {
    let ctx = b.ctx;
    let o = name(owner);
    let comp = |n: &str| -> Result<Option<Json>, KindError> {
        let path = format!("{}.{n}", owner.path());
        let Some(c) = ctx.level().objects.iter().find(|x| x.path() == path) else {
            return fail(PART, format!("{o}: no {n}"));
        };
        Ok(props.get(c)?.get("SkeletalMesh").cloned())
    };
    let gear = comp("Gear1SkelMesh")?;
    if comp("Gear2SkelMesh")? != gear {
        return fail(PART, format!("{o}: gears of two meshes"));
    }
    let mut out = JsonProps::new();
    put(&mut out, "impact", emitter_at(ctx, vars, "ScoobyWallImpactVFX", "ScoobyWallImpactVFXTransform")?);
    put(&mut out, "destroy", emitter_at(ctx, vars, "ScoobyWallDestroyVFX", "ScoobyWallDestroyVFXTransform")?);
    put(&mut out, "impactAnim", play_of(b, vars.get("ScoobyWallImpactAnim"), comp("ScoobyWallSkelMesh")?.as_ref(), o)?);
    put(&mut out, "gearRelease", play_of(b, vars.get("ScoobyWallGearReleaseAnim"), gear.as_ref(), o)?);
    put(&mut out, "gearRetract", play_of(b, vars.get("ScoobyWallGearRetractAnim"), gear.as_ref(), o)?);
    Ok(out)
}

// ---- what moves on its own ----
// The gameplay level's level Blueprint (the same graph): BeginPlay @454 -> CreateUIGlobalWidget(2), DepthOffset 0,
// HandleWalkingGhost @409 -> @127: RandomFloatInRange(30, 60) s, Delay @165 -> @224 SpawnEmitterAtLocation(
// PS_ScoobyMansion_GhostWalk, (-2985, 2380, 990), rot(0, -90, 0), scale 1, auto destroy), @297 RandomFloatInRange(30,
// 60) s, Delay @335 -> @54 PS_ScoobyMansion_GhostWalk_L at (-2985, -2380, 990), rot(0, 90, 0), then @394
// HandleWalkingGhost, @409 goto 127: the two alternate for ever. The rotations are EX_RotationConst (@54
// rot(0.0,90.0,0.0), @224 rot(0.0,-90.0,0.0)); the latent resumes the latent links'.
// The elevators (ScoobyElevator @42..92): PS_Mansion_Dustfall at DefaultSceneRoot's K2_GetComponentLocation, rot(0, 0,
// 0), scale 1.
fn motion_level<'a>(
    ctx: &dyn SceneryBindCtx<'a>,
    ghosts: &mut Option<Json>,
    dustfall: &mut Option<Json>,
) -> Result<(), KindError> {
    if ghosts.is_some() {
        return fail(PART, "two ghost scripts".into());
    }
    // in the order they come: the first after the first wait
    let walk = |sys: &str, loc: [f64; 3], rot: [f64; 3]| -> Result<Json, KindError> {
        let mut o = JsonProps::new();
        put(&mut o, "system", ctx.add_system(&path_of(VFX, sys))?);
        put(&mut o, "loc", triple(loc));
        put(&mut o, "rot", triple(rot));
        Ok(Json::Obj(o))
    };
    let mut g = JsonProps::new();
    put(&mut g, "delay", Json::Arr(vec![Json::Num(30.0), Json::Num(60.0)]));
    let walks = vec![
        walk("PS_ScoobyMansion_GhostWalk", [-2985.0, 2380.0, 990.0], [0.0, -90.0, 0.0])?,
        walk("PS_ScoobyMansion_GhostWalk_L", [-2985.0, -2380.0, 990.0], [0.0, 90.0, 0.0])?,
    ];
    put(&mut g, "walks", Json::Arr(walks));
    *ghosts = Some(Json::Obj(g));
    let mut d = JsonProps::new();
    put(&mut d, "system", ctx.add_system(&path_of(VFX, "PS_Mansion_Dustfall"))?);
    *dustfall = Some(Json::Obj(d));
    Ok(())
}

pub struct ScoobysHauntedMansion;

impl SceneryPart for ScoobysHauntedMansion {
    fn name(&self) -> &'static str {
        "scooby"
    }
    fn blueprints(&self) -> &'static [&'static str] {
        &[
            "BP_ScoobyWeb_C",
            "ScoobyCandleabra_C",
            "ScoobyCandleabraDim_C",
            "BP_ScoobyDustMote_C",
            "BP_ScoobyDustMoteBlue_C",
            "BP_Mansion_LightningStrike_C",
            "ScoobyElevator_C",
            "ScoobyElevatorSwitch_C",
            "ScoobyWall_C",
        ]
    }
    fn levels(&self) -> &'static [&'static str] {
        &["Map_ScoobyDoo_C", "Map_ScoobyDoo_NoRoof_C"]
    }
    fn anim_blueprints(&self) -> &'static [&'static str] {
        &["ScoobyMansion_Statue_AnimBP_C", "ScoobyMansion_StatueB_AnimBP_C"]
    }
    // the webs' material: each web's dynamic instance has its own Emissive_Mult and WPO (the renderer's block)
    fn keep_params(&self) -> &'static [&'static str] {
        &["/Game/Panda_Main/Maps/ScoobyMansion/Materials/SCOOB_Webs_A_01_M.SCOOB_Webs_A_01_M"]
    }
    fn run<'a>(&self) -> Box<dyn SceneryPartRun<'a> + 'a> {
        Box::new(ScoobyRun::default())
    }
}

/// One bind's state.
#[derive(Default)]
struct ScoobyRun {
    /// `<Emissive_Mult>,<WPO>` -> its instances.
    webs: IndexMap<String, usize>,
    elevators: Vec<Json>,
    switches: Vec<Json>,
    walls: IndexMap<String, JsonProps>,
    statues: Vec<Json>,
    lightning: Option<Json>,
    ghosts: Option<Json>,
    dustfall: Option<Json>,
    play_vfx: Option<JsonProps>,
}

fn number_text(n: f64) -> String {
    let mut b = Vec::new();
    crate::json::write_number(&mut b, n);
    String::from_utf8(b).unwrap_or_default()
}

impl<'a> SceneryPartRun<'a> for ScoobyRun {
    fn level(&mut self, _cls: &str, ctx: &dyn SceneryBindCtx<'a>) -> Result<SceneryLevelRun, KindError> {
        motion_level(ctx, &mut self.ghosts, &mut self.dustfall)?;
        Ok(SceneryLevelRun { tags: Vec::new() })
    }

    fn static_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
        p: &JsonProps,
    ) -> Result<StaticMeshOut, KindError> {
        let props = props_of_actor(ctx, owner)?;
        let mut placed = p.clone();
        for (k, v) in ctx.place_of(e, Some(&props))? {
            placed.insert(k, v);
        }
        let vars = vars_of(ctx, owner)?;
        if cls == WEB {
            let over = js::arr(p.get("OverrideMaterials"));
            if name(e) != "ScoobyWeb" || over.len() != 1 {
                return fail(PART, format!("{}: web component", name(owner)));
            }
            let mid = match &over[0] {
                Json::Str(r) => ctx
                    .level()
                    .objects
                    .iter()
                    .find(|x| x.path() == r && x.class().ends_with(".MaterialInstanceDynamic")),
                _ => None,
            };
            let Some(q) = mid.map(props_of) else { return fail(PART, format!("{}: web material", name(owner))) };
            if !is_str(q.get("Parent"), WEB_MAT)
                || !js::arr(q.get("VectorParameterValues")).is_empty()
                || !js::arr(q.get("TextureParameterValues")).is_empty()
            {
                return fail(PART, format!("{}: web material", name(owner)));
            }
            let mut scalars = JsonProps::new();
            for s in js::arr(q.get("ScalarParameterValues")) {
                let n = at(s.get("ParameterInfo"), "Name").and_then(Json::as_str).unwrap_or("undefined");
                put_opt(&mut scalars, n, s.get("ParameterValue"));
            }
            let mut keys: Vec<&str> = scalars.keys().map(String::as_str).collect();
            keys.sort_unstable();
            if keys.join(",") != "Emissive_Mult,WPO"
                || scalars.get("Emissive_Mult") != vars.get("Emissive_Mult")
                || scalars.get("WPO") != vars.get("WPO")
            {
                return fail(PART, format!("{}: web scalars", name(owner)));
            }
            let k = format!(
                "{},{}",
                number_text(js::number_of(scalars.get("Emissive_Mult"))),
                number_text(js::number_of(scalars.get("WPO")))
            );
            *self.webs.entry(k).or_insert(0) += 1;
            let mut out = placed;
            out.insert("OverrideMaterials".into(), Json::Arr(q.get("Parent").cloned().into_iter().collect()));
            let mut extra = JsonProps::new();
            put(&mut extra, "web", Json::Obj(scalars));
            return Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: out, extra, placed: true }));
        }
        if CANDLES.contains(&cls) {
            let toggle = truthy(vars.get("CandleB_atoggle"));
            let want = path_of(MESHES, if toggle { "SCOOB_MG_Candelabra_B_01" } else { "SCOOB_MG_Candelabra_A_01" });
            if name(e) != "SCOOB_MG_Candelabra_A_01" || !is_str(p.get("StaticMesh"), &want) {
                let shown = p.get("StaticMesh").and_then(Json::as_str).unwrap_or("undefined");
                return fail(PART, format!("{}: candelabra mesh {shown}", name(owner)));
            }
            return Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: placed, extra: JsonProps::new(), placed: true }));
        }
        fail(PART, format!("{}: static mesh component {} of {cls}", name(owner), name(e)))
    }

    // a Blueprint actor's particle component in a scenery level: a placed emitter at its world transform
    fn particle_system(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        let props = props_of_actor(ctx, owner)?;
        let q = props.get(e)?;
        if q.get("bAutoActivate") == Some(&Json::Bool(false))
            || q.get("bVisible") == Some(&Json::Bool(false))
            || truthy(q.get("bHiddenInGame"))
            || !truthy(q.get("Template"))
        {
            return fail(PART, format!("{}.{}: particle component", name(owner), name(e)));
        }
        if !(CANDLES.contains(&cls) || DUST.contains(&cls) || cls == LIGHTNING) {
            return Ok(false);
        }
        let o = xf_out(&place_on_socket(ctx, owner, e, &props)?);
        let ename = format!("{}.{}", name(owner), name(e));
        let system = ctx.add_system(&reference(q.get("Template"), "Template")?)?;
        let mut entry = JsonProps::new();
        put(&mut entry, "name", ename.as_str());
        put(&mut entry, "system", system.as_str());
        put(&mut entry, "loc", triple(o.loc));
        put(&mut entry, "rot", triple(o.rot));
        put(&mut entry, "scale", triple(o.scale));
        ctx.add_emitter(entry);
        if cls == LIGHTNING {
            if self.lightning.is_some() {
                return fail(PART, "two lightning actors".into());
            }
            let mut at_ = JsonProps::new();
            put(&mut at_, "loc", triple(o.loc));
            put(&mut at_, "rot", triple(o.rot));
            put(&mut at_, "scale", triple(o.scale));
            let mut l = JsonProps::new();
            put(&mut l, "emitter", ename.as_str());
            put(&mut l, "system", system.as_str());
            put(&mut l, "delay", Json::Arr(vec![Json::Num(30.0), Json::Num(40.0)]));
            put_opt(&mut l, "sfxDelay", vars_of(ctx, owner)?.get("SFX Delay"));
            put(&mut l, "event", "amb_map_scoobydoo_emit_lightning");
            put(&mut l, "at", Json::Obj(at_));
            self.lightning = Some(Json::Obj(l));
        }
        Ok(true)
    }

    fn skeletal_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        let props = props_of_actor(ctx, owner)?;
        let p = props.get(e)?.clone();
        let nm = format!("{}.{}", name(owner), name(e));
        let mut b = Bind { ctx, play_vfx: &mut self.play_vfx };
        if cls == ELEVATOR {
            if name(e) != "TrapDoorSkelMesh" {
                return Ok(false);
            }
            let vars = vars_of(ctx, owner)?;
            let slot = js::number_of(vars.get("GearElementNum"));
            let material = vars.get("GearGlowSourceMaterial").filter(|m| truthy(Some(m)));
            let h = ctx.mesh_header(&reference(p.get("SkeletalMesh"), &format!("{nm} SkeletalMesh"))?)?;
            let Some(material) = material.filter(|_| slot >= 0.0 && slot < h.slots.len() as f64) else {
                return fail(PART, format!("{nm}: gear slot"));
            };
            let slot_i = slot as usize;
            let mut over: Vec<Json> = js::arr(p.get("OverrideMaterials")).to_vec();
            while over.len() <= slot_i {
                over.push(Json::Null);
            }
            over[slot_i] = material.clone();
            let mut comp = p.clone();
            comp.insert("OverrideMaterials".into(), Json::Arr(over));
            ctx.add_skeletal(&nm, &comp, &ctx.place_of(e, Some(&props))?, &SkeletalOpts::default())?;
            let mut gear = JsonProps::new();
            put(&mut gear, "slot", slot);
            let skin = ctx.skin_material(&reference(Some(material), "GearGlowSourceMaterial")?, &nm)?;
            put(&mut gear, "material", skin.map_or(Json::Null, Json::Str));
            let mut rec = JsonProps::new();
            put(&mut rec, "actor", name(owner));
            put(&mut rec, "skeletal", nm.as_str());
            put(&mut rec, "gear", Json::Obj(gear));
            for (k, v) in elevator_art(&mut b, owner, &vars, p.get("SkeletalMesh"))? {
                rec.insert(k, v);
            }
            self.elevators.push(Json::Obj(rec));
            return Ok(true);
        }
        if cls == SWITCH {
            if name(e) != "SkeletalMesh" {
                return Ok(false);
            }
            ctx.add_skeletal(&nm, &p, &ctx.place_of(e, Some(&props))?, &SkeletalOpts::default())?;
            let own = props_of(owner);
            let mut rec = JsonProps::new();
            put(&mut rec, "actor", name(owner));
            put(&mut rec, "skeletal", nm.as_str());
            if let Some(t) = tail(own.get("ConnectedScoobyElevator")) {
                put(&mut rec, "elevator", t);
            }
            if let Some(t) = tail(own.get("ConnectedScoobyWall")) {
                put(&mut rec, "wall", t);
            }
            let vars = vars_of(ctx, owner)?;
            for (k, v) in switch_art(&mut b, owner, &vars, p.get("SkeletalMesh"))? {
                rec.insert(k, v);
            }
            self.switches.push(Json::Obj(rec));
            return Ok(true);
        }
        if cls == WALL {
            if !["ScoobyWallSkelMesh", "Gear1SkelMesh", "Gear2SkelMesh"].contains(&name(e)) {
                return Ok(false);
            }
            ctx.add_skeletal(&nm, &p, &ctx.place_of(e, Some(&props))?, &SkeletalOpts::default())?;
            if !self.walls.contains_key(name(owner)) {
                let mut rec = JsonProps::new();
                put(&mut rec, "actor", name(owner));
                put(&mut rec, "wall", Json::Null);
                put(&mut rec, "gears", Json::Arr(Vec::new()));
                let vars = vars_of(ctx, owner)?;
                for (k, v) in wall_art(&mut b, owner, &vars, &props)? {
                    rec.insert(k, v);
                }
                self.walls.insert(name(owner).to_owned(), rec);
            }
            let w = &mut self.walls[name(owner)];
            if name(e) == "ScoobyWallSkelMesh" {
                put(w, "wall", nm.as_str());
            } else if let Some(Json::Arr(g)) = w.get_mut("gears") {
                g.push(Json::Str(nm));
            }
            return Ok(true);
        }
        Ok(false)
    }

    // a SkeletalMeshActor whose AnimClass is one of animBlueprints: the statues at the reference pose
    fn skeletal_actor(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        if !STATUES.contains(&cls) {
            return Ok(false);
        }
        let a = ctx.place_of(e, None)?;
        ctx.add_skeletal(
            name(owner),
            &props_of(e),
            &a,
            &SkeletalOpts { anim_bp: Some(cls.to_owned()), ..Default::default() },
        )?;
        let (l, r, s) = (a.get("RelativeLocation"), a.get("RelativeRotation"), a.get("RelativeScale3D"));
        let n = |v: Option<&Json>, k: &str, d: f64| num_or(at(v, k), d);
        let mut at_ = JsonProps::new();
        put(&mut at_, "loc", triple([n(l, "X", 0.0), n(l, "Y", 0.0), n(l, "Z", 0.0)]));
        put(&mut at_, "rot", triple([n(r, "Pitch", 0.0), n(r, "Yaw", 0.0), n(r, "Roll", 0.0)]));
        put(&mut at_, "scale", triple([n(s, "X", 1.0), n(s, "Y", 1.0), n(s, "Z", 1.0)]));
        let mut rec = JsonProps::new();
        put(&mut rec, "skeletal", name(owner));
        put(&mut rec, "animBP", cls);
        put(&mut rec, "lookAt", statue_graph(ctx, cls)?);
        put(&mut rec, "at", Json::Obj(at_));
        self.statues.push(Json::Obj(rec));
        Ok(true)
    }

    fn level_blocks(&mut self) -> Result<JsonProps, KindError> {
        if self.ghosts.is_none() || self.lightning.is_none() || self.statues.is_empty() {
            let what = if self.ghosts.is_none() {
                "the level script"
            } else if self.lightning.is_none() {
                "the lightning"
            } else {
                "statues"
            };
            return fail(PART, format!("motion without {what}"));
        }
        let mut m = JsonProps::new();
        put_opt(&mut m, "lightning", self.lightning.as_ref());
        put(&mut m, "statues", Json::Arr(self.statues.clone()));
        put_opt(&mut m, "ghosts", self.ghosts.as_ref());
        put_opt(&mut m, "dustfall", self.dustfall.as_ref());
        let mut out = JsonProps::new();
        put(&mut out, "scoobyMotion", Json::Obj(m));
        Ok(out)
    }

    fn actor_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut out = JsonProps::new();
        if !self.webs.is_empty() || !self.elevators.is_empty() || !self.statues.is_empty() {
            let mut s = JsonProps::new();
            put(&mut s, "webs", self.webs.len() as f64);
            put(&mut s, "elevators", Json::Arr(self.elevators.clone()));
            put(&mut s, "switches", Json::Arr(self.switches.clone()));
            put(&mut s, "walls", Json::Arr(self.walls.values().map(|w| Json::Obj(w.clone())).collect()));
            put(&mut out, "scooby", Json::Obj(s));
        }
        Ok(out)
    }

    fn summary(&self) -> String {
        format!(
            ", {} webs ({} material instances), {} elevators, {} switches, {} walls, {} statues{}",
            self.webs.values().sum::<usize>(),
            self.webs.len(),
            self.elevators.len(),
            self.switches.len(),
            self.walls.len(),
            self.statues.len(),
            if self.lightning.is_some() { ", lightning" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_variable() {
        // a FTransform variable with only a translation: the identity rotation, scale 1
        let vars = match Json::parse(br#"{"T":{"Translation":{"X":1,"Y":2}}}"#).unwrap() {
            Json::Obj(m) => m,
            _ => unreachable!(),
        };
        let o = transform_of(&vars, "T");
        assert_eq!(o["loc"], triple([1.0, 2.0, 0.0]));
        assert_eq!(o["rot"], triple([0.0, 0.0, 0.0]));
        assert_eq!(o["scale"], triple([1.0, 1.0, 1.0]));
        assert_eq!(number_text(0.5), "0.5");
        assert_eq!(path_of("/A/", "B"), "/A/B.B");
    }

    #[test]
    fn summary_of_nothing() {
        let run = ScoobyRun::default();
        assert_eq!(
            <ScoobyRun as SceneryPartRun<'_>>::summary(&run),
            ", 0 webs (0 material instances), 0 elevators, 0 switches, 0 walls, 0 statues"
        );
    }
}
