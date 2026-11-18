//! The Court's scenery part (`m010`): its own Blueprint actors and level Blueprints.
//!
//! This writes what each draws at its place and pose at BeginPlay (the still drawing); the block `court` names what
//! the renderer moves or spawns with the sim later (the hoops' plays, the ship's montages, the clock hands, the balloon
//! boat's sequence, the walls' barrier parameters, the effects the graphs spawn). The landscape (Map_M010_Background's
//! two Landscape actors) is the core's.
//! - M010_Spotlight_BP_C (VFX level, eight): the skeletal mesh SJAM_Spotlight_A_01 plays Idle02 ?
//!   SJAM_Spotlight_Anim_Idle_v2 : _v1 (@300 / @319), looping, playing, from 0 at rate 1 (UserConstructionScript @357
//!   OverrideAnimationData(anim, true, true, 0.0, 1.0)); PS_Spotlight_Ray, attached to the skeletal component with no
//!   socket, with the construction script's eight parameters @10..571 (the actor's fields). The level stores both
//!   results of the construction script (AnimationData, InstanceParameters); they are checked against the actor's
//!   fields here. BeginPlay destroys the ray only at a visual effect quality of 1 or lower or a mobile feature level
//!   (@48..375): neither here.
//! - M010_ShotClock_C (VFX level, SJAM_ShotClock_5): PS_Spacejam_Clock (PS_M010_Clock, auto activated: a placed
//!   emitter), and BeginPlay's loop over every M010_Hoop_A_01_BP_C (@816 GetAllActorsOfClass) spawns PS_M010_Clock
//!   attached to PS_Spacejam_Clock once per hoop (@357 SpawnEmitterAttached, offset 0, rot 0, scale 1,
//!   KeepRelativeOffset, auto destroyed): `shotClock.begin`, which the renderer spawns at begin. Its Delays (@458 24 s:
//!   PS_M010_Clock1 @35 then ShotClockFinishedEvent; @694 20 s: sfx_map_M010_shotclock_tick @136) are the sim's:
//!   `shotClock.finished`, `shotClock.tick`.
//! - M010_Hoop_A_01_BP_C (gameplay level, two): SkeletalMesh (M010_ST_Hoop_01, the AnimationBlueprint mode with no
//!   AnimClass: the reference pose) and PS_SpacejamHoopArrows (PS_M010_HoopArrows, attached to the skeletal component,
//!   no socket: a placed emitter, gameplayParticles). `hoops[i].plays`: the SkeletalMesh.PlayAnimation(asset, false)
//!   calls of its overlap events (HOOP_PLAYS: every such call of the graph), the animations by build object path;
//!   `explosion` PS_Hoop_Explosion @312.
//! - m010_wall_C (gameplay level, two): the actor is hidden (bHidden) and no graph shows it, so its WallGlow draws
//!   nothing. Its ConnectedBarrierWall (SM_Barrier_Side_L / _R, StaticMeshActors of the gameplay level with no stored
//!   AttachParent) is attached to the wall's DefaultSceneRoot at BeginPlay (@1520 K2_AttachToComponent(
//!   DefaultSceneRoot, None, 1, 0, 1, true): location and scale KeepWorld, rotation KeepRelative, so its placed
//!   rotation turns relative to the wall root: Side_R's world yaw becomes about +90): begin_attach, the core writes
//!   it at that transform, `attachedTo` the wall. Its material M010_Barrier_Side_MESH_MAT is lit surface
//!   translucency; the renderer moves it with the wall's WallCollision solid. It takes Damage @1582 and Dissolve @105
//!   (SetScalarParameterValueOnMaterials) on its material (keep_params: M010_Barrier_Side_MESH_MAT keeps its
//!   preshaders), its ConnectedFloorGlow is shown @15 and hidden @2037: `walls`, with the effects the wall spawns
//!   (PS_M010_Barrier_Touch @1067, PS_M010_WallImpact @1843 at the root + (0, 0, 1000) @1746 by `impactRot`, the
//!   actor's DamageVFXRotation), `root` its root's place at BeginPlay and `ak` its Ak component's, where BeginPlay
//!   posts `ambience` @1374.
//! - PickupPowerup_Jump_C / _Power_C / _Speed_C (spawned by Map_M010_C's PowerupSpawner, no placed actor): `pickups`,
//!   the systems their graphs spawn (the intro and type particles @53 / @203 on the type component, of scale
//!   `scale`; the outro on the collector's root; the buffs' PS_C016_PowerUp_<type>_Buff @165 on the owner's main mesh
//!   at Root, Pickup<type>Buff @301 destroying it).
//! - M010_JumboTron_C (gameplay level): SkeletalMeshComponent0 (M010_ST_JumboTron_01, the AnimationBlueprint mode
//!   with no AnimClass: the reference pose). Its WidgetComponent UI (UI_SpaceJam_Scoreboard, hidden at BeginPlay
//!   @685): `jumbotron.ui`.
//! - M010_MG_Ship_C (Animation level): Mesh with M010_Ship_AnimBP_C (a DefaultSlot with no source: the reference pose
//!   until a montage plays). BeginPlay plays M010Ship_Anim_Landing_Montage @552, MatchEnded
//!   M010Ship_Anim_TakeOff_Montage @981, whose completion hides Mesh @10: `ship` (`landing`, `takeOff`: each
//!   montage's DefaultSlot segments and sections, the animations by path, its notifies), `sockets` the notifies'
//!   sockets (a skeleton socket, else the bone of that name: Root, shipBase), `root` the actor root's place (the
//!   AkComponent's).
//! - M010_BombFactory_ClockHands_C (Animation level): its two StaticMesh hands at their placed relative rolls (the
//!   component templates' RelativeRotation, -60 / -160.00035): `clockHands` the numbers its graph moves them by:
//!   `roll` the placed rolls, `random` BeginPlay's RandomFloatInRange(0, 360) @1627 / @1764, `idle`
//!   UpdateMatchTimer's step (@439..861: every 0.4 s, A + 12 @483, B - 8 @693), `rotate` OnUpdateLifeCount's
//!   RotateHands(770, -770) @1912 and its Delay(0.5) @885 before B (Rotate Hand B.PlayFromStart @100, A @325; the
//!   hand's roll = rotation x EaseInOut + initial, @133 / @179, @1151), `timelines` A and B (length: A's
//!   TheTimeline Length 4, B's unstored: 5.0, the UTimelineComponent default 142e34d70; keys the EaseInOut curves
//!   CurveFloat_2 / CurveFloat_2_3).
//! - Map_M010_Animation_C (Map_M010_Animation @10..52: GetSequencePlayer().PlayLooping(-1)): plays SpaceJam_AnimSeq_2
//!   looping, whose one binding moves the SkeletalMeshActor M010_BalloonBoat_01: drawn at its level place, playing
//!   its single-node idle; `sequence` the sequence's playback range, its transform section's channels (seconds; a
//!   tangent per second) and its skeletal section.
//! - Map_M010_C (gameplay level): LevelKnockoutFX spawns PS_M010_Barrier at (-2300, Y, Z) @1803 / @1854 (`knockout`);
//!   its DepthOffset write @1639 and UI widget request @1554 are not the scenery's.
//! - Item_Basketball_C (spawned by Map_M010_C SpawnBall, a World.items actor of the sim): its BasketballGeo
//!   (StaticMesh M010_Ball_01 under the Capsule, OverrideMaterials [M010_Ball_Gray_MI]) is written as one hidden,
//!   unplaced instance per material its graph sets on slot 0: the template's Gray (NeutralMaterial @353, SetMaterial
//!   @1918), Red and Blue (@289 / @225, SetTeamColor @1772): `ball`, which the renderer places with the item. No
//!   lightmap (a spawned component has no build data): the plain key, as the platforms' meshes.

use mvs_math::js;

use crate::json::Json;
use crate::kind::KindError;
use crate::kinds::scenery::part::{
    JsonProps, SceneryBeginAttach, SceneryBindCtx, SceneryLevelRun, SceneryMeshOut, SceneryPart, SceneryPartRun,
    SkeletalOpts, StaticMeshOut,
};
use crate::kinds::skeleton;
use crate::objview::{ObjView, short_name};

use super::batcave::{at, is_num_or, level_obj, need, notify_time, num, nums, obj, object_of, r3, tail, v3d};
use super::util::js::{arr, first, is_str, req_arr, truthy};
use super::util::{
    class_inner, curve_keys, name_of, number, outer_name, props_of, props_of_actor, reference, vars_of, world_of,
    xf_out,
};

const SPOTLIGHT: &str = "M010_Spotlight_BP_C";
const SHOT_CLOCK: &str = "M010_ShotClock_C";
const HOOP: &str = "M010_Hoop_A_01_BP_C";
const WALL: &str = "m010_wall_C";
const JUMBOTRON: &str = "M010_JumboTron_C";
const SHIP: &str = "M010_MG_Ship_C";
const HANDS: &str = "M010_BombFactory_ClockHands_C";
const ANIMATION_LEVEL: &str = "Map_M010_Animation_C";
const SHIP_ANIM_BP: &str = "M010_Ship_AnimBP_C";
const M010: &str = "/Game/Panda_Main/Maps/M010/";
const ANIMS: &str = "/Game/Panda_Main/Maps/M010/Animations/";
const VFX: &str = "/Game/Panda_Main/Maps/M010/VFX/";
const BARRIER_MAT: &str = "/Game/Panda_Main/Maps/M010/VFX/M010_Barrier_Side_MESH_MAT.M010_Barrier_Side_MESH_MAT";
const PICKUP_TYPES: [&str; 3] = ["Jump", "Power", "Speed"];
const BARRIER_AMBIENCE: &str = "amb_map_M010_emit_barrier";
const BALL: &str = "Item_Basketball_C";
const BALL_CLASS: &str = "/Game/Panda_Main/Blueprints/Items/Item_Basketball.Item_Basketball_C";
const BALL_MESH: &str = "/Game/Panda_Main/Maps/M010/Meshes/M010_Ball_01.M010_Ball_01";
const BALL_MATS: [(&str, &str); 3] =
    [("gray", "M010_Ball_Gray_MI"), ("red", "M010_Ball_Red_MI"), ("blue", "M010_Ball_Blue_MI")];
const HANDS_CLASS: &str = "/Game/Panda_Main/Maps/M010/Meshes/M010_BombFactory_ClockHands.M010_BombFactory_ClockHands_C";
const PLAY_VFX: &str = "/Game/Panda_Main/Blueprints/AnimNotifies/AnimNotify_PlayVFX.AnimNotify_PlayVFX_C";
const ONE_SHOT: &str = "/Game/Panda_Main/Ak_Audio/AkNotify/Ak_OneShot.Ak_OneShot_C";

fn fail<T>(what: impl std::fmt::Display) -> Result<T, KindError> {
    Err(KindError::Failed(format!("scenery m010: {what}")))
}

fn sys(n: &str) -> String {
    at(VFX, n)
}

/// `Math.fround(a) === Math.fround(b)`.
fn same_f32(a: f64, b: f64) -> bool {
    js::fround(a) == js::fround(b)
}

/// `[X, Y, Z]` of a struct; the struct is read as JavaScript reads it, so a missing member is `undefined` (null).
fn v3(v: Option<&Json>) -> Result<Json, KindError> {
    match v {
        Some(s @ Json::Obj(_)) => Ok(Json::Arr(["X", "Y", "Z"].map(|k| num(number(s.get(k)))).into())),
        _ => fail("a vector is not a struct"),
    }
}

// [offset, event, asset] of every SkeletalMesh.PlayAnimation(AnimSequence, false) of M010_Hoop_A_01_BP, and the
// overlap event each is in
const HOOP_PLAYS: [(u32, &str, &str); 6] = [
    (381, "scored", "Basket_Anim_HR_Front"),
    (428, "topBegin", "Basket_Anim_WeightAdd"),
    (540, "backBegin", "Basket_Anim_HR_Behind"),
    (652, "bottomBegin", "Basket_Anim_HR_Underneath"),
    (764, "frontBegin", "Basket_Anim_HR_Front"),
    (876, "topEnd", "Basket_Anim_WeightLift"),
];

// A montage's DefaultSlot track and sections, its sequences by build object path, and its queued notifies at montage
// time (the segments' sequences' own among them), in the shape of batcave.rs montage_of: `at` the link value plus
// TriggerTimeOffset, `end` a notify state's end; by kind
// - AnimNotify_PlayVFX (a non-fighter owner's SpawnVFX @2233: SpawnEmitterAttached(ParticleTeam0, the mesh,
//   SocketName, Location, Rotation, VFX Scale, KeepRelativeOffset)): `vfx`, its end deactivating it with
//   KillOnNotifyEnd (Landing: PS_M010_Spaceship_Land at Root from 0 to 3.35 s, PS_M010_Spaceship_Land01 at
//   Ramp_Lower; TakeOff: PS_SpaceshipRibbon at shipBase);
// - Ak_OneShot (Follow and not Local Only: the class default): `sound`, its event on the owner's root AkComponent
//   (the ship's ufo_* events).
// A blend the montage does not store is UAnimMontage's constructor's, 0.25 s (UAnimMontage ctor 142849770; TakeOff
// stores neither), as batcave.rs writes it.
const MONTAGE_DEFAULT_BLEND: f64 = 0.25;

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

/// The notifies of a montage as they gather: (`at`, the notify's JSON).
struct Gather<'c> {
    play_vfx: &'c JsonProps,
    one_shot: &'c JsonProps,
    list: Vec<(f64, Json)>,
}

fn notify<'a>(
    ctx: &dyn SceneryBindCtx<'a>,
    g: &mut Gather<'_>,
    name: &str,
    n: &Json,
    t: f64,
    end: Option<f64>,
    of_segment: bool,
) -> Result<(), KindError> {
    if !is_str(n.get("MontageTickType"), "EMontageNotifyTickType::Queued")
        || !is_num_or(n.get("NotifyTriggerChance"), 1.0, 1.0)
    {
        return fail(format!("{name}: notify tick type"));
    }
    let nr = first(n.get("Notify"), None).or(n.get("NotifyStateClass"));
    let q = match nr {
        Some(Json::Str(path)) => props_of(&ctx.load(path)?),
        _ => JsonProps::new(),
    };
    let notify_name = n.get("NotifyName").and_then(Json::as_str);
    if notify_name == Some("PlayVFX_C") {
        // `{ ...playVfx, ...q }`: a shallow merge
        let mut p = g.play_vfx.clone();
        p.extend(q.clone());
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
        let system = ctx.add_system(&reference(p.get("ParticleTeam0"), "ParticleTeam0")?)?;
        g.list.push((
            t,
            obj([
                ("at", Some(num(t))),
                ("end", end.map(num)),
                ("kind", Some(Json::from("vfx"))),
                ("system", Some(Json::from(system))),
                ("socket", p.get("SocketName").cloned()),
                ("loc", Some(v3d(p.get("Location"), [0.0; 3]))),
                ("rot", Some(r3(p.get("Rotation")))),
                ("scale", Some(v3d(p.get("VFX Scale"), [1.0; 3]))),
                ("kill", Some(Json::Bool(truthy(p.get("KillOnNotifyEnd"))))),
            ]),
        ));
    } else if notify_name == Some("Ak_OneShot_C") {
        only(&q, &["AkEvent", "Attach Name"], name, n)?;
        let mut p = g.one_shot.clone();
        p.extend(q);
        // (the segments' sequences each hold one with no AkEvent: Ak_OneShot @51 IsValid(AkEvent) @80 goes to the "No
        // AkEvent on" print @551 and posts nothing)
        if !truthy(p.get("AkEvent")) && of_segment {
            return Ok(());
        }
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
    } else {
        return fail(format!("{name}: notify {}", notify_name.unwrap_or("undefined")));
    }
    Ok(())
}

/// A montage as the entry writes it, and the sockets its notifies name (those with a truthy `socket`, in order).
fn montage_of<'a>(ctx: &dyn SceneryBindCtx<'a>, name: &str) -> Result<(Json, Vec<String>), KindError> {
    let m = props_of(&ctx.load(&at(ANIMS, name))?);
    let tracks = req_arr("m010", m.get("SlotAnimTracks"), "SlotAnimTracks")?;
    if tracks.len() != 1 || !is_str(tracks[0].get("SlotName"), "DefaultSlot") {
        return fail(format!("{name}: slots"));
    }
    let blend = |b: Option<&Json>| -> Result<Json, KindError> {
        let k: Vec<&str> = b
            .and_then(Json::as_object)
            .map_or(Vec::new(), |o| o.keys().map(String::as_str).filter(|x| *x != "BlendTime").collect());
        if !k.is_empty() {
            return fail(format!("{name}: blend {}", k.join(",")));
        }
        Ok(first(b.and_then(|b| b.get("BlendTime")), None).cloned().unwrap_or(num(MONTAGE_DEFAULT_BLEND)))
    };
    let play_vfx = super::util::cdo_props(ctx, PLAY_VFX)?;
    let one_shot = super::util::cdo_props(ctx, ONE_SHOT)?;
    let mut g = Gather { play_vfx: &play_vfx, one_shot: &one_shot, list: Vec::new() };
    for n in arr(m.get("Notifies")) {
        let lv = number(n.get("LinkValue"));
        let end = truthy(n.get("Duration")).then(|| lv + number(n.get("Duration")));
        notify(ctx, &mut g, name, n, notify_time(n), end, false)?;
    }
    let mut segments = Vec::new();
    for s in arr(tracks[0].get("AnimTrack").and_then(|t| t.get("AnimSegments"))) {
        if !is_num_or(s.get("AnimPlayRate"), 1.0, 1.0) || !is_num_or(s.get("LoopingCount"), 1.0, 1.0) {
            return fail(format!("{name}: segment rate / loops"));
        }
        let anim = reference(s.get("AnimReference"), "a segment")?;
        let start = first(s.get("AnimStartTime"), None).map_or(0.0, |v| number(Some(v)));
        let end = number(s.get("AnimEndTime"));
        let pos = first(s.get("StartPos"), None).map_or(0.0, |v| number(Some(v)));
        segments.push(obj([
            ("anim", Some(Json::from(ctx.anim_path(&anim, false)?))),
            ("start", Some(num(start))),
            ("end", s.get("AnimEndTime").cloned()),
            ("at", Some(num(pos))),
        ]));
        for n in arr(props_of(&ctx.load(&anim)?).get("Notifies")) {
            let t = notify_time(n);
            if t >= start && t < end {
                let nend = truthy(n.get("Duration")).then(|| pos + t + number(n.get("Duration")) - start);
                notify(ctx, &mut g, name, n, pos + t - start, nend, true)?;
            }
        }
    }
    g.list.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut sockets: Vec<String> = Vec::new();
    for (_, j) in &g.list {
        if let Some(s) = j.get("socket").filter(|s| json_truthy_ref(s)).and_then(Json::as_str)
            && !sockets.iter().any(|x| x == s)
        {
            sockets.push(s.to_owned());
        }
    }
    let sections: Vec<Json> = arr(m.get("CompositeSections"))
        .iter()
        .map(|s| {
            obj([
                ("name", s.get("SectionName").cloned()),
                ("at", Some(first(s.get("LinkValue"), None).cloned().unwrap_or(num(0.0)))),
                ("next", s.get("NextSectionName").cloned()),
            ])
        })
        .collect();
    let play = obj([
        ("name", Some(Json::from(name))),
        ("length", m.get("SequenceLength").cloned()),
        ("blendIn", Some(blend(m.get("BlendIn"))?)),
        ("blendOut", Some(blend(m.get("BlendOut"))?)),
        ("segments", Some(Json::Arr(segments))),
        ("sections", Some(Json::Arr(sections))),
        ("notifies", Some(Json::Arr(g.list.into_iter().map(|(_, j)| j).collect()))),
    ]);
    Ok((play, sockets))
}

fn json_truthy_ref(v: &Json) -> bool {
    super::util::json_truthy(v)
}

/// A notify's socket on the ship's mesh: a skeleton socket (SkeletalMeshSocket of the mesh's skeleton; the mesh has
/// none of its own), else the bone of that name (USkeletalMeshComponent's socket lookup falls back to a bone), as
/// { bone, loc, rot, scale }.
fn ship_socket<'a>(ctx: &dyn SceneryBindCtx<'a>, mesh_ref: Option<&Json>, name: &str) -> Result<Json, KindError> {
    let mesh = object_of(ctx, mesh_ref)?;
    let mp = props_of(&mesh);
    if !arr(mp.get("Sockets")).is_empty() {
        return fail(format!("{}: sockets on the mesh", short_name(mesh.path())));
    }
    let skel_path = reference(mp.get("Skeleton"), "Skeleton")?;
    let skel = ctx.load(&skel_path)?;
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
    if let Some(s) = found {
        let r = s.get("RelativeRotation");
        let rot = if truthy(r) {
            Json::Arr(
                ["Pitch", "Yaw", "Roll"]
                    .map(|k| first(r.and_then(|r| r.get(k)), None).cloned().unwrap_or(num(0.0)))
                    .into(),
            )
        } else {
            nums(&[0.0; 3])
        };
        return Ok(obj([
            ("bone", s.get("BoneName").cloned()),
            ("loc", Some(v3d(s.get("RelativeLocation"), [0.0; 3]))),
            ("rot", Some(rot)),
            ("scale", Some(v3d(s.get("RelativeScale"), [1.0; 3]))),
        ]));
    }
    if !skeleton::entry(ctx.cx(), &skel_path)?.bones.iter().any(|b| b == name) {
        return fail(format!("{}: no socket or bone {name}", short_name(mesh.path())));
    }
    Ok(obj([
        ("bone", Some(Json::from(name))),
        ("loc", Some(nums(&[0.0; 3]))),
        ("rot", Some(nums(&[0.0; 3]))),
        ("scale", Some(nums(&[1.0; 3]))),
    ]))
}

// M010_BombFactory_ClockHands_C's numbers: the hands' placed relative rolls (the component templates', no level
// override), the two timelines' lengths and EaseInOut curves.
// UNSTORED_TIMELINE_LENGTH: a timeline whose Length is not stored keeps the UTimelineComponent default, 5.0
// (142e34d70): Rotate Hand B (CX-M010-4).
const UNSTORED_TIMELINE_LENGTH: f64 = 5.0;

fn hands_of<'a>(ctx: &dyn SceneryBindCtx<'a>, owner: &ObjView<'a>) -> Result<JsonProps, KindError> {
    let mut roll = Json::obj();
    for h in ["A", "B"] {
        let t = props_of(&class_inner(ctx, HANDS_CLASS, &format!("SJAM_BombFactory_ClockHand_{h}_GEN_VARIABLE"))?);
        let Some(lv) = level_obj(ctx, owner, &format!("SJAM_BombFactory_ClockHand_{h}")) else {
            return fail(format!("{}: no hand {h}", name_of(owner)));
        };
        let lv = props_of(lv);
        let r = t.get("RelativeRotation");
        let rr = |k: &str| r.and_then(|r| r.get(k));
        if truthy(lv.get("RelativeRotation"))
            || truthy(lv.get("RelativeLocation"))
            || truthy(lv.get("RelativeScale3D"))
            || truthy(t.get("RelativeLocation"))
            || truthy(t.get("RelativeScale3D"))
            || truthy(rr("Pitch"))
            || truthy(rr("Yaw"))
        {
            return fail(format!("{}: hand {h} placed with more than a roll", name_of(owner)));
        }
        roll.set(h.to_lowercase(), first(rr("Roll"), None).cloned().unwrap_or(num(0.0)));
    }
    let mut timelines = Json::obj();
    for h in ["A", "B"] {
        let Some(o) = level_obj(ctx, owner, &format!("Rotate Hand {h}")) else {
            return fail(format!("{}: no timeline {h}", name_of(owner)));
        };
        let op = props_of(o);
        let tl = need(op.get("TheTimeline"), "TheTimeline")?;
        let floats = req_arr("m010", tl.get("InterpFloats"), "InterpFloats")?;
        if !is_str(tl.get("LengthMode"), "TL_TimelineLength")
            || truthy(tl.get("bLooping"))
            || !is_num_or(tl.get("PlayRate"), 1.0, 1.0)
            || floats.len() != 1
            || !arr(tl.get("InterpVectors")).is_empty()
            || !arr(tl.get("Events")).is_empty()
        {
            return fail(format!("{}: timeline {h}", name_of(owner)));
        }
        let curve = props_of(&object_of(ctx, floats[0].get("FloatCurve"))?);
        let keys = curve_keys(curve.get("FloatCurve"))?;
        timelines.set(
            h.to_lowercase(),
            obj([
                ("length", Some(first(tl.get("Length"), None).cloned().unwrap_or(num(UNSTORED_TIMELINE_LENGTH)))),
                ("keys", Some(Json::Arr(keys.iter().map(|k| nums(k)).collect()))),
            ]),
        );
    }
    let mut out = JsonProps::new();
    out.insert("roll".into(), roll);
    out.insert("random".into(), nums(&[0.0, 360.0]));
    out.insert("idle".into(), Json::object([("every", num(0.4)), ("a", num(12.0)), ("b", num(-8.0))]));
    out.insert("rotate".into(), Json::object([("a", num(770.0)), ("b", num(-770.0)), ("delayB", num(0.5))]));
    out.insert("timelines".into(), timelines);
    Ok(out)
}

// SpaceJam_AnimSeq_2's sequence (M010_AnimSeq): its one binding (M010_BalloonBoat_01 of Map_M010_Animation), its
// playback range, the transform section's nine channels and the skeletal section, times in seconds (TickResolution
// 24000). A FMovieSceneFloatChannel key: [time, value, arrive, leave, mode] in the scenery entry's CurveKey form (its
// tangents per tick times the resolution: per second; mode ERichCurveInterpMode 0 linear, 1 constant, 2 cubic as 0, 2,
// 1); a channel with no key holds its DefaultValue.
fn seq_mode(mode: f64) -> Option<f64> {
    [(0.0, 0.0), (1.0, 2.0), (2.0, 1.0)].iter().find(|(k, _)| *k == mode).map(|(_, v)| *v)
}

fn member<'j>(v: Option<&'j Json>, k: &str) -> Option<&'j Json> {
    v.and_then(|v| v.get(k))
}

fn sequence_of<'a>(ctx: &dyn SceneryBindCtx<'a>, binding: &str) -> Result<JsonProps, KindError> {
    let seq_obj = ctx.load(&at(ANIMS, "M010_AnimSeq"))?;
    let sp = props_of(&seq_obj);
    let refs = req_arr("m010", member(sp.get("BindingReferences"), "BindingIdToReferences"), "BindingIdToReferences")?;
    let r0 = refs
        .first()
        .and_then(|r| r.as_array())
        .and_then(|p| p.get(1))
        .and_then(|p| p.get("References"))
        .and_then(Json::as_array);
    let path = r0
        .and_then(|r| r.first())
        .and_then(|r| r.get("ExternalObjectPath"))
        .and_then(Json::as_str)
        .unwrap_or("undefined");
    if refs.len() != 1 || r0.map(<[Json]>::len) != Some(1) || !path.ends_with(&format!(":PersistentLevel.{binding}")) {
        return fail("M010_AnimSeq: binding");
    }
    let inner = |n: &str| -> Result<JsonProps, KindError> {
        match seq_obj.inner_named(n)? {
            Some(o) => Ok(props_of(&o)),
            None => fail(format!("M010_AnimSeq: no {n}")),
        }
    };
    let ms = props_of(&object_of(ctx, sp.get("MovieScene"))?);
    let tr = ms.get("TickResolution");
    let res = number(member(tr, "Numerator")) / first(member(tr, "Denominator"), None).map_or(1.0, |d| number(Some(d)));
    let range = member(ms.get("PlaybackRange"), "Value");
    let lb = member(range, "LowerBound");
    let ub = member(range, "UpperBound");
    if !is_num_or(member(lb, "Type"), f64::NAN, 1.0) || !is_num_or(member(ub, "Type"), f64::NAN, 0.0) {
        return fail("M010_AnimSeq: range bounds");
    }
    let bindings = req_arr("m010", ms.get("ObjectBindings"), "ObjectBindings")?;
    if bindings.len() != 1 || req_arr("m010", bindings[0].get("Tracks"), "Tracks")?.len() != 2 {
        return fail("M010_AnimSeq: tracks");
    }
    let ts = inner("MovieScene_0.MovieScene3DTransformTrack_0.MovieScene3DTransformSection_0")?;
    // (the section's range is open on both sides: always)
    let tsr = member(ts.get("SectionRange"), "Value");
    if !is_num_or(member(member(tsr, "LowerBound"), "Type"), f64::NAN, 2.0)
        || !is_num_or(member(member(tsr, "UpperBound"), "Type"), f64::NAN, 2.0)
        || truthy(ts.get("BlendType"))
        || truthy(ts.get("Mask"))
    {
        return fail("M010_AnimSeq: transform section");
    }
    let channel = |c: Option<&Json>| -> Result<Json, KindError> {
        let c = need(c, "a channel")?;
        if !is_str(c.get("PreInfinityExtrap"), "RCCE_Constant") || !is_str(c.get("PostInfinityExtrap"), "RCCE_Constant")
        {
            return fail("M010_AnimSeq: extrapolation");
        }
        let times = arr(c.get("Times"));
        if times.is_empty() {
            return Ok(Json::Arr(vec![Json::Arr(vec![
                num(0.0),
                c.get("DefaultValue").cloned().unwrap_or_default(),
                num(0.0),
                num(0.0),
                num(2.0),
            ])]));
        }
        let values = req_arr("m010", c.get("Values"), "Values")?;
        let mut keys = Vec::new();
        for (i, t) in times.iter().enumerate() {
            let v = need(values.get(i), "a channel value")?;
            let tg = need(v.get("Tangent"), "a tangent")?;
            let mode = v.get("InterpMode").and_then(Json::as_f64).and_then(seq_mode);
            let Some(mode) = mode.filter(|_| !truthy(tg.get("TangentWeightMode"))) else {
                return fail("M010_AnimSeq: key mode");
            };
            keys.push(Json::Arr(vec![
                num(number(t.get("Value")) / res),
                v.get("Value").cloned().unwrap_or_default(),
                num(number(tg.get("ArriveTangent")) * res),
                num(number(tg.get("LeaveTangent")) * res),
                num(mode),
            ]));
        }
        Ok(Json::Arr(keys))
    };
    let ch = |n: &str| -> Result<Json, KindError> {
        Ok(Json::Arr(vec![
            channel(ts.get(n))?,
            channel(ts.get(&format!("{n}[1]")))?,
            channel(ts.get(&format!("{n}[2]")))?,
        ]))
    };
    let sk = inner("MovieScene_0.MovieSceneSkeletalAnimationTrack_0.MovieSceneSkeletalAnimationSection_0")?;
    let p = need(sk.get("Params"), "Params")?;
    let sr = member(sk.get("SectionRange"), "Value");
    let off = |k: &str| truthy(member(p.get(k), "Value"));
    let w = need(p.get("Weight"), "Weight")?;
    if !is_num_or(member(sr, "LowerBound").and_then(|b| b.get("Type")), f64::NAN, 1.0)
        || !is_num_or(member(sr, "UpperBound").and_then(|b| b.get("Type")), f64::NAN, 0.0)
        || off("StartFrameOffset")
        || off("EndFrameOffset")
        || off("FirstLoopStartFrameOffset")
        || !is_num_or(p.get("PlayRate"), 1.0, 1.0)
        || truthy(p.get("bReverse"))
        || !arr(w.get("Times")).is_empty()
        || w.get("DefaultValue").and_then(Json::as_f64) != Some(1.0)
    {
        return fail("M010_AnimSeq: skeletal section");
    }
    let translation = ch("Translation")?;
    let rotation = ch("Rotation")?;
    let scale = ch("Scale")?;
    let anim = ctx.anim_path(&reference(p.get("Animation"), "the sequence animation")?, false)?;
    let frame = |b: Option<&Json>| num(number(member(member(b, "Value"), "Value")) / res);
    let mut out = JsonProps::new();
    out.insert("start".into(), frame(lb));
    out.insert("end".into(), frame(ub));
    out.insert("translation".into(), translation);
    out.insert("rotation".into(), rotation);
    out.insert("scale".into(), scale);
    out.insert(
        "anim".into(),
        Json::object([
            ("path", Json::from(anim)),
            ("from", frame(member(sr, "LowerBound"))),
            ("to", frame(member(sr, "UpperBound"))),
            ("rate", num(1.0)),
        ]),
    );
    Ok(out)
}

// The three pickups' systems (PickupPowerup_<type>_C, spawned by the level's PowerupSpawner: no placed actor), by
// class: the type component PS_M010_Powerup_<type> (on the root, location and rotation 0, hidden in game: its own
// template draws nothing), whose scale the intro and type particles spawned on it take; the outro; the buff's
// particles (Pickup<type>Buff_C).
fn pickups_of<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<Json, KindError> {
    let mut kinds = Json::obj();
    for t in PICKUP_TYPES {
        let cls = ctx.load(&format!("{M010}PickupPowerup_{t}.PickupPowerup_{t}_C"))?;
        let comp = cls.inner_named(&format!("PS_M010_Powerup_{t}_GEN_VARIABLE"))?;
        let p = comp.as_ref().map(props_of).unwrap_or_default();
        if comp.is_none()
            || truthy(p.get("RelativeLocation"))
            || truthy(p.get("RelativeRotation"))
            || !truthy(p.get("bHiddenInGame"))
            || !truthy(p.get("RelativeScale3D"))
            || p.get("Template").and_then(Json::as_str) != Some(sys(&format!("PS_M010_Powerup_{t}")).as_str())
        {
            return fail(format!("PickupPowerup_{t}: type component"));
        }
        let mut node = None;
        for x in cls.inner()? {
            if x.class().ends_with(".SCS_Node") {
                let xp = props_of(&x);
                if is_str(xp.get("InternalVariableName"), &format!("PS_M010_Powerup_{t}")) {
                    node = Some(xp);
                    break;
                }
            }
        }
        if !node.is_some_and(|n| is_str(n.get("ParentComponentOrVariableName"), "Capsule")) {
            return fail(format!("PickupPowerup_{t}: the type component is not on the root"));
        }
        kinds.set(
            format!("PickupPowerup_{t}_C"),
            Json::object([
                ("type", Json::from(ctx.add_system(&sys(&format!("PS_M010_Powerup_{t}")))?)),
                ("scale", v3(p.get("RelativeScale3D"))?),
                ("outro", Json::from(ctx.add_system(&sys(&format!("PS_M010_Powerup_{t}_Outro")))?)),
                ("buff", Json::from(format!("Pickup{t}Buff_C"))),
                ("buffSystem", Json::from(ctx.add_system(&sys(&format!("PS_C016_PowerUp_{t}_Buff")))?)),
            ]),
        );
    }
    Ok(Json::object([("intro", Json::from(ctx.add_system(&sys("PS_M010_Powerup_Intro"))?)), ("kinds", kinds)]))
}

/// The effects the drawn actors' graphs spawn, converted with the entry's systems.
struct Systems {
    clock: String,
    clock1: String,
    explosion: String,
    touch: String,
    impact: String,
    barrier: String,
}

pub struct TheCourt;

impl SceneryPart for TheCourt {
    fn name(&self) -> &'static str {
        "m010"
    }
    fn blueprints(&self) -> &'static [&'static str] {
        &[
            "M010_Spotlight_BP_C",
            "M010_ShotClock_C",
            "M010_Hoop_A_01_BP_C",
            "m010_wall_C",
            "M010_JumboTron_C",
            "M010_MG_Ship_C",
            "M010_BombFactory_ClockHands_C",
        ]
    }
    fn levels(&self) -> &'static [&'static str] {
        &["Map_M010_Animation_C"]
    }
    fn ignore(&self) -> &'static [(&'static str, &'static str)] {
        // Map_M010_VFX @10..354: destroys the eight spotlights only on XboxOne or PlayStation4
        &[("Map_M010_VFX_C", "destroys the spotlights only on XboxOne or PlayStation4: neither here")]
    }
    fn keep_params(&self) -> &'static [&'static str] {
        &["/Game/Panda_Main/Maps/M010/VFX/M010_Barrier_Side_MESH_MAT.M010_Barrier_Side_MESH_MAT"]
    }
    fn gameplay_particles(&self) -> &'static [&'static str] {
        &["M010_Hoop_A_01_BP_C"]
    }
    /// The wall's BeginPlay attach of its ConnectedBarrierWall (@1520; EAttachmentRule location 1 KeepWorld, rotation 0
    /// KeepRelative, scale 1 KeepWorld).
    fn begin_attach<'a>(
        &self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
    ) -> Result<Vec<SceneryBeginAttach<'a>>, KindError> {
        if cls != WALL {
            return Ok(Vec::new());
        }
        let barrier = tail(props_of(owner).get("ConnectedBarrierWall"));
        let root = level_obj(ctx, owner, "DefaultSceneRoot");
        let (Some(barrier), Some(root)) = (barrier, root) else {
            return fail(format!("{}: connected barrier / root", name_of(owner)));
        };
        Ok(vec![SceneryBeginAttach {
            actor: barrier,
            parent: root.clone(),
            props: Box::new(props_of_actor(ctx, owner)?),
            rules: [1, 0, 1],
        }])
    }
    fn run<'a>(&self) -> Box<dyn SceneryPartRun<'a> + 'a> {
        Box::new(Run::default())
    }
}

#[derive(Default)]
struct Run {
    spotlights: Vec<Json>,
    shot_clock: Option<JsonProps>,
    hoops: Vec<JsonProps>,
    walls: Vec<JsonProps>,
    jumbotron: Option<Json>,
    ship: Option<Json>,
    clock_hands: Option<JsonProps>,
    sequence: Option<JsonProps>,
    systems: Option<Systems>,
    pickups: Option<Json>,
    /// The basketball's instances by material colour, in the order written.
    ball: Option<Vec<(&'static str, String)>>,
}

impl Run {
    /// The basketball's BasketballGeo, one hidden instance per material, with the entry's first context.
    fn ball_of<'a>(&mut self, ctx: &dyn SceneryBindCtx<'a>) -> Result<(), KindError> {
        if self.ball.is_some() {
            return Ok(());
        }
        let geo = props_of(&class_inner(ctx, BALL_CLASS, "BasketballGeo_GEN_VARIABLE")?);
        let over = arr(geo.get("OverrideMaterials"));
        if !is_str(geo.get("StaticMesh"), BALL_MESH)
            || tail(over.first()).as_deref() != Some(BALL_MATS[0].1)
            || over.len() != 1
        {
            return fail(format!("{BALL}: BasketballGeo"));
        }
        if ctx.mesh_header(BALL_MESH)?.slots.len() != 1 {
            return fail(format!("{BALL}: mesh slots"));
        }
        let mesh = ctx.add_mesh(BALL_MESH)?;
        let (r, l) = (geo.get("RelativeRotation"), geo.get("RelativeLocation"));
        let any = |v: Option<&Json>, ks: [&str; 3]| ks.iter().any(|k| truthy(member(v, k)));
        let off_capsule = geo.get("AttachParent").and_then(Json::as_str).is_some_and(|a| !a.contains("Capsule"));
        if (truthy(r) && any(r, ["Pitch", "Yaw", "Roll"])) || (truthy(l) && any(l, ["X", "Y", "Z"])) || off_capsule {
            return fail(format!("{BALL}: BasketballGeo placed"));
        }
        let mut made = Vec::new();
        for (v, mi) in BALL_MATS {
            let key =
                ctx.build_for(&[Some(at(&format!("{M010}Materials/"), mi))], None, None)?.into_iter().next().flatten();
            let name = format!("{BALL}.BasketballGeo.{v}");
            // (UPrimitiveComponent defaults: channel 0, CastShadow true; the template sets neither)
            ctx.add_instance(JsonProps::from([
                ("name".to_owned(), Json::from(name.as_str())),
                ("mesh".to_owned(), num(mesh as f64)),
                ("loc".to_owned(), nums(&[0.0; 3])),
                ("rot".to_owned(), nums(&[0.0; 3])),
                ("scale".to_owned(), nums(&[1.0; 3])),
                ("materials".to_owned(), Json::Arr(vec![key.map_or(Json::Null, Json::from)])),
                ("channels".to_owned(), Json::Arr(vec![Json::Bool(true), Json::Bool(false), Json::Bool(false)])),
                ("castShadow".to_owned(), Json::Bool(true)),
                ("mobility".to_owned(), Json::from("Movable")),
            ]));
            made.push((v, name));
        }
        self.ball = Some(made);
        Ok(())
    }

    fn systems_of<'a>(&mut self, ctx: &dyn SceneryBindCtx<'a>) -> Result<(), KindError> {
        if self.systems.is_none() {
            self.systems = Some(Systems {
                clock: ctx.add_system(&sys("PS_M010_Clock"))?,
                clock1: ctx.add_system(&sys("PS_M010_Clock1"))?,
                explosion: ctx.add_system(&sys("PS_Hoop_Explosion"))?,
                touch: ctx.add_system(&sys("PS_M010_Barrier_Touch"))?,
                impact: ctx.add_system(&sys("PS_M010_WallImpact"))?,
                barrier: ctx.add_system(&sys("PS_M010_Barrier"))?,
            });
        }
        if self.pickups.is_none() {
            self.pickups = Some(pickups_of(ctx)?);
        }
        Ok(())
    }

    fn hoop_of<'a>(&mut self, ctx: &dyn SceneryBindCtx<'a>, owner: &ObjView<'a>) -> Result<&mut JsonProps, KindError> {
        let at = match self.hoops.iter().position(|h| h.get("actor").and_then(Json::as_str) == Some(name_of(owner))) {
            Some(i) => i,
            None => {
                let index = first(vars_of(ctx, owner)?.get("HoopIndex"), None).cloned().unwrap_or(num(0.0));
                self.hoops.push(JsonProps::from([
                    ("actor".to_owned(), Json::from(name_of(owner))),
                    ("index".to_owned(), index),
                ]));
                self.hoops.len() - 1
            }
        };
        Ok(&mut self.hoops[at])
    }

    fn spotlight_params<'a>(
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        q: &JsonProps,
    ) -> Result<JsonProps, KindError> {
        // the construction script's parameters (the actor's fields), as the level stores them
        let vars = vars_of(ctx, owner)?;
        let mut params = JsonProps::new();
        for n in ["LightRay", "Alpha", "Flare_Alpha", "Ray02_Alpha"] {
            params.insert(
                n.into(),
                vars.get(n)
                    .cloned()
                    .ok_or_else(|| KindError::Failed(format!("scenery m010: {}: {n}", name_of(owner))))?,
            );
        }
        for n in ["Size", "Color", "Ray02_Size", "Ray02_Color"] {
            params.insert(n.into(), v3(vars.get(n))?);
        }
        let stored = arr(q.get("InstanceParameters"));
        for (n, v) in &params {
            let ip = stored.iter().find(|x| is_str(x.get("Name"), n));
            let sv = match ip {
                Some(ip) if is_str(ip.get("ParamType"), "PSPT_Scalar") => ip.get("Scalar").cloned(),
                Some(ip) if is_str(ip.get("ParamType"), "PSPT_Vector") => Some(v3(ip.get("Vector"))?),
                _ => None,
            };
            let ok = match (&sv, v) {
                (Some(Json::Num(b)), Json::Num(a)) => same_f32(*a, *b),
                (Some(Json::Arr(b)), Json::Arr(a)) => {
                    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_f32(number(Some(x)), number(Some(y))))
                }
                _ => false,
            };
            if !ok || ip.is_some_and(|ip| truthy(ip.get("Actor")) || truthy(ip.get("Material"))) {
                return fail(format!("{}: instance parameter {n} is not the construction script's", name_of(owner)));
            }
        }
        if stored.len() != params.len() {
            return fail(format!("{}: instance parameters", name_of(owner)));
        }
        Ok(params)
    }
}

impl<'a> SceneryPartRun<'a> for Run {
    fn level(&mut self, cls: &str, ctx: &dyn SceneryBindCtx<'a>) -> Result<SceneryLevelRun, KindError> {
        if cls != ANIMATION_LEVEL {
            return fail(format!("level {cls}"));
        }
        let objects = &ctx.level().objects;
        let seq = objects.iter().find(|x| x.class().ends_with(".LevelSequenceActor"));
        let binding =
            objects.iter().find(|x| x.class().ends_with(".SkeletalMeshActor") && name_of(x) == "M010_BalloonBoat_01");
        let (Some(seq), Some(binding)) = (seq, binding) else {
            return fail(format!("{cls}: the sequence or its binding"));
        };
        let mut sequence = JsonProps::new();
        sequence.insert("actor".into(), Json::from(name_of(seq)));
        sequence.insert("skeletal".into(), Json::from(name_of(binding)));
        sequence.insert("loop".into(), Json::Bool(true));
        sequence.extend(sequence_of(ctx, name_of(binding))?);
        self.sequence = Some(sequence);
        Ok(SceneryLevelRun::default())
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
        let own = props_of(owner);
        if cls == WALL && name_of(e) == "WallGlow" {
            // (the actor is hidden: its components draw nothing)
            if !truthy(own.get("bHidden")) {
                return fail(format!("{}: the wall is not hidden", name_of(owner)));
            }
            self.systems_of(ctx)?;
            let barrier = tail(own.get("ConnectedBarrierWall"));
            let floor_glow = tail(own.get("ConnectedFloorGlow"));
            let mesh = |a: &str| {
                ctx.level().objects.iter().find(|x| x.class().ends_with(".StaticMeshComponent") && outer_name(x) == a)
            };
            let bm = barrier.as_deref().and_then(mesh);
            let ok = bm.is_some_and(|bm| {
                arr(props_of(bm).get("OverrideMaterials")).first().and_then(Json::as_str) == Some(BARRIER_MAT)
            });
            let (Some(barrier), Some(floor_glow), true) = (barrier, floor_glow, ok) else {
                return fail(format!("{}: connected barrier / floor glow", name_of(owner)));
            };
            if mesh(&floor_glow).is_none() {
                return fail(format!("{}: connected barrier / floor glow", name_of(owner)));
            }
            // (the root's place at BeginPlay, the Ak component's under it: the ambience's place; DamageVFXRotation, the
            // impact's rotation @1843)
            let (root, ak) = (level_obj(ctx, owner, "DefaultSceneRoot"), level_obj(ctx, owner, "Ak"));
            let vars = vars_of(ctx, owner)?;
            let rot = vars.get("DamageVFXRotation").filter(|r| matches!(r, Json::Obj(_)));
            let (Some(root), Some(ak), Some(rot)) = (root, ak, rot) else {
                return fail(format!("{}: root / Ak / DamageVFXRotation", name_of(owner)));
            };
            let objects = &ctx.level().objects;
            let mut w = JsonProps::new();
            w.insert("actor".into(), Json::from(name_of(owner)));
            w.insert("left".into(), Json::Bool(truthy(vars.get("IsLeftWall"))));
            w.insert("barrier".into(), Json::from(barrier));
            w.insert("floorGlow".into(), Json::from(floor_glow));
            w.insert("root".into(), nums(&xf_out(&world_of(objects, root, &props)?).loc));
            w.insert("ak".into(), nums(&xf_out(&world_of(objects, ak, &props)?).loc));
            w.insert("impactRot".into(), r3(Some(rot)));
            w.insert("ambience".into(), Json::from(BARRIER_AMBIENCE));
            self.walls.push(w);
            let mut out = p.clone();
            out.insert("bHiddenInGame".into(), Json::Bool(true));
            return Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: out, extra: JsonProps::new(), placed: true }));
        }
        let is_hand = |n: &str| matches!(n, "SJAM_BombFactory_ClockHand_A" | "SJAM_BombFactory_ClockHand_B");
        if cls != HANDS || !is_hand(name_of(e)) {
            return fail(format!("{}: static mesh component {} of {cls}", name_of(owner), name_of(e)));
        }
        if self.clock_hands.is_none() {
            let mut hands = JsonProps::new();
            hands.insert("actor".into(), Json::from(name_of(owner)));
            hands.extend(hands_of(ctx, owner)?);
            self.clock_hands = Some(hands);
        }
        let last = name_of(e)[name_of(e).len() - 1..].to_lowercase();
        if let Some(h) = &mut self.clock_hands {
            h.insert(last, Json::from(format!("{}.{}", name_of(owner), name_of(e))));
        }
        let mut out = p.clone();
        out.extend(ctx.place_of(e, Some(&props))?);
        Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: out, extra: JsonProps::new(), placed: true }))
    }

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
            return fail(format!("{}.{}: particle component", name_of(owner), name_of(e)));
        }
        self.systems_of(ctx)?;
        let o = xf_out(&world_of(&ctx.level().objects, e, &props)?);
        let mut entry = JsonProps::new();
        entry.insert("name".into(), Json::from(format!("{}.{}", name_of(owner), name_of(e))));
        entry.insert("system".into(), Json::from(ctx.add_system(&reference(q.get("Template"), "Template")?)?));
        entry.insert("loc".into(), nums(&o.loc));
        entry.insert("rot".into(), nums(&o.rot));
        entry.insert("scale".into(), nums(&o.scale));
        let entry_name = entry["name"].clone();
        if cls == SPOTLIGHT && name_of(e) == "PS_Spotlight_Ray" {
            let params = Self::spotlight_params(ctx, owner, q)?;
            let mut emitter = entry.clone();
            emitter.insert("params".into(), Json::Obj(params));
            ctx.add_emitter(emitter);
            self.spotlights.push(Json::object([("actor", Json::from(name_of(owner))), ("emitter", entry_name)]));
            return Ok(true);
        }
        if cls == SHOT_CLOCK && name_of(e) == "PS_Spacejam_Clock" {
            if self.shot_clock.is_some() {
                return fail("two shot clocks");
            }
            let at = Json::object([("loc", nums(&o.loc)), ("rot", nums(&o.rot)), ("scale", nums(&o.scale))]);
            ctx.add_emitter(entry);
            self.shot_clock = Some(JsonProps::from([
                ("actor".to_owned(), Json::from(name_of(owner))),
                ("emitter".to_owned(), entry_name),
                ("at".to_owned(), at),
            ]));
            return Ok(true);
        }
        if cls == HOOP && name_of(e) == "PS_SpacejamHoopArrows" {
            ctx.add_emitter(entry);
            self.hoop_of(ctx, owner)?.insert("emitter".into(), entry_name);
            return Ok(true);
        }
        Ok(false)
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
        let name = format!("{}.{}", name_of(owner), name_of(e));
        if cls == SPOTLIGHT {
            if name_of(e) != "SJAM_Spotlight_A_01" {
                return Ok(false);
            }
            let idle2 = truthy(vars_of(ctx, owner)?.get("Idle02"));
            let want = if idle2 { "SJAM_Spotlight_Anim_Idle_v2" } else { "SJAM_Spotlight_Anim_Idle_v1" };
            let d = p.get("AnimationData");
            let dm = |k: &str| member(d, k);
            if !is_str(p.get("AnimationMode"), "EAnimationMode::AnimationSingleNode")
                || tail(dm("AnimToPlay")).as_deref() != Some(want)
                || dm("bSavedLooping") == Some(&Json::Bool(false))
                || dm("bSavedPlaying") == Some(&Json::Bool(false))
                || truthy(dm("SavedPosition"))
                || !is_num_or(dm("SavedPlayRate"), 1.0, 1.0)
            {
                return fail(format!("{name}: not the construction script's animation"));
            }
            ctx.add_skeletal(&name, &p, &ctx.place_of(e, Some(&props))?, &SkeletalOpts::default())?;
            return Ok(true);
        }
        if cls == HOOP {
            if name_of(e) != "SkeletalMesh" {
                return Ok(false);
            }
            ctx.add_skeletal(&name, &p, &ctx.place_of(e, Some(&props))?, &SkeletalOpts::default())?;
            let mut plays = Vec::new();
            for (off, event, a) in HOOP_PLAYS {
                plays.push(Json::object([
                    ("event", Json::from(event)),
                    ("at", num(f64::from(off))),
                    ("anim", Json::from(ctx.anim_path(&at(ANIMS, a), false)?)),
                ]));
            }
            let h = self.hoop_of(ctx, owner)?;
            h.insert("skeletal".into(), Json::from(name));
            h.insert("plays".into(), Json::Arr(plays));
            return Ok(true);
        }
        if cls == JUMBOTRON {
            if name_of(e) != "SkeletalMeshComponent0" || self.jumbotron.is_some() {
                return Ok(false);
            }
            self.ball_of(ctx)?;
            ctx.add_skeletal(&name, &p, &ctx.place_of(e, Some(&props))?, &SkeletalOpts::default())?;
            // its WidgetComponent UI (World space, the plane geometry): the widget class, draw size and pivot, the
            // blend mode, its world place
            let ui_path = format!("{}.UI", owner.path());
            let ui_e =
                ctx.level().objects.iter().find(|x| x.class().ends_with(".WidgetComponent") && x.path() == ui_path);
            let ui = ui_e.map(|u| props.get(u)).transpose()?;
            let ok = ui.is_some_and(|ui| {
                tail(ui.get("WidgetClass")).as_deref() == Some("UI_SpaceJam_Scoreboard_C")
                    && !["Space", "GeometryMode", "Pivot", "bIsTwoSided", "TintColorAndOpacity", "bDrawAtDesiredSize"]
                        .iter()
                        .any(|k| truthy(ui.get(*k)))
                    && ui.get("OpacityFromTexture").is_none()
            });
            let (Some(ui_e), Some(ui), true) = (ui_e, ui, ok) else { return fail(format!("{}: UI", name_of(owner))) };
            // (UWidgetComponent's default draw size)
            let size = ui.get("DrawSize").filter(|s| !matches!(s, Json::Null));
            let size = |k: &str| match size {
                Some(s) => num(number(s.get(k))),
                None => num(500.0),
            };
            let blend = ui.get("BlendMode").and_then(Json::as_str).unwrap_or("::Masked");
            let blend = blend
                .rfind("::")
                .map(|i| &blend[i + 2..])
                .filter(|b| !b.is_empty() && b.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
            let Some(blend) = blend else { return fail(format!("{}: UI blend mode", name_of(owner))) };
            let world = xf_out(&world_of(&ctx.level().objects, ui_e, &props)?);
            let ui_json = Json::object([
                ("widget", Json::from("UI_SpaceJam_Scoreboard")),
                ("size", Json::Arr(vec![size("X"), size("Y")])),
                ("pivot", nums(&[0.5, 0.5])),
                ("blend", Json::from(blend)),
                ("loc", nums(&world.loc)),
                ("rot", nums(&world.rot)),
                ("scale", nums(&world.scale)),
            ]);
            self.jumbotron = Some(Json::object([
                ("actor", Json::from(name_of(owner))),
                ("skeletal", Json::from(name)),
                ("ui", ui_json),
            ]));
            return Ok(true);
        }
        if cls == SHIP {
            if name_of(e) != "Mesh" || self.ship.is_some() {
                return Ok(false);
            }
            let opts = SkeletalOpts { anim_bp: Some(SHIP_ANIM_BP.to_owned()), ..SkeletalOpts::default() };
            ctx.add_skeletal(&name, &p, &ctx.place_of(e, Some(&props))?, &opts)?;
            let (landing, landing_sockets) = montage_of(ctx, "M010Ship_Anim_Landing_Montage")?;
            let (take_off, take_off_sockets) = montage_of(ctx, "M010Ship_Anim_TakeOff_Montage")?;
            let mut socket_names: Vec<String> = Vec::new();
            for s in landing_sockets.into_iter().chain(take_off_sockets) {
                if !socket_names.contains(&s) {
                    socket_names.push(s);
                }
            }
            let Some(root) = level_obj(ctx, owner, "DefaultSceneRoot") else {
                return fail(format!("{}: root", name_of(owner)));
            };
            let mut sockets = Json::obj();
            for s in &socket_names {
                sockets.set(s.as_str(), ship_socket(ctx, p.get("SkeletalMesh"), s)?);
            }
            self.ship = Some(Json::object([
                ("actor", Json::from(name_of(owner))),
                ("skeletal", Json::from(name)),
                ("landing", landing),
                ("takeOff", take_off),
                ("sockets", sockets),
                ("root", nums(&xf_out(&world_of(&ctx.level().objects, root, &props)?).loc)),
            ]));
            return Ok(true);
        }
        Ok(false)
    }

    fn actor_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut b = JsonProps::new();
        let Some(shot_clock) = &self.shot_clock else { return Ok(b) };
        let hands_done = self.clock_hands.as_ref().is_some_and(|h| truthy(h.get("a")) && truthy(h.get("b")));
        let hoops_done =
            self.hoops.len() == 2 && self.hoops.iter().all(|h| truthy(h.get("skeletal")) && truthy(h.get("emitter")));
        let (Some(ball), Some(systems), Some(pickups), Some(jumbotron), Some(ship), Some(sequence)) =
            (&self.ball, &self.systems, &self.pickups, &self.jumbotron, &self.ship, &self.sequence)
        else {
            return fail("The Court: actors");
        };
        if self.spotlights.len() != 8 || !hoops_done || !hands_done {
            return fail("The Court: actors");
        }
        // (one PS_M010_Clock per hoop, @357 in the loop over GetAllActorsOfClass)
        let mut shot = shot_clock.clone();
        shot.insert("begin".into(), Json::Arr(self.hoops.iter().map(|_| Json::from(systems.clock.as_str())).collect()));
        shot.insert(
            "finished".into(),
            Json::object([("delay", num(24.0)), ("system", Json::from(systems.clock1.as_str()))]),
        );
        shot.insert(
            "tick".into(),
            Json::object([("delay", num(20.0)), ("event", Json::from("sfx_map_M010_shotclock_tick"))]),
        );
        let mut hoops = self.hoops.clone();
        hoops.sort_by(|a, b| {
            number(a.get("index")).partial_cmp(&number(b.get("index"))).unwrap_or(std::cmp::Ordering::Equal)
        });
        let hoops: Vec<Json> = hoops
            .into_iter()
            .map(|mut h| {
                h.insert("explosion".into(), Json::from(systems.explosion.as_str()));
                Json::Obj(h)
            })
            .collect();
        let walls: Vec<Json> = self
            .walls
            .iter()
            .map(|w| {
                let mut w = w.clone();
                w.insert("material".into(), Json::from(BARRIER_MAT));
                w.insert("touch".into(), Json::from(systems.touch.as_str()));
                w.insert("impact".into(), Json::from(systems.impact.as_str()));
                Json::Obj(w)
            })
            .collect();
        let court = Json::object([
            ("spotlights", Json::Arr(self.spotlights.clone())),
            ("shotClock", Json::Obj(shot)),
            ("hoops", Json::Arr(hoops)),
            ("walls", Json::Arr(walls)),
            ("jumbotron", jumbotron.clone()),
            ("ship", ship.clone()),
            ("clockHands", Json::Obj(self.clock_hands.clone().unwrap_or_default())),
            ("sequence", Json::Obj(sequence.clone())),
            ("knockout", Json::object([("system", Json::from(systems.barrier.as_str())), ("x", num(-2300.0))])),
            ("pickups", pickups.clone()),
            ("ball", Json::object(ball.iter().map(|(k, v)| (*k, Json::from(v.as_str()))))),
        ]);
        b.insert("court".into(), court);
        Ok(b)
    }

    // (the gameplay level's walls: read off the actor, which the core skips but for its WallGlow)
    fn summary(&self) -> String {
        format!(
            ", The Court {} spotlights, {} hoops, {} walls",
            self.spotlights.len(),
            self.hoops.len(),
            self.walls.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_and_checks() {
        // the animation level's sequence modes: ERichCurveInterpMode 0 linear, 1 constant, 2 cubic as 0, 2, 1
        assert_eq!(
            (seq_mode(0.0), seq_mode(1.0), seq_mode(2.0), seq_mode(3.0)),
            (Some(0.0), Some(2.0), Some(1.0), None)
        );
        assert!(same_f32(0.1, f64::from(0.1_f32)) && !same_f32(0.1, 0.2));
        assert!(HOOP_PLAYS.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(sys("PS_M010_Clock"), "/Game/Panda_Main/Maps/M010/VFX/PS_M010_Clock.PS_M010_Clock");
        assert_eq!(BARRIER_MAT, sys("M010_Barrier_Side_MESH_MAT"));
        assert_eq!(TheCourt.keep_params(), [BARRIER_MAT]);
        assert!(v3(Some(&Json::Null)).is_err());
        let mut run = Run::default();
        assert_eq!(run.summary(), ", The Court 0 spotlights, 0 hoops, 0 walls");
        assert!(run.actor_blocks().unwrap().is_empty());
    }
}
