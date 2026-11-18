//! Tree Fort's scenery part (`treehouse`, `treehousesmall`, `treehouse_1plat_large`): its own Blueprint actors and
//! level Blueprints, written at their place and pose at BeginPlay, with what moves them kept for the renderer's blocks
//! (`treefort`, the level Blueprints'; `treefortActors`, the actors').

use crate::game::MAPS_ROOT;
use crate::json::Json;
use crate::kind::KindError;
use crate::kinds::scenery::part::{
    InstanceActor, JsonProps, SceneryBindCtx, SceneryLevelRun, SceneryMeshOut, SceneryPart, SceneryPartRun,
    SkeletalOpts, SkeletalPlay, StaticMeshOut,
};
use crate::objview::{ObjView, short_name};

use super::util::{
    ActorProps, as_props, cdo_props, curve_keys, is_struct, name_of, number, outer_name, over_props, parallax_of,
    props_of, props_of_actor, reference, rel_of, rot_of, short_ref, vars_of, vec, world_of, xf_out,
};

use super::util::js::{self, arr, at, fail, first, is_str, nn, num_or, or_num, put, put_opt, req_arr, triple, truthy};

const PART: &str = "treehouse";

const PILLAR: [&str; 2] = ["TreehouseSidePillar_C", "TreehouseSidePillar_Child_C"];
const UPPER: [&str; 2] = ["TreehouseUpperPlatform_C", "TreehouseUpperPlatform_1v1_C"];
const SMALL: [&str; 3] =
    ["TreehouseSmallPlatform_C", "TreehouseSmallPlatform_Child_C", "M002_TreehouseSmallPlatform_Night_01_C"];
const LADY: &str = "Lady_Rainicorn_Blueprint_C";
const SPIN: &str = "MoutainCloudSpin_C";
const TRAMPOLINE: &str = "TreeFortTrampoline_C";

// The level Blueprints: BeginPlay's Delay(self, 0.5) and BlackoutSphere's K2_DestroyActor (Map_TreeHouse @15 / @70;
// Map_TreeHouseSmall and Map_TreeHouse_1Plat_Large @344 / @16); OnRep_ServerSnail's three SetActorHiddenInGame(false)
// (@149, @191, @233; only the third's case calls PlaySnail, @270), ServerSnail = RandomInteger(3) at BeginPlay (Geo
// @266, the others @112), PlaySnail: Timeline_0 at rate 1 / (MakeLiteralFloat(2.0) x 60.0) from the start, its track
// setting the rail's CurrentPositionOnRail (Geo @3745, the others @177), in the Geo level after Delay(self, 10.0)
// (@3913); the Geo level's Tick (@331..3740): ActorStartLocation.Y += dt x BackCloudGroup<n>Speed (@717, @1804,
// @2970), set to 9000 (@1317, @2404, @3570) at CloudDistance or below (@1091, @2178, @3344).
struct Snails {
    refs: [&'static str; 3],
    rail: &'static str,
    delay: f64,
}

struct LevelRule {
    cls: &'static str,
    blackout: bool,
    snails: Option<Snails>,
    clouds: bool,
}

const LEVEL_RULES: [LevelRule; 4] = [
    LevelRule { cls: "Map_TreeHouse_C", blackout: true, snails: None, clouds: false },
    LevelRule {
        cls: "Map_TreeHouse_Geo_C",
        blackout: false,
        snails: Some(Snails {
            refs: ["Snail_SkelMesh1", "Snail_SkelMesh2", "Snail_SkelMesh3_0"],
            rail: "CameraRig_Rail1",
            delay: 10.0,
        }),
        clouds: true,
    },
    LevelRule {
        cls: "Map_TreeHouseSmall_C",
        blackout: true,
        snails: Some(Snails {
            refs: ["Snail_SkelMesh1", "Snail_SkelMesh2", "Snail_SkelMesh3"],
            rail: "CameraRig_Rail2",
            delay: 0.0,
        }),
        clouds: false,
    },
    LevelRule {
        cls: "Map_TreeHouse_1Plat_Large_C",
        blackout: true,
        snails: Some(Snails {
            refs: ["Snail_SkelMesh1", "Snail_SkelMesh2", "Snail_SkelMesh3"],
            rail: "CameraRig_Rail2",
            delay: 0.0,
        }),
        clouds: false,
    },
];
const BLACKOUT_AT: f64 = 0.5;
const SNAIL_RATE: f64 = 1.0 / (2.0 * 60.0);
const CLOUD_RESET: f64 = 9000.0;
// Lady_Rainicorn_Blueprint: Tick's RandomFloatInRange(1000, 2500) @539 (the height) and (0, 20) @774 (the wait).
const LADY_HEIGHT: [f64; 2] = [1000.0, 2500.0];
const LADY_WAIT: [f64; 2] = [0.0, 20.0];
// MoutainCloudSpin Tick: dt x 5 @10 on CloudRing_Mesh @99 and CloudRing_Mesh1 @151, dt x 10 @203 on CloudRing_Wisp_Mesh
// @292, dt x 15 @344 on Sphere @433.
const SPIN_RATES: [(&str, f64); 4] =
    [("CloudRing_Mesh", 5.0), ("CloudRing_Mesh1", 5.0), ("CloudRing_Wisp_Mesh", 10.0), ("Sphere", 15.0)];
// The pillar's montages and its bounce / damage system: TreeFortPillarDamageVFX (SpawnEmitterAtLocation(PS_Treefortbounce)
// @7075 at TreePillarCollision's location, MakeRotator(0, 0, 90), scale 1.5) and TreeFortTrampolineBounceVisual (@1485,
// scale 1). DeathVisuals @3818..4124: CreateDynamicMaterialInstance(0 and 1, DamageMaterialStates[2]), each with the
// scalar Tint 0.5 (a material without that parameter takes nothing).
const PILLAR_DIR: &str = "/Game/Panda_Main/Maps/Map_TreeHouse/NewSkeletalMeshes/";
const BOUNCE: &str = "/Game/Panda_Main/Maps/Map_TreeHouse/VFX/PS_Treefortbounce.PS_Treefortbounce";
const DEATH_TINT: f64 = 0.5;
/// UMaterial::GetDefaultMaterial(MD_Surface): a slot with no material.
const WORLD_GRID: &str = "/Engine/EngineMaterials/WorldGridMaterial.WorldGridMaterial";
// The upper platform's plank wiggle: Box1..12's begin overlap calls SetWiggle(OtherActor, N) (@2258..2577): a
// BaseFighter, and no record of WiggleInfo holds N already: a record (N, SavedLocation (0, 0, -1) when the fighter's
// actor Z >= the platform's actor Z, else (0, 0, 1), HitTime the game time). Tick (@15..2081): with records, each
// record past 0.5 s sets AnimBp.Bones[N] to zero and goes; one within it sets Bones[N] = (0, 0, 1) x WiggleCurve(age) x
// SavedLocation.Z x 2.5 (@1920). The renderer computes it from the fighters' places (presentation: nothing in the sim
// reads it). BoxExtent: the class default's (UBoxComponent's 32).
const WIGGLE_BOXES: usize = 12;
const WIGGLE_EXTENT: f64 = 32.0;
const WIGGLE_SCALE: f64 = 2.5;
const WIGGLE_LIFE: f64 = 0.5;
const PLAY_VFX: &str = "/Game/Panda_Main/Blueprints/AnimNotifies/AnimNotify_PlayVFX.AnimNotify_PlayVFX_C";

// The pose such a graph gives with its variables at their defaults, as the drawing takes it:
// - Side_Tree_AnimBp_C: Root <- ComponentToLocal <- 5 ModifyBone (Top, TopR, TopL, Bot_L_01, Bot_R_01; translation
//   BMM_Replace in BCS_BoneSpace from the Bones array, rotation and scale ignored) <- LocalToComponent <-
//   ApplyAdditive(Base: Slot DefaultSlot <- SequencePlayer Side_Tree_Idle at 0.35, looping; Additive: Slot "Additive" <-
//   SequencePlayer Side_Tree_AdditiveNull). With no montage on either slot and Bones all zero: a ModifyBone that
//   replaces a bone's translation in its own bone space with 0 leaves it where it is (4.26 FAnimNode_ModifyBone: the
//   bone's component transform relative to itself is the identity), so the pose is Side_Tree_Idle plus the additive
//   Side_Tree_AdditiveNull. Unverified: that additive is taken as no change (its name; an AAT_LocalSpaceBase
//   sequence of one frame on a frame of Side_Tree_Death_Left, not sampled here).
// - Treehouse_TopPlatformSkelMesh_Skeleton_AnimBlueprint_C: Root <- ComponentToLocal <- 12 ModifyBone (the planks, the
//   same rule) <- LocalToComponent <- SequencePlayer Treehouse_TopPlatform_Idle at 1, looping: that sequence.
// - Treehouse_TopPlatform_AnimPB_C (the night variant's): the same 12 ModifyBone over an unlinked component pose
//   (LinkID -1: the reference pose, as an unlinked FPoseLink): the reference pose.
// idle_of gives the sequence and its rate, or `None` for the reference pose; a graph of other nodes fails.
struct Idle {
    name: &'static str,
    nodes: &'static str,
    mods: usize,
    player: Option<&'static str>,
    additive: Option<&'static str>,
}

const IDLE: [Idle; 3] = [
    Idle {
        name: "Side_Tree_AnimBp_C",
        nodes: "UberGraphFrame,AnimGraphNode_ComponentToLocalSpace,AnimGraphNode_Root,AnimGraphNode_SequencePlayer_1,AnimGraphNode_Slot_1,AnimGraphNode_ApplyAdditive,AnimGraphNode_Slot,AnimGraphNode_LocalToComponentSpace,AnimGraphNode_SequencePlayer",
        mods: 5,
        player: Some("AnimGraphNode_SequencePlayer"),
        additive: Some("Side_Tree_AdditiveNull"),
    },
    Idle {
        name: "Treehouse_TopPlatformSkelMesh_Skeleton_AnimBlueprint_C",
        nodes: "UberGraphFrame,AnimGraphNode_Root,AnimGraphNode_ComponentToLocalSpace,AnimGraphNode_LocalToComponentSpace,AnimGraphNode_SequencePlayer",
        mods: 12,
        player: Some("AnimGraphNode_SequencePlayer"),
        additive: None,
    },
    Idle {
        name: "Treehouse_TopPlatform_AnimPB_C",
        nodes: "UberGraphFrame,AnimGraphNode_Root,AnimGraphNode_ComponentToLocalSpace",
        mods: 12,
        player: None,
        additive: None,
    },
];

fn is_mod(k: &str) -> bool {
    k.starts_with("AnimGraphNode_ModifyBone")
}

/// An AnimBlueprint's graph nodes (its CDO's own AnimGraphNode_ properties), by its class path.
fn anim_graph<'a>(ctx: &dyn SceneryBindCtx<'a>, cls: &str) -> Result<JsonProps, KindError> {
    let dot = cls.rfind('.').unwrap_or(0);
    Ok(props_of(&ctx.load(&format!("{}.Default__{}", &cls[..dot], &cls[dot + 1..]))?))
}

fn idle_of<'a>(ctx: &dyn SceneryBindCtx<'a>, cls: &str) -> Result<Option<SkeletalPlay>, KindError> {
    let name = short_name(cls);
    let Some(rule) = IDLE.iter().find(|r| r.name == name) else {
        return fail(PART, format!("AnimBP {name} not read"));
    };
    let g = anim_graph(ctx, cls)?;
    let keys: Vec<&str> = g.keys().map(String::as_str).filter(|k| !is_mod(k) && *k != "Bones").collect();
    let mods: Vec<&Json> = g.iter().filter(|(k, _)| is_mod(k)).map(|(_, v)| v).collect();
    if keys.join(",") != rule.nodes
        || mods.len() != rule.mods
        || arr(g.get("Bones")).iter().any(|v| truthy(v.get("X")) || truthy(v.get("Y")) || truthy(v.get("Z")))
    {
        return fail(PART, format!("{name} changed"));
    }
    for m in &mods {
        let m = Some(*m);
        if !is_str(at(m, "TranslationMode"), "BMM_Replace")
            || !is_str(at(m, "TranslationSpace"), "BCS_BoneSpace")
            || !is_str(at(m, "RotationMode"), "BMM_Ignore")
            || !is_str(at(m, "ScaleMode"), "BMM_Ignore")
        {
            return fail(PART, format!("{name}: ModifyBone"));
        }
    }
    if let Some(additive) = rule.additive {
        let seq = at(g.get("AnimGraphNode_SequencePlayer_1"), "Sequence");
        if short_ref(seq)? != additive {
            return fail(PART, format!("{name}: additive"));
        }
    }
    let Some(player) = rule.player else {
        if !mods.iter().any(|m| number(at(at(Some(m), "ComponentPose"), "LinkID")) == -1.0) {
            return fail(PART, format!("{name}: its source pose"));
        }
        return Ok(None);
    };
    let s = g.get(player);
    let c = at(s, "PlayRateScaleBiasClamp");
    if !truthy(at(s, "bLoopAnimation"))
        || truthy(at(s, "StartPosition"))
        || num_or(at(s, "PlayRateBasis"), 1.0) != 1.0
        || truthy(at(c, "bMapRange"))
        || truthy(at(c, "bClampResult"))
        || truthy(at(c, "bInterpResult"))
        || number(at(c, "Scale")) != 1.0
        || truthy(at(c, "Bias"))
    {
        return fail(PART, format!("{name}: sequence player"));
    }
    Ok(Some(SkeletalPlay {
        anim: reference(at(s, "Sequence"), &format!("{name} sequence"))?,
        rate: number(at(s, "PlayRate")),
    }))
}

/// What such a graph's ModifyBone nodes read: its property access copies (PropertyAccessLibrary: source path k to
/// destination path k, a Bones[i] element to a node's Translation), as { slot: i, bone } in the copies' order. The
/// owning actor writes Bones (the pillar's wiggle pass, the upper platform's Tick); every other node input is the class
/// default checked by `idle_of`.
fn modify_of<'a>(ctx: &dyn SceneryBindCtx<'a>, cls: &str) -> Result<JsonProps, KindError> {
    let name = short_name(cls);
    let lib_obj = props_of(&ctx.load(cls)?);
    let l = lib_obj.get("PropertyAccessLibrary");
    let g = anim_graph(ctx, cls)?;
    let segs = req_arr(PART, at(l, "PathSegments"), "PathSegments")?;
    let seg = |p: Option<&Json>| -> Vec<&Json> {
        let start = js::index(at(p, "PathSegmentStartIndex"));
        let count = js::index(at(p, "PathSegmentCount"));
        segs.iter().skip(start).take(count).collect()
    };
    let batches: Vec<Option<&Json>> = match at(l, "CopyBatches") {
        Some(Json::Arr(a)) => a.iter().map(Some).collect(),
        other => vec![other],
    };
    let mut out = Vec::new();
    for b in batches {
        for c in arr(at(b, "Copies")) {
            let src = arr(at(l, "SrcPaths")).get(js::index(c.get("AccessIndex")));
            let s = seg(src);
            if number(c.get("DestAccessEndIndex")) - number(c.get("DestAccessStartIndex")) != 1.0 {
                return fail(PART, format!("{name}: a copy to several"));
            }
            let d = seg(arr(at(l, "DestPaths")).get(js::index(c.get("DestAccessStartIndex"))));
            if s.len() != 1
                || !is_str(s[0].get("Name"), "Bones")
                || js::index(s[0].get("ArrayIndex")) == usize::MAX
                || d.len() != 2
                || !d[0].get("Name").and_then(Json::as_str).is_some_and(is_mod)
                || !is_str(d[1].get("Name"), "Translation")
            {
                let names = |v: &[&Json]| {
                    v.iter().map(|x| x.get("Name").and_then(Json::as_str).unwrap_or("")).collect::<Vec<_>>().join(".")
                };
                return fail(PART, format!("{name}: property copy {} -> {}", names(&s), names(&d)));
            }
            let node = d[0].get("Name").and_then(Json::as_str).unwrap_or("");
            let mut m = JsonProps::new();
            put_opt(&mut m, "slot", s[0].get("ArrayIndex"));
            put_opt(&mut m, "bone", at(at(g.get(node), "BoneToModify"), "BoneName"));
            out.push(Json::Obj(m));
        }
    }
    let want = IDLE.iter().find(|r| r.name == name).map(|r| r.mods);
    if want != Some(out.len()) {
        return fail(PART, format!("{name}: {} copies", out.len()));
    }
    let mut r = JsonProps::new();
    put(&mut r, "slots", arr(g.get("Bones")).len() as f64);
    put(&mut r, "modify", Json::Arr(out));
    Ok(r)
}

/// A montage as kept for the renderer (the pillar's hit and death): its length and blends, the slot track's segments
/// (animation by build object path, montage start, animation start / end, rate) and the PlayVFX notifies (time, end,
/// system, socket).
fn montage_of<'a>(ctx: &dyn SceneryBindCtx<'a>, path: &str) -> Result<Json, KindError> {
    let name = short_name(path);
    let m = props_of(&ctx.load(path)?);
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
    let tracks = arr(m.get("SlotAnimTracks"));
    let sections = arr(m.get("CompositeSections"));
    if !other.is_empty()
        || tracks.len() != 1
        || sections.len() != 1
        || !is_str(sections[0].get("NextSectionName"), "None")
    {
        return fail(PART, format!("{name}: montage {}", other.join(",")));
    }
    let blend = |b: Option<&Json>| -> Result<Json, KindError> {
        match b {
            Some(Json::Obj(o)) if o.len() == 1 && o.contains_key("BlendTime") => Ok(o["BlendTime"].clone()),
            _ => fail(PART, format!("{name}: blend")),
        }
    };
    let cdo = cdo_props(ctx, PLAY_VFX)?;
    let mut notifies = Vec::new();
    for n in arr(m.get("Notifies")) {
        if !is_str(n.get("NotifyName"), "PlayVFX_C")
            || !is_str(n.get("MontageTickType"), "EMontageNotifyTickType::Queued")
        {
            return fail(
                PART,
                format!("{name}: notify {}", n.get("NotifyName").and_then(Json::as_str).unwrap_or("undefined")),
            );
        }
        let notify = reference(first(n.get("Notify"), n.get("NotifyStateClass")), "a notify")?;
        let q = over_props(&cdo, Some(&props_of(&ctx.load(&notify)?)));
        let (l, r, s) = (q.get("Location"), q.get("Rotation"), q.get("VFX Scale"));
        // (UseTeamColorParam?, the class default, sets the Team parameter only on an owner with BPI_BaseFighter,
        // @3275..3559)
        if truthy(q.get("UseSkinVFX"))
            || truthy(q.get("UseOverrideAttachParent"))
            || q.get("ParticleTeam0") != q.get("ParticleTeam1")
            || ["X", "Y", "Z"].iter().any(|k| truthy(at(l, k)))
            || ["Pitch", "Yaw", "Roll"].iter().any(|k| truthy(at(r, k)))
            || ["X", "Y", "Z"].iter().any(|k| number(at(s, k)) != 1.0)
        {
            return fail(PART, format!("{name}: PlayVFX"));
        }
        let lv = number(n.get("LinkValue"));
        let mut o = JsonProps::new();
        put(&mut o, "at", lv + or_num(n.get("TriggerTimeOffset"), 0.0));
        if truthy(n.get("Duration")) {
            put(&mut o, "end", lv + number(n.get("Duration")));
        }
        put(&mut o, "system", ctx.add_system(&reference(q.get("ParticleTeam0"), "ParticleTeam0")?)?);
        put_opt(&mut o, "socket", q.get("SocketName"));
        put(&mut o, "kill", truthy(q.get("KillOnNotifyEnd")));
        notifies.push(Json::Obj(o));
    }
    let t = &tracks[0];
    // (an additive sequence: AAT_LocalSpaceBase over frame 0 of an ABPT_AnimFrame base, its cooked delta)
    let mut segments = Vec::new();
    for s in req_arr(PART, at(t.get("AnimTrack"), "AnimSegments"), "AnimSegments")? {
        let anim = reference(s.get("AnimReference"), "a segment")?;
        if num_or(s.get("LoopingCount"), 1.0) != 1.0 {
            return fail(PART, format!("{anim}: segment"));
        }
        let a = props_of(&ctx.load(&anim)?);
        let kind = a.get("AdditiveAnimType");
        if !is_str(first(kind, None), "AAT_None") && kind.is_some_and(|k| !matches!(k, Json::Null)) {
            let ok = is_str(kind, "AAT_LocalSpaceBase")
                && is_str(a.get("RefPoseType"), "ABPT_AnimFrame")
                && num_or(a.get("RefFrameIndex"), 0.0) == 0.0;
            if !ok {
                return fail(PART, format!("{anim}: additive base"));
            }
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

/// A Blueprint component's world transform as the core's relative properties (placed: the part wrote it).
fn placed<'a>(
    ctx: &dyn SceneryBindCtx<'a>,
    owner: &ObjView<'a>,
    e: &ObjView<'a>,
    p: &JsonProps,
) -> Result<JsonProps, KindError> {
    let props = props_of_actor(ctx, owner)?;
    placed_with(ctx, &props, e, p)
}

fn placed_with<'a>(
    ctx: &dyn SceneryBindCtx<'a>,
    props: &ActorProps,
    e: &ObjView<'a>,
    p: &JsonProps,
) -> Result<JsonProps, KindError> {
    let own = |c: &ObjView<'_>| -> Result<JsonProps, KindError> {
        if c.path() == e.path() { Ok(p.clone()) } else { props.get(c).cloned() }
    };
    let w = world_of(&ctx.level().objects, e, &own)?;
    let mut out = p.clone();
    for (k, v) in as_props(&w) {
        out.insert(k, v);
    }
    out.shift_remove("AttachParent");
    Ok(out)
}

/// A placed actor's component by name.
fn comp_named<'v, 'a>(ctx: &'v dyn SceneryBindCtx<'a>, owner: &ObjView<'_>, name: &str) -> Option<&'v ObjView<'a>> {
    let path = format!("{}.{name}", owner.path());
    ctx.level().objects.iter().find(|x| x.path() == path)
}

/// [actor, component] of a reference to a placed actor's component.
fn component_ref(v: Option<&Json>) -> Result<(String, String), KindError> {
    let s = v.and_then(Json::as_str).unwrap_or("");
    let word = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    for (i, _) in s.match_indices("PersistentLevel.") {
        let rest = &s[i + "PersistentLevel.".len()..];
        if let Some((a, b)) = rest.split_once('.')
            && word(a)
            && word(b)
        {
            return Ok((a.to_owned(), b.to_owned()));
        }
    }
    let shown = v.map_or_else(
        || "undefined".to_owned(),
        |v| String::from_utf8(crate::json::to_vec(v).unwrap_or_default()).unwrap_or_default(),
    );
    fail(PART, format!("not a placed component: {shown}"))
}

fn triple_of(v: Option<&Json>) -> [f64; 3] {
    vec(v, [0.0; 3])
}

fn upper_wiggle<'a>(
    ctx: &dyn SceneryBindCtx<'a>,
    owner: &ObjView<'a>,
    v: &JsonProps,
    bp: &str,
) -> Result<JsonProps, KindError> {
    let props = props_of_actor(ctx, owner)?;
    let mut boxes = Vec::new();
    for n in 1..=WIGGLE_BOXES {
        let Some(e) = comp_named(ctx, owner, &format!("Box{n}")) else {
            return fail(PART, format!("{}: no Box{n}", owner.path()));
        };
        let p = props.get(e)?;
        // (a BodyInstance of MaxAngularVelocity or bInterpolateWhenSubStepping alone: no collision profile set)
        let body_set = match p.get("BodyInstance") {
            Some(Json::Obj(o)) => o.keys().any(|k| k != "MaxAngularVelocity" && k != "bInterpolateWhenSubStepping"),
            _ => false,
        };
        if truthy(p.get("BoxExtent")) || body_set {
            return fail(PART, format!("{}.Box{n}: its extent or profile set", owner.path()));
        }
        let w = placed_with(ctx, &props, e, p)?;
        let mut b = JsonProps::new();
        put(&mut b, "n", n as f64);
        put(&mut b, "loc", triple(vec(w.get("RelativeLocation"), [0.0; 3])));
        put(&mut b, "rot", triple(rot_of(w.get("RelativeRotation"))));
        put(&mut b, "scale", triple(vec(w.get("RelativeScale3D"), [0.0; 3])));
        boxes.push(Json::Obj(b));
    }
    let wiggle_curve = reference(v.get("WiggleCurve"), "WiggleCurve")?;
    let curve_obj = props_of(&ctx.load(&wiggle_curve)?);
    let curve = curve_obj.get("FloatCurve");
    // (K2_GetActorLocation: the actor's RootComponent's world location)
    let owner_props = props_of(owner);
    let root = match owner_props.get("RootComponent") {
        Some(Json::Str(r)) => comp_named(ctx, owner, short_name(r)),
        _ => None,
    };
    let Some(root) = root else { return fail(PART, format!("{}: its root component", owner.path())) };
    let z = number(at(placed_with(ctx, &props, root, props.get(root)?)?.get("RelativeLocation"), "Z"));
    let mut w = JsonProps::new();
    put(&mut w, "boxes", Json::Arr(boxes));
    put(&mut w, "extent", WIGGLE_EXTENT);
    put(&mut w, "z", z);
    put(&mut w, "curve", Json::Arr(curve_keys(curve)?.iter().map(js::key_json).collect()));
    put(&mut w, "scale", WIGGLE_SCALE);
    put(&mut w, "life", WIGGLE_LIFE);
    for (k, x) in modify_of(ctx, bp)? {
        w.insert(k, x);
    }
    let mut out = JsonProps::new();
    put(&mut out, "wiggle", Json::Obj(w));
    Ok(out)
}

/// The level Blueprint's own, as it answers for the actors of its level afterwards.
#[derive(Default)]
struct LevelHooks {
    blackout: Option<String>,
    snails: Option<Vec<String>>,
    clouds: Option<Vec<Vec<String>>>,
}

/// The level Blueprint's own: the BlackoutSphere it destroys, its snails and the Geo level's clouds.
fn level_of<'a>(ctx: &dyn SceneryBindCtx<'a>, cls: &str, level: &mut JsonProps) -> Result<LevelHooks, KindError> {
    let Some(rule) = LEVEL_RULES.iter().find(|r| r.cls == cls) else {
        return fail(PART, format!("level {cls}"));
    };
    let lv = ctx.level();
    let cls_path = format!("{MAPS_ROOT}{}.{cls}", lv.level);
    let actors = || lv.actors.iter().map(|&i| &lv.objects[i]);
    let Some(script) = actors().find(|a| a.class() == cls_path) else {
        return fail(PART, format!("{}: no {cls} actor", lv.level));
    };
    let vars = over_props(&cdo_props(ctx, &cls_path)?, Some(&props_of(script)));
    let name = cls.strip_suffix("_C").unwrap_or(cls);
    let mut out = LevelHooks::default();
    let actor_ref = |k: &str| -> Result<String, KindError> {
        match vars.get(k) {
            Some(Json::Str(v)) => Ok(short_name(v).to_owned()),
            _ => fail(PART, format!("{cls}: {k}")),
        }
    };
    if rule.blackout {
        if truthy(level.get("blackout")) {
            return fail(PART, "two BlackoutSphere scripts".into());
        }
        let actor = actor_ref(&format!("BlackoutSphere_ExecuteUbergraph_{name}_RefProperty"))?;
        let mut b = JsonProps::new();
        put(&mut b, "actor", actor.as_str());
        put(&mut b, "at", BLACKOUT_AT);
        level.insert("blackout".into(), Json::Obj(b));
        out.blackout = Some(actor);
    }
    if let Some(sn) = &rule.snails {
        if truthy(level.get("snails")) {
            return fail(PART, format!("{cls}: snails"));
        }
        let tl = props_of(&ctx.load(&format!("{cls_path}:Timeline_0_Template"))?);
        let tracks = arr(tl.get("FloatTracks"));
        if !is_str(tl.get("LengthMode"), "TL_LastKeyFrame")
            || tracks.len() != 1
            || truthy(tl.get("bLoop"))
            || !arr(tl.get("EventTracks")).is_empty()
            || !arr(tl.get("VectorTracks")).is_empty()
        {
            return fail(PART, format!("{cls}: timeline"));
        }
        let track = props_of(&ctx.load(&reference(tracks[0].get("CurveFloat"), "the snail track")?)?);
        let keys = curve_keys(track.get("FloatCurve"))?;
        // the rail (CameraRig_Rail): its spline's curves in the component's space, the root's transform
        let rail_actor = sn.rail;
        let rig = actors().find(|a| name_of(a) == rail_actor && a.class().ends_with(".CameraRig_Rail"));
        let rail_comp = rig.and_then(|r| comp_named(ctx, r, "RailSplineComponent"));
        let (Some(rig), Some(rail_comp)) = (rig, rail_comp) else {
            return fail(PART, format!("{cls}: no rail {rail_actor}"));
        };
        let spline = props_of(rail_comp);
        let rig_props = props_of(rig);
        let pts = |c: Option<&Json>, f: &dyn Fn(Option<&Json>) -> Vec<f64>| -> Result<Vec<Json>, KindError> {
            let mut out = Vec::new();
            for p in arr(at(c, "Points")) {
                let mut row = vec![p.get("InVal").cloned().unwrap_or(Json::Null)];
                for k in ["OutVal", "ArriveTangent", "LeaveTangent"] {
                    row.extend(f(p.get(k)).into_iter().map(Json::Num));
                }
                let mode = p.get("InterpMode").and_then(Json::as_str);
                let Some(mode) = mode else { return fail(PART, format!("{cls}: rail interp mode")) };
                row.push(Json::Str(mode.replacen("CIM_", "", 1)));
                out.push(Json::Arr(row));
            }
            Ok(out)
        };
        let t3 = |v: Option<&Json>| triple_of(v).to_vec();
        let q4 = |v: Option<&Json>| -> Vec<f64> {
            let s = if is_struct(v) { v } else { None };
            vec![num_or(at(s, "X"), 0.0), num_or(at(s, "Y"), 0.0), num_or(at(s, "Z"), 0.0), num_or(at(s, "W"), 1.0)]
        };
        let sc = spline.get("SplineCurves");
        let place_in = |n: &str| -> Result<Json, KindError> {
            let Some(c) = comp_named(ctx, rig, n) else { return fail(PART, format!("{rail_actor}: no {n}")) };
            let p = props_of(c);
            let mut o = JsonProps::new();
            put(&mut o, "loc", triple(triple_of(p.get("RelativeLocation"))));
            put(&mut o, "rot", triple(rot_of(p.get("RelativeRotation"))));
            Ok(Json::Obj(o))
        };
        // (the spline under the rig's root with no relative transform of its own, its Rotation keys all the identity
        // and no up vector of its own: the mount's rotation is the spline direction's MakeFromXZ with world Z;
        // ReparamTable: the cooked distance -> input key table, CIM_Linear)
        let rot_pts = arr(at(at(sc, "Rotation"), "Points"));
        let reparam = arr(at(at(sc, "ReparamTable"), "Points"));
        let attach_ok =
            matches!(spline.get("AttachParent"), Some(Json::Str(a)) if short_name(a) == "TransformComponent");
        if !attach_ok
            || truthy(spline.get("RelativeLocation"))
            || truthy(spline.get("RelativeRotation"))
            || truthy(spline.get("RelativeScale3D"))
            || truthy(spline.get("DefaultUpVector"))
            || rot_pts.iter().any(|p| {
                let o = p.get("OutVal");
                ["X", "Y", "Z"].iter().any(|k| truthy(at(o, k))) || number(at(o, "W")) != 1.0
            })
            || reparam.iter().any(|p| !is_str(p.get("InterpMode"), "CIM_Linear"))
        {
            return fail(PART, format!("{cls}: rail spline"));
        }
        let mut actor_names = Vec::new();
        for r in sn.refs {
            actor_names.push(actor_ref(&format!("{r}_EdGraph_0_RefProperty"))?);
        }
        // OnRep_ServerSnail: only the third snail's case calls PlaySnail (@233..270; the other two return at @186 /
        // @228), and that snail alone is attached to the rail's RailCameraMount: its relative transform
        let mut mounts = Vec::new();
        for a in &actor_names {
            let c = lv.objects.iter().find(|o| name_of(o) == "SkeletalMeshComponent0" && outer_name(o) == *a);
            let Some(c) = c else { return fail(PART, format!("{cls}: snail {a}")) };
            mounts.push(props_of(c));
        }
        let on_mount = |p: &JsonProps| -> Result<bool, KindError> {
            if !truthy(p.get("AttachParent")) {
                return Ok(false);
            }
            let (a, c) = component_ref(p.get("AttachParent"))?;
            Ok(format!("{a}.{c}") == format!("{rail_actor}.RailCameraMount"))
        };
        for (i, p) in mounts.iter().enumerate() {
            if (i == 2) != on_mount(p)? {
                return fail(PART, format!("{cls}: snail mount"));
            }
        }
        let m = &mounts[2];
        let mut mounted = JsonProps::new();
        put(&mut mounted, "snail", 2.0);
        put(&mut mounted, "loc", triple(triple_of(m.get("RelativeLocation"))));
        put(&mut mounted, "rot", triple(rot_of(m.get("RelativeRotation"))));
        put(&mut mounted, "scale", triple(vec(m.get("RelativeScale3D"), [1.0; 3])));
        let mut spline_out = JsonProps::new();
        put(&mut spline_out, "position", Json::Arr(pts(at(sc, "Position"), &t3)?));
        put(&mut spline_out, "rotation", Json::Arr(pts(at(sc, "Rotation"), &q4)?));
        put(&mut spline_out, "scale", Json::Arr(pts(at(sc, "Scale"), &t3)?));
        let mut rail = JsonProps::new();
        put(&mut rail, "actor", rail_actor);
        put(&mut rail, "position", nn(rig_props.get("CurrentPositionOnRail"), 0.0));
        put(&mut rail, "lockOrientation", truthy(rig_props.get("bLockOrientationToRail")));
        put(&mut rail, "root", place_in("TransformComponent")?);
        put(&mut rail, "mount", place_in("RailCameraMount")?);
        put(&mut rail, "spline", Json::Obj(spline_out));
        put(
            &mut rail,
            "reparam",
            Json::Arr(
                reparam
                    .iter()
                    .map(|p| {
                        Json::Arr(vec![
                            p.get("InVal").cloned().unwrap_or(Json::Null),
                            p.get("OutVal").cloned().unwrap_or(Json::Null),
                        ])
                    })
                    .collect(),
            ),
        );
        let mut snails = JsonProps::new();
        put(&mut snails, "actors", Json::Arr(actor_names.iter().map(|a| Json::Str(a.clone())).collect()));
        put(&mut snails, "mounted", Json::Obj(mounted));
        put(&mut snails, "delay", sn.delay);
        put(&mut snails, "rate", SNAIL_RATE);
        put(&mut snails, "curve", Json::Arr(keys.iter().map(js::key_json).collect()));
        put(&mut snails, "rail", Json::Obj(rail));
        level.insert("snails".into(), Json::Obj(snails));
        out.snails = Some(actor_names);
    }
    if rule.clouds {
        let mut groups = Vec::new();
        let mut names = Vec::new();
        for n in 1..=3 {
            let acts: Vec<String> = match vars.get(&format!("BackCloudGroup{n}")) {
                Some(Json::Arr(a)) => a.iter().filter_map(Json::as_str).map(|r| short_name(r).to_owned()).collect(),
                _ => return fail(PART, format!("{cls}: BackCloudGroup{n}")),
            };
            let mut g = JsonProps::new();
            put(&mut g, "actors", Json::Arr(acts.iter().map(|a| Json::Str(a.clone())).collect()));
            put_opt(&mut g, "speed", vars.get(&format!("BackCloudGroup{n}Speed")));
            groups.push(Json::Obj(g));
            names.push(acts);
        }
        let mut c = JsonProps::new();
        put_opt(&mut c, "distance", vars.get("CloudDistance"));
        put(&mut c, "reset", CLOUD_RESET);
        put(&mut c, "groups", Json::Arr(groups));
        level.insert("clouds".into(), Json::Obj(c));
        out.clouds = Some(names);
    }
    Ok(out)
}

pub struct TreeFort;

impl SceneryPart for TreeFort {
    fn name(&self) -> &'static str {
        "treehouse"
    }
    fn blueprints(&self) -> &'static [&'static str] {
        &[
            "TreehouseSidePillar_C",
            "TreehouseSidePillar_Child_C",
            "TreehouseUpperPlatform_C",
            "TreehouseUpperPlatform_1v1_C",
            "TreehouseSmallPlatform_C",
            "TreehouseSmallPlatform_Child_C",
            "M002_TreehouseSmallPlatform_Night_01_C",
            "Lady_Rainicorn_Blueprint_C",
            "MoutainCloudSpin_C",
            "TreeFortTrampoline_C",
        ]
    }
    fn levels(&self) -> &'static [&'static str] {
        &["Map_TreeHouse_C", "Map_TreeHouse_Geo_C", "Map_TreeHouseSmall_C", "Map_TreeHouse_1Plat_Large_C"]
    }
    fn run<'a>(&self) -> Box<dyn SceneryPartRun<'a> + 'a> {
        Box::new(TreeFortRun::default())
    }
}

/// A cloud spin's record: its fields, then the `comps` the loop adds.
struct SpinRec {
    actor: String,
    fields: JsonProps,
    comps: Option<JsonProps>,
}

/// One bind's state.
#[derive(Default)]
struct TreeFortRun {
    /// actor -> record
    pillars: indexmap::IndexMap<String, JsonProps>,
    platforms: Vec<Json>,
    ladies: Vec<Json>,
    spins: Vec<SpinRec>,
    /// `blackout`, `snails`, `clouds` as the level Blueprints set them.
    level: JsonProps,
    /// What the last level Blueprint answers for its actors.
    hooks: LevelHooks,
}

impl TreeFortRun {
    fn pillar_of<'a>(
        &mut self,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
    ) -> Result<&mut JsonProps, KindError> {
        let actor = name_of(owner).to_owned();
        if !self.pillars.contains_key(&actor) {
            let mut rec = JsonProps::new();
            put(&mut rec, "actor", actor.as_str());
            put(&mut rec, "isRight", truthy(vars_of(ctx, owner)?.get("IsRight")));
            put(&mut rec, "mesh", format!("{actor}.SkeletalMesh"));
            self.pillars.insert(actor.clone(), rec);
        }
        Ok(&mut self.pillars[&actor])
    }
}

impl<'a> SceneryPartRun<'a> for TreeFortRun {
    fn level(&mut self, cls: &str, ctx: &dyn SceneryBindCtx<'a>) -> Result<SceneryLevelRun, KindError> {
        self.hooks = level_of(ctx, cls, &mut self.level)?;
        Ok(SceneryLevelRun { tags: Vec::new() })
    }

    fn instance(&mut self, actor: InstanceActor<'_, 'a>) -> Result<JsonProps, KindError> {
        let mut out = JsonProps::new();
        let name = actor.name();
        if let Some(groups) = &self.hooks.clouds {
            if let Some(i) = groups.iter().position(|g| g.iter().any(|a| a == name)) {
                put(&mut out, "cloudGroup", i as f64);
            }
        } else if self.hooks.blackout.as_deref() == Some(name) {
            put(&mut out, "destroyAt", BLACKOUT_AT);
        }
        Ok(out)
    }

    fn skeletal_shows(&mut self, actor: &ObjView<'a>) -> Result<Option<JsonProps>, KindError> {
        let Some(actors) = &self.hooks.snails else { return Ok(None) };
        Ok(actors.iter().position(|a| a == name_of(actor)).map(|i| {
            let mut o = JsonProps::new();
            put(&mut o, "snail", i as f64);
            o
        }))
    }

    fn skeletal_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        e: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        let props = props_of_actor(ctx, owner)?;
        let q = props.get(e)?.clone();
        let name = format!("{}.{}", name_of(owner), name_of(e));
        if PILLAR.contains(&cls) {
            let v = vars_of(ctx, owner)?;
            let is_right = truthy(self.pillar_of(ctx, owner)?.get("isRight"));
            let mesh = format!("{}.SkeletalMesh", name_of(owner));
            if name_of(e) == "SkeletalMesh" {
                let bp = reference(q.get("AnimClass"), "AnimClass")?;
                let play = idle_of(ctx, &bp)?;
                let place = placed_with(ctx, &props, e, &q)?;
                ctx.add_skeletal(
                    &name,
                    &q,
                    &place,
                    &SkeletalOpts { anim_bp: Some(short_name(&bp).to_owned()), play, ..Default::default() },
                )?;
                // the wiggles: Bones (the sim's TreePillar.bones) into the ModifyBone nodes
                let bones = modify_of(ctx, &bp)?;
                // the damage stages: SetSkeletalMesh(DamageMeshStates[stage]) and SetMaterial(1,
                // DamageMaterialStates[stage]) with the component's OverrideMaterials on the other slots (the mesh's own
                // past them)
                let over = arr(q.get("OverrideMaterials"));
                let dms = arr(v.get("DamageMaterialStates"));
                let mut damage = Vec::new();
                for (k, m) in req_arr(PART, v.get("DamageMeshStates"), "DamageMeshStates")?.iter().enumerate() {
                    let m = m.as_str().unwrap_or("");
                    let h = ctx.mesh_header(m)?;
                    if h.lods[0].sections.iter().enumerate().any(|(i, x)| x.slot != i as f64) {
                        return fail(PART, format!("{m}: section materials"));
                    }
                    // (R_Tree_Damage_01 / _02's third slot, ST_TopRightPlatform_01_M (the flag's pole), names no
                    // material and the component overrides two slots: drawn with the engine's default surface
                    // material, WorldGridMaterial.
                    // Unverified: that SetSkeletalMesh keeps OverrideMaterials and a slot with neither renders with
                    // GetDefaultMaterial(MD_Surface), as the fighter draws take it; not read in the exe.)
                    let mut mats = Vec::new();
                    for (i, x) in h.slots.iter().enumerate() {
                        let pick =
                            |j: Option<&Json>| j.and_then(Json::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
                        let mm = if i == 1 {
                            pick(dms.get(k))
                        } else {
                            pick(over.get(i)).or_else(|| x.material.clone().filter(|s| !s.is_empty()))
                        };
                        let sk = ctx.skin_material(mm.as_deref().unwrap_or(WORLD_GRID), &name)?;
                        mats.push(sk.map_or(Json::Null, Json::Str));
                    }
                    let mut d = JsonProps::new();
                    put(&mut d, "mesh", m);
                    put(&mut d, "materials", Json::Arr(mats));
                    damage.push(Json::Obj(d));
                }
                let hit = montage_of(ctx, &format!("{PILLAR_DIR}SideTree_Hit_AnimMontage.SideTree_Hit_AnimMontage"))?;
                let death_name =
                    if is_right { "Side_Tree_Death_Right_Montage" } else { "Side_Tree_Death_Left_Montage" };
                let death = montage_of(ctx, &format!("{PILLAR_DIR}{death_name}.{death_name}"))?;
                let dm = reference(dms.get(2), "DamageMaterialStates[2]")?;
                let mut dmat = JsonProps::new();
                put(&mut dmat, "slots", Json::Arr(vec![Json::Num(0.0), Json::Num(1.0)]));
                put(&mut dmat, "material", ctx.skin_material(&dm, &name)?.map_or(Json::Null, Json::Str));
                let mut scalars = JsonProps::new();
                put(&mut scalars, "Tint", DEATH_TINT);
                put(&mut dmat, "scalars", Json::Obj(scalars));
                let bounce = ctx.add_system(BOUNCE)?;
                let rec = self.pillar_of(ctx, owner)?;
                put(rec, "bones", Json::Obj(bones));
                put(rec, "damage", Json::Arr(damage));
                put(rec, "hit", hit);
                put(rec, "death", death);
                put(rec, "deathMaterial", Json::Obj(dmat));
                put(rec, "bounce", bounce);
            } else if ["Ball", "Bucket", "Flag"].contains(&name_of(e)) {
                // on a bone of SkeletalMesh (no sockets on its skeleton: the names are bones), at its relative transform
                let socket = q.get("AttachSocketName").and_then(Json::as_str).unwrap_or("");
                let parent_ok = matches!(q.get("AttachParent"), Some(Json::Str(a)) if short_name(a) == "SkeletalMesh");
                if !parent_ok || !["Trunk", "Root"].contains(&socket) {
                    return fail(PART, format!("{name}: attach"));
                }
                let hidden =
                    if is_right { name_of(e) == "Bucket" } else { name_of(e) == "Ball" || name_of(e) == "Flag" };
                ctx.add_skeletal(
                    &name,
                    &q,
                    &q,
                    &SkeletalOpts {
                        attach: Some(Json::Arr(vec![Json::Str(mesh), Json::Str(socket.to_owned())])),
                        hidden,
                        ..Default::default()
                    },
                )?;
                put(self.pillar_of(ctx, owner)?, &name_of(e).to_lowercase(), name.as_str());
            } else {
                return Ok(false);
            }
            return Ok(true);
        }
        if UPPER.contains(&cls) || SMALL.contains(&cls) {
            let upper = UPPER.contains(&cls);
            if name_of(e) != if upper { "SkeletalMesh" } else { "TreeFortSmallPlatformSkelMesh" } {
                return Ok(false);
            }
            let v = vars_of(ctx, owner)?;
            let place = placed_with(ctx, &props, e, &q)?;
            let mut rec = JsonProps::new();
            put(&mut rec, "actor", name_of(owner));
            if upper {
                let bp = reference(q.get("AnimClass"), "AnimClass")?;
                let play = idle_of(ctx, &bp)?;
                ctx.add_skeletal(
                    &name,
                    &q,
                    &place,
                    &SkeletalOpts { anim_bp: Some(short_name(&bp).to_owned()), play, ..Default::default() },
                )?;
                put(&mut rec, "kind", "upper");
                put(&mut rec, "skeletal", name.as_str());
                for (k, x) in upper_wiggle(ctx, owner, &v, &bp)? {
                    rec.insert(k, x);
                }
            } else {
                let anim = ctx.add_skeletal(&name, &q, &place, &SkeletalOpts::default())?;
                put(&mut rec, "kind", "small");
                put(&mut rec, "skeletal", name.as_str());
                let playing = match anim {
                    Some(a) => Json::Str(ctx.anim_path(&a, false)?),
                    None => Json::Null,
                };
                put(&mut rec, "playing", playing);
                let hit = reference(v.get("TreeFortSmallPlatformHitAnim"), "hit anim")?;
                put(&mut rec, "hit", ctx.anim_path(&hit, false)?);
                let idle = reference(v.get("TreeFortSmallPlatformIdleAnim"), "idle anim")?;
                put(&mut rec, "idle", ctx.anim_path(&idle, false)?);
            }
            self.platforms.push(Json::Obj(rec));
            return Ok(true);
        }
        if cls == LADY {
            if name_of(e) != "SkeletalMeshComponent0" {
                return Ok(false);
            }
            let v = vars_of(ctx, owner)?;
            ctx.add_skeletal(&name, &q, &q, &SkeletalOpts::default())?;
            let mut rec = JsonProps::new();
            put(&mut rec, "actor", name_of(owner));
            put(&mut rec, "skeletal", name.as_str());
            put_opt(&mut rec, "speed", v.get("RainicornSpeed"));
            put_opt(&mut rec, "distance", v.get("RainicornDistance"));
            put(&mut rec, "height", triple2(LADY_HEIGHT));
            put(&mut rec, "wait", triple2(LADY_WAIT));
            self.ladies.push(Json::Obj(rec));
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
        let mut extra = JsonProps::new();
        if cls == SPIN {
            let props = props_of_actor(ctx, owner)?;
            let actor = name_of(owner).to_owned();
            let idx = match self.spins.iter().position(|s| s.actor == actor) {
                Some(i) => i,
                None => {
                    let mut fields = JsonProps::new();
                    put(&mut fields, "actor", actor.as_str());
                    let mut rates = JsonProps::new();
                    for (k, r) in SPIN_RATES {
                        put(&mut rates, k, r);
                    }
                    put(&mut fields, "yawRate", Json::Obj(rates));
                    if let Some(par) = comp_named(ctx, owner, "Comp_Parallax") {
                        for (k, x) in parallax_of(props.get(par)?, ctx)? {
                            fields.insert(k, x);
                        }
                    }
                    self.spins.push(SpinRec { actor, fields, comps: None });
                    self.spins.len() - 1
                }
            };
            // the component's relative transform and its parents' up to the root, whose relative one is the actor's
            // place (the spin turns CloudRing_Mesh / _Mesh1 / CloudRing_Wisp_Mesh and Sphere, the planes' parent)
            let comps = self.spins[idx].comps.get_or_insert_with(JsonProps::new);
            let mut at_obj = Some(e);
            while let Some(cur) = at_obj {
                if comps.contains_key(name_of(cur)) {
                    break;
                }
                let q = if cur.path() == e.path() { p.clone() } else { props.get(cur)?.clone() };
                let parent =
                    if truthy(q.get("AttachParent")) { Some(component_ref(q.get("AttachParent"))?) } else { None };
                if let Some(par) = &parent
                    && par.0 != name_of(owner)
                {
                    return fail(PART, format!("{}.{}: attached to another actor", owner.path(), name_of(cur)));
                }
                if truthy(q.get("AttachSocketName")) && !is_str(q.get("AttachSocketName"), "None") {
                    return fail(PART, format!("{}.{}: socket", owner.path(), name_of(cur)));
                }
                let o = xf_out(&rel_of(&q));
                let mut c = JsonProps::new();
                put(&mut c, "parent", parent.as_ref().map_or(Json::Null, |p| Json::Str(p.1.clone())));
                put(&mut c, "loc", triple(o.loc));
                put(&mut c, "rot", triple(o.rot));
                put(&mut c, "scale", triple(o.scale));
                comps.insert(name_of(cur).to_owned(), Json::Obj(c));
                at_obj = match &parent {
                    Some(par) => match comp_named(ctx, owner, &par.1) {
                        Some(n) => Some(n),
                        None => return fail(PART, format!("{}: no {}", owner.path(), par.1)),
                    },
                    None => None,
                };
            }
            put(&mut extra, "spin", name_of(owner));
        } else if PILLAR.contains(&cls) {
            // TreePillarCollision / TreePillarTopCollision: hidden in game (the stage's shapes). TreePillarCollision's
            // world location: where TreeFortPillarDamageVFX spawns its effect.
            if !truthy(p.get("bHiddenInGame")) {
                return fail(PART, format!("{}.{}: a shown pillar mesh", owner.path(), name_of(e)));
            }
            if name_of(e) == "TreePillarCollision" {
                self.pillar_of(ctx, owner)?;
                let w = placed(ctx, owner, e, p)?;
                put(self.pillar_of(ctx, owner)?, "collision", triple(vec(w.get("RelativeLocation"), [0.0; 3])));
            }
        } else if UPPER.contains(&cls) || SMALL.contains(&cls) {
            if name_of(e) != "StaticMesh" || p.get("bVisible") != Some(&Json::Bool(false)) {
                return fail(PART, format!("{}.{}: a shown platform mesh", owner.path(), name_of(e)));
            }
        } else if cls != TRAMPOLINE || name_of(e) != "WandStaticMeshPlane" {
            return fail(PART, format!("{}: static mesh component of {cls}", owner.path()));
        }
        Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: placed(ctx, owner, e, p)?, extra, placed: true }))
    }

    fn level_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut out = JsonProps::new();
        let kept: JsonProps =
            self.level.iter().filter(|(_, v)| truthy(Some(v))).map(|(k, v)| (k.clone(), v.clone())).collect();
        if !kept.is_empty() {
            put(&mut out, "treefort", Json::Obj(kept));
        }
        Ok(out)
    }

    fn actor_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut actors = JsonProps::new();
        put(&mut actors, "pillars", Json::Arr(self.pillars.values().map(|p| Json::Obj(p.clone())).collect()));
        put(&mut actors, "platforms", Json::Arr(self.platforms.clone()));
        let mut out = JsonProps::new();
        put(&mut out, "treefortActors", Json::Obj(actors));
        if !self.ladies.is_empty() || !self.spins.is_empty() {
            let spins = self
                .spins
                .iter()
                .map(|s| {
                    let mut o = s.fields.clone();
                    if let Some(c) = &s.comps {
                        o.insert("comps".into(), Json::Obj(c.clone()));
                    }
                    Json::Obj(o)
                })
                .collect();
            let mut sky = JsonProps::new();
            put(&mut sky, "ladies", Json::Arr(self.ladies.clone()));
            put(&mut sky, "spins", Json::Arr(spins));
            put(&mut out, "treefortSky", Json::Obj(sky));
        }
        Ok(out)
    }

    fn summary(&self) -> String {
        let snails = match self.level.get("snails").and_then(|s| s.get("actors")) {
            Some(Json::Arr(a)) => format!(", snails {}", a.len()),
            _ => String::new(),
        };
        let blackout = if truthy(self.level.get("blackout")) {
            format!(", BlackoutSphere at {BLACKOUT_AT} s")
        } else {
            String::new()
        };
        format!(
            ", pillars {}, platforms {}, Lady {}, cloud spins {}{snails}{blackout}",
            self.pillars.len(),
            self.platforms.len(),
            self.ladies.len(),
            self.spins.len()
        )
    }
}

fn triple2(v: [f64; 2]) -> Json {
    Json::Arr(v.into_iter().map(Json::Num).collect())
}
