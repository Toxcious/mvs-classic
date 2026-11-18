//! The stage data's sim part: the name, spawns, respawns, waypoints, bot recovery zones, centre, camera and blast
//! boxes, the placed actors' buffs and spawned actors, and the tagged plain actors. The placed
//! actors are the collision part's `placed`.

use indexmap::IndexMap;
use mvs_data::fighter::ActorDef;
use mvs_data::stage::{
    BlastBox, BlastBoxCorners, BlastShrink, BotZone, NumOrBool, StageCenter, StagePlacedData, StageStart, StageTagged,
    Waypoint,
};
use mvs_data::{Props, Value};

use super::super::actor_class::{ActorClass, ActorClasses, actor_passive_buffs};
use super::super::reach::BUFF_SPAWN_MEMBERS;
use super::super::shape::{cdo_path, fail, short_name};
use super::super::transform::quat_rotator;
use super::context::{StageCtx, Vec3, f, js_truthy, s, strings, v3, vec, vec0};
use super::reach::{BUFF_PARENT, LevelSpawn, level_spawns, stage_buff_reach};
use crate::kind::KindError;
use crate::objview::ObjView;

/// The data's unit note.
const UNITS: &str = "UE cm; X depth (+X toward camera), Y horizontal, Z up";
/// PFGCamera's class defaults (the camera is spawned by the game mode, no level places it).
const PFG_CAMERA_CDO: &str = "/Game/Panda_Main/Blueprints/PFGCamera.Default__PFGCamera_C";
/// The blast box actor the boxes are read from, by name (the gameplay level's).
const BLAST_ACTOR: &str = "BlastBoxExit";

/// The actor classes of the loop's branches this part reads.
const READ: [&str; 6] = [
    "BlastBoxPlayerExit_C",
    "CameraCenter_C",
    "PlayerStart",
    "BotWaypoint_C",
    "BotRecoveryZone_C",
    "CenterStageLocation_C",
];
/// ATriggerBox's constructor (exe 142e4a060): CollisionComp's BoxExtent 40, where no template stores one.
const TRIGGER_BOX_EXTENT: f64 = 40.0;

/// What the sim part writes.
pub struct SimOut {
    pub name: String,
    pub units: String,
    pub spawns: Vec<StageStart>,
    pub respawns: Vec<StageStart>,
    pub waypoints: Vec<Waypoint>,
    pub bot_zones: Option<Vec<BotZone>>,
    pub center: Option<StageCenter>,
    pub blast_boxes: IndexMap<String, BlastBox>,
    pub camera: IndexMap<String, NumOrBool>,
    pub camera_center: Vec<f64>,
    pub blast_box_corners: BlastBoxCorners,
    pub blast_shrink: BlastShrink,
    pub camera_shrunk_angle: Option<f64>,
    pub buffs: Option<IndexMap<String, Props>>,
    pub actors: Option<IndexMap<String, ActorDef>>,
    pub tagged: Option<Vec<StageTagged>>,
}

/// The sim part (SIM_PART.bind); `placed` is the collision part's.
pub fn bind(ctx: &StageCtx<'_>, placed: &[StagePlacedData]) -> Result<SimOut, KindError> {
    let name = ctx.level(0).level.rsplit('/').next().unwrap_or_default().to_owned();
    let mut spawns = Vec::new();
    let mut respawns = Vec::new();
    let mut waypoints = Vec::new();
    let mut bot_zones = Vec::new();
    let mut tagged = Vec::new();
    let mut blast: Option<ObjView<'_>> = None;
    let mut center: Option<StageCenter> = None;
    let mut camera_center: Option<Vec3> = None;
    let mut shrunk_angle: Option<f64> = None;
    let mut center_lens: Vec<(&str, Option<f64>)> = Vec::new();

    // ---- the actor loop: the gameplay and lighting levels' actors in export order. An actor goes to one branch: a
    // platform (no class read here is one), a stage block candidate (a Box tagged TraceEdges), a hidden actor (none),
    // then by its class.
    for a in ctx.actors() {
        let t = short_name(a.class());
        let ap = ctx.properties(&a)?;
        if t == "Actor" {
            // A plain Actor with Tags: a point the game modes find with GetAllActorsWithTag (the tutorial's minigames).
            if let Some(Value::Arr(tags)) = ap.get("Tags")
                && !tags.is_empty()
            {
                let (loc, rot, _) = ctx.root_xf(&a)?;
                let Some(tags) = strings(ap.get("Tags")) else {
                    return fail(format!("{}: Tags is no list of names", a.path()));
                };
                tagged.push(StageTagged { name: short_name(a.path()).to_owned(), tags, loc: v3(loc), rot: v3(rot) });
            }
            continue;
        }
        if !READ.contains(&t) {
            continue;
        }
        if has_trace_edges_box(ctx, &a)? {
            continue;
        }
        // (GetAllActorsOfClass, IsBotInNeverRecoverZone_BTD's list, takes a hidden actor too)
        if t == "BotRecoveryZone_C" {
            bot_zones.push(bot_zone(ctx, &a)?);
            continue;
        }
        if js_truthy(ap.get("bHidden")) {
            continue;
        }
        let (loc, rot, _) = ctx.root_xf(&a)?;
        match t {
            "BlastBoxPlayerExit_C" => blast = Some(a.clone()),
            "CameraCenter_C" => {
                // PFGCamera eases this actor; its map location is the first frame's camera. AdjustCameraForShrinking
                // BlastBox: CameraAngle goes to ShrunkCameraAngle (the instance's value over the class default's;
                // neither: 0).
                camera_center = Some(loc);
                let ccdo = ctx.properties(&ctx.load(&cdo_path(a.class()))?)?;
                let num = |p: &Props, k: &str| f(p, k);
                shrunk_angle =
                    Some(num(&ap, "ShrunkCameraAngle").or_else(|| num(&ccdo, "ShrunkCameraAngle")).unwrap_or(0.0));
                // PFGCamera's tick copies CameraAngle and CameraFOV from CameraCenterPoint (@1636, @1685): the
                // instance's values over the CameraCenter class defaults' (Map_TrophyRoom_LargePlatform's angle 6, the
                // TrophyRoom 2platforms and 4platforms maps' FOV 34).
                center_lens = vec![
                    ("CameraAngle", num(&ap, "CameraAngle").or_else(|| num(&ccdo, "CameraAngle"))),
                    ("CameraFOV", num(&ap, "CameraFOV").or_else(|| num(&ccdo, "CameraFOV"))),
                ];
            }
            "PlayerStart" => {
                // Every start, its PlayerStartTag null where it has none (the tutorial's punching bag starts: the
                // tutorial finds its starts by Actor Tags).
                let tag = match ap.get("PlayerStartTag") {
                    None => None,
                    Some(Value::Str(t)) => Some(t.clone()),
                    Some(_) => return fail(format!("{}: a PlayerStartTag that is no name", a.path())),
                };
                let tags = match ap.get("Tags") {
                    None => Vec::new(),
                    v => strings(v).map_or_else(|| fail(format!("{}: Tags is no list of names", a.path())), Ok)?,
                };
                let respawn = tag.as_deref().is_some_and(|t| t.starts_with("Respawn"));
                let e = StageStart { tag, tags, loc: v3(loc), yaw: rot[1] };
                if respawn { respawns.push(e) } else { spawns.push(e) }
            }
            "BotWaypoint_C" => {
                // (the instance's values, the port's defaults where it has none)
                waypoints.push(Waypoint {
                    name: short_name(a.path()).to_owned(),
                    loc: v3(loc),
                    position: f(&ap, "Position").unwrap_or(0.0),
                    edge: ap.get("Edge").and_then(Value::as_bool).unwrap_or(false),
                    recovery: f(&ap, "RecoveryWaypoint").unwrap_or(0.0),
                });
            }
            "CenterStageLocation_C" => {
                let Some(r) = f(&ap, "StageRadius") else {
                    return fail(format!("{}: a CenterStageLocation with no StageRadius", a.path()));
                };
                center = Some(StageCenter { loc: v3(loc), stage_radius: r });
            }
            _ => {}
        }
    }
    let Some(blast) = blast else { return fail("no BlastBoxPlayerExit_C actor") };
    let (blast_boxes, blast_box_corners, blast_shrink) = blast_boxes(ctx, &blast)?;

    // ---- the camera: PFGCamera's class defaults, its numbers and UseEasedMovement, CameraAngle and CameraFOV the
    // CameraCenter's
    let cam = ctx.properties(&ctx.load(PFG_CAMERA_CDO)?)?;
    let mut camera: IndexMap<String, NumOrBool> = IndexMap::new();
    for (k, v) in cam.iter() {
        if let Value::Num(n) = v {
            camera.insert(k.clone(), NumOrBool::Num(*n));
        }
    }
    match cam.get("UseEasedMovement") {
        Some(Value::Bool(b)) => camera.insert("UseEasedMovement".to_owned(), NumOrBool::Bool(*b)),
        _ => return fail("PFGCamera: UseEasedMovement is no boolean"),
    };
    for (k, v) in center_lens {
        if let Some(v) = v {
            camera.insert(k.to_owned(), NumOrBool::Num(v));
        }
    }
    let Some(camera_center) = camera_center else { return fail("no CameraCenter_C actor") };

    let (buffs, actors) = buffs_and_actors(ctx, placed)?;
    Ok(SimOut {
        name,
        units: UNITS.to_owned(),
        spawns,
        respawns,
        waypoints,
        bot_zones: (!bot_zones.is_empty()).then_some(bot_zones),
        center,
        blast_boxes,
        camera,
        camera_center: v3(camera_center),
        blast_box_corners,
        blast_shrink,
        camera_shrunk_angle: shrunk_angle,
        buffs,
        actors,
        tagged: (!tagged.is_empty()).then_some(tagged),
    })
}

/// A BotRecoveryZone_C (a TriggerBox child): its root CollisionComp's world centre, rotation and scaled half extent
/// (BoxExtent, its templates' or the TriggerBox constructor's, times the component's world scale).
fn bot_zone(ctx: &StageCtx<'_>, a: &ObjView<'_>) -> Result<BotZone, KindError> {
    let root = ctx.root(a)?;
    let Some(root) = root.filter(|r| short_name(r.class()) == "BoxComponent") else {
        return fail(format!("{}: no BoxComponent root", a.path()));
    };
    let w = ctx.world(&root)?;
    let ext = vec(ctx.with_templates(&root)?.get("BoxExtent"), [TRIGGER_BOX_EXTENT; 3]);
    Ok(BotZone {
        name: short_name(a.path()).to_owned(),
        center: v3(w.loc),
        half_extent: (0..3).map(|i| (ext[i] * w.scale[i]).abs()).collect(),
        rot: quat_rotator(w.q).to_vec(),
    })
}

/// An actor with a Box component tagged TraceEdges (its values over its templates') is a stage block candidate and goes
/// to no other branch.
fn has_trace_edges_box(ctx: &StageCtx<'_>, a: &ObjView<'_>) -> Result<bool, KindError> {
    for c in ctx.comps(a, Some("BoxComponent")) {
        let p = ctx.with_templates(&c)?;
        if let Some(Value::Arr(tags)) = p.get("ComponentTags")
            && tags.iter().any(|t| t.as_str() == Some("TraceEdges"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

type Blast = (IndexMap<String, BlastBox>, BlastBoxCorners, BlastShrink);

/// `blastBoxes`: the BlastBoxPlayerExit class's Box components (its `_GEN_VARIABLE` templates and its default object's
/// CollisionComp, by name less the suffix, a later one over an earlier one of the same name) and the gameplay level's
/// BlastBoxExit actor's own (each its instance's non-null values over the class's, one level deep), sorted by name:
/// the root CollisionComp carries the actor's location and scale, a child box its own scale times the root's.
/// `blastBoxCorners`: BlastBoxPlayerExit ReceiveBeginPlay @3885-4166 (and its bottom right after), the CollisionComp's
/// bounds inset by the class defaults' corner buffers. `blastShrink`: BPI_ShrinkBlastBox, the ShrinkTimeline's update
/// (ExecuteUbergraph @7751-8025) moves the actor to ShrunkPosition, sets the root box's unscaled extent to
/// ShrunkBoxExtent (OriginalBoxExtent is the bounds over the actor scale, @4268) and BlastBoxJumpOutBufferTop to
/// ShrunkJumpOutBuffer: the instance's values over the class defaults'.
fn blast_boxes(ctx: &StageCtx<'_>, blast_actor: &ObjView<'_>) -> Result<Blast, KindError> {
    let cls = ctx.load(blast_actor.class())?;
    let cdo = ctx.load(&cdo_path(blast_actor.class()))?;
    let mut inner: Vec<ObjView<'_>> = cls.inner()?;
    inner.extend(cdo.inner()?);
    inner.sort_by_key(ObjView::index);
    let mut tpl: IndexMap<String, std::rc::Rc<Props>> = IndexMap::new();
    for o in &inner {
        if short_name(o.class()) == "BoxComponent" {
            tpl.insert(short_name(o.path()).replacen("_GEN_VARIABLE", "", 1), ctx.properties(o)?);
        }
    }
    // (by name in the gameplay level; its class is not asked)
    let level0 = ctx.level(0);
    let Some(inst) = level0.actors.iter().map(|&i| &level0.objects[i]).find(|a| short_name(a.path()) == BLAST_ACTOR)
    else {
        return fail(format!("no {BLAST_ACTOR} actor in the gameplay level"));
    };
    let mut own: IndexMap<String, std::rc::Rc<Props>> = IndexMap::new();
    for c in ctx.comps(inst, Some("BoxComponent")) {
        own.insert(short_name(c.path()).to_owned(), ctx.properties(&c)?);
    }
    let Some(root) = own.get("CollisionComp") else { return fail(format!("{BLAST_ACTOR}: no CollisionComp")) };
    let r_loc = vec0(root.get("RelativeLocation"));
    let r_sc = vec(root.get("RelativeScale3D"), [1.0; 3]);
    let mut names: Vec<&String> = tpl.keys().chain(own.keys()).collect();
    names.sort_unstable();
    names.dedup();
    let mut boxes: IndexMap<String, BlastBox> = IndexMap::new();
    for k in names {
        let mut p: Props = tpl.get(k).map(|t| (**t).clone()).unwrap_or_default();
        if let Some(o) = own.get(k) {
            for (kk, v) in o.iter() {
                if !v.is_null() {
                    p.insert(kk.clone(), v.clone());
                }
            }
        }
        let ext = vec(p.get("BoxExtent"), [32.0; 3]);
        let sc = if k == "CollisionComp" {
            r_sc
        } else {
            let rs = vec(p.get("RelativeScale3D"), [1.0; 3]);
            [rs[0] * r_sc[0], rs[1] * r_sc[1], rs[2] * r_sc[2]]
        };
        let he = [ext[0] * sc[0], ext[1] * sc[1], ext[2] * sc[2]];
        boxes.insert(
            k.clone(),
            BlastBox {
                center: v3(r_loc),
                half_extent: v3(he),
                y: vec![r_loc[1] - he[1], r_loc[1] + he[1]],
                z: vec![r_loc[2] - he[2], r_loc[2] + he[2]],
            },
        );
    }

    let bcdo = ctx.properties(&cdo)?;
    let num = |k: &str| f(&bcdo, k).unwrap_or(0.0);
    let Some(bb) = boxes.get("CollisionComp") else { return fail("the blast boxes have no CollisionComp") };
    let (hb, tb, btm) =
        (num("BlastBoxCornerHorizontalBuffer"), num("BlastBoxCornerTopBuffer"), num("BlastBoxCornerBottomBuffer"));
    let corners =
        BlastBoxCorners { top_left: vec![bb.y[1] - hb, bb.z[1] - tb], bottom_right: vec![bb.y[0] + hb, bb.z[0] + btm] };

    // (the BlastBoxPlayerExit_C actor's own values: the last one the loop met)
    let ap = ctx.properties(blast_actor)?;
    let shrunk = |k: &str| -> Option<Value> {
        match ap.get(k) {
            Some(v) if !v.is_null() => Some(v.clone()),
            _ => bcdo.get(k).filter(|v| !v.is_null()).cloned(),
        }
    };
    let s_ext = vec0(shrunk("ShrunkBoxExtent").as_ref());
    let jump = match shrunk("ShrunkJumpOutBuffer") {
        Some(Value::Num(n)) if n != 0.0 && !n.is_nan() => n,
        _ => 0.0,
    };
    let shrink = BlastShrink {
        center: v3(vec0(shrunk("ShrunkPosition").as_ref())),
        half_extent: vec![s_ext[0] * r_sc[0], s_ext[1] * r_sc[1], s_ext[2] * r_sc[2]],
        jump_out_buffer_top: jump,
    };
    Ok((boxes, corners, shrink))
}

type BuffsAndActors = (Option<IndexMap<String, Props>>, Option<IndexMap<String, ActorDef>>);

/// `actors`: the classes the level Blueprints spawn (level_spawns: LEVEL_SPAWNS by each level's level script class, the
/// levels in order), each as the fighter data's spawned actors are bound. `buffs`: the
/// buffs the placed actors' classes and those spawned classes lead to (placed_buffs: STAGE_BUFF_REACH, then the spawned
/// classes' Comp_Pawn_PassiveBuffManager Buffs, then what those buffs lead to in turn, in order of reaching), each its
/// class defaults. Each key only where it has an entry.
fn buffs_and_actors(ctx: &StageCtx<'_>, placed: &[StagePlacedData]) -> Result<BuffsAndActors, KindError> {
    let classes = ActorClasses::new(ctx.cx, &ctx.key);
    let mut spawned: IndexMap<String, std::sync::Arc<ActorClass>> = IndexMap::new();
    for lv in &ctx.levels.levels {
        let Some(lsa) = level_script_class(ctx, &lv.objects)? else { continue };
        let Some(list) = level_spawns(short_name(&lsa)) else {
            return fail(format!(
                "level script {} ({}): not in the level spawns table (LEVEL_SPAWNS)",
                short_name(&lsa),
                lv.level
            ));
        };
        for sp in list {
            let paths: Vec<String> = match sp {
                LevelSpawn::Cls(c) => vec![(*c).to_owned()],
                LevelSpawn::Member(m) => {
                    let v = ctx.properties(&ctx.load(&cdo_path(&lsa))?)?.get(*m).cloned();
                    match strings(v.as_ref()) {
                        Some(l) => l,
                        None => return fail(format!("{}: {m} is no class list", short_name(&lsa))),
                    }
                }
            };
            for p in paths {
                if !spawned.contains_key(short_name(&p)) {
                    spawned.insert(short_name(&p).to_owned(), classes.actor(&p)?);
                }
            }
        }
    }

    let mut path_of: IndexMap<String, String> = IndexMap::new();
    let reach_of = |c: &str| -> Result<Vec<String>, KindError> {
        stage_buff_reach(c)
            .map_or_else(|| fail(format!("{c}: not in the stage buff reach table (STAGE_BUFF_REACH)")), Ok)
    };
    let add = |p: &str, path_of: &mut IndexMap<String, String>| -> bool {
        let n = short_name(p);
        if path_of.contains_key(n) {
            return false;
        }
        path_of.insert(n.to_owned(), p.to_owned());
        true
    };
    let roots: Vec<String> = placed.iter().map(|p| p.cls.clone()).chain(spawned.keys().cloned()).collect();
    for c in &roots {
        for b in reach_of(c)? {
            add(&b, &mut path_of);
        }
        if let Some(a) = spawned.get(c) {
            for b in actor_passive_buffs(a) {
                add(&b, &mut path_of);
            }
        }
    }
    let mut i = 0;
    while i < path_of.len() {
        let c = path_of.get_index(i).map(|(k, _)| k.clone()).expect("in range");
        let mut lead = Vec::new();
        for m in BUFF_SPAWN_MEMBERS.get(&c)? {
            let raw = classes.raw_defaults(&path_of[&c])?;
            if let Some(Value::Str(v)) = raw.get(*m) {
                lead.push(v.clone());
            }
        }
        for b in lead.into_iter().chain(reach_of(&c)?) {
            add(&b, &mut path_of);
        }
        i += 1;
    }
    let mut buffs: IndexMap<String, Props> = IndexMap::new();
    for (b, p) in &path_of {
        if !is_buff(ctx, p)? {
            return fail(format!("placed actors' buffs: {b} is no {BUFF_PARENT} child"));
        }
        buffs.insert(b.clone(), classes.class_defaults(p)?);
    }
    let actors: IndexMap<String, ActorDef> = spawned.iter().map(|(n, c)| (n.clone(), c.def.clone())).collect();
    Ok(((!buffs.is_empty()).then_some(buffs), (!actors.is_empty()).then_some(actors)))
}

/// A level's level script class (its PersistentLevel's LevelScriptActor's), where it is a Blueprint's.
fn level_script_class(ctx: &StageCtx<'_>, objects: &[ObjView<'_>]) -> Result<Option<String>, KindError> {
    let Some(pl) = objects.iter().find(|o| o.path().ends_with(":PersistentLevel")) else { return Ok(None) };
    let props = ctx.properties(pl)?;
    let Some(r) = s(&props, "LevelScriptActor") else { return Ok(None) };
    let lsa = match objects.iter().find(|o| o.path() == r) {
        Some(o) => o.clone(),
        None => ctx.load(r)?,
    };
    Ok(lsa.class().starts_with("/Game/").then(|| lsa.class().to_owned()))
}

/// A class that is BuffParentActor_C's child (not itself), by its Blueprint parents.
fn is_buff(ctx: &StageCtx<'_>, cls: &str) -> Result<bool, KindError> {
    let mut c = ctx.load(cls)?.super_path()?;
    while let Some(p) = c.filter(|p| p.starts_with("/Game/")) {
        if short_name(&p) == BUFF_PARENT {
            return Ok(true);
        }
        c = ctx.load(&p)?.super_path()?;
    }
    Ok(false)
}
