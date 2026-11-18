//! The Throne Room's scenery part (`m008`): its own Blueprint actors and level Blueprint.
//!
//! This writes what each draws at its place and pose at BeginPlay; the block `throneRoom` names the pieces the renderer
//! moves with the sim (the movers following their Box solids, the level script's intro, fold and reverse plays):
//! - M008_Elevator_BP_C, the five movers: SkeletalMesh under DefaultSceneRoot (single node, no animation, not
//!   playing, not looping: the reference pose until the level script plays a fold on it). Its Box and Comp_EdgeTrace
//!   are the stage data's; the sim moves the root: `movers`, in the sim's ROLE_NAMES order, with the root's place in
//!   the level.
//! - M008_Stage_01_BP_C (data-only): its StaticMesh (M008_Stage_01, NoCollision) and Skelmesh (M008_Stage_01_SkelMesh,
//!   StageFold at 0, not playing) under RootSceneComponent; its Box is the stage's floor.
//! - M008_Pillar_01_SkelMesh*_Blueprint_C (four SkeletalMeshActor children): the native root SkeletalMeshComponent0
//!   with the class's play data (M008_Pillar_01_Raise, not looping, playing: the core's single-node rule), and
//!   BeginPlay's SpawnEmitterAttached(PS_M008_Fire, SkeletalMeshComponent, Pillar_Buttom, ..., KeepRelativeOffset,
//!   bAutoDestroy, no pooling, bAutoActivate) @10: `pillars[i].fire`, its bone (Pillar_Buttom is a bone of
//!   M008_Pillar_01_SkelMesh_Skeleton, no socket of that name), which the renderer spawns at begin and follows on the
//!   bone.
//! - Map_M008_C (three instances in the level, all the same class: read once): the ten components BeginPlay caches
//!   the play rates of (`cached`, GetPlayRate @8194..8842 in that order; PlayTransformAnim's ResetAnims puts them back
//!   at 0, PlayAnimsInReverse reverses them) and per animated component its calls (`anims`: the folds' PlayAnimation
//!   and the wall gears' Play, CALLS below; the PlayAnimation assets by build object path), keyed to the sim's
//!   presentation calls, which the renderer plays. The gameplay level's SkeletalMeshActors (alcoves, gears, throne) are
//!   the core's: their animation at 0, not playing, until the renderer plays them.

use indexmap::IndexMap;

use crate::json::Json;
use crate::kind::KindError;
use crate::kinds::scenery::part::{
    JsonProps, SceneryBindCtx, SceneryLevelRun, SceneryMeshOut, SceneryPart, SceneryPartRun, SkeletalOpts,
    StaticMeshOut,
};
use crate::kinds::skeleton;
use crate::objview::ObjView;

use super::util::{comp_of, component_props, json_truthy, name_of, props_of, props_of_actor, reference, vec};

const ELEVATOR: &str = "M008_Elevator_BP_C";
const STAGE: &str = "M008_Stage_01_BP_C";
const PILLARS: [&str; 4] = [
    "M008_Pillar_01_SkelMesh_Blueprint_C",
    "M008_Pillar_01_SkelMesh2_Blueprint_C",
    "M008_Pillar_01_SkelMesh3_Blueprint_C",
    "M008_Pillar_01_SkelMesh4_Blueprint_C",
];
const LEVEL: &str = "Map_M008_C";
// the elevator's role names (Default__Map_M008_C's actor references)
const ROLES: [&str; 5] = [
    "ThroneRoomLeftElevator_7",
    "ThroneRoomRightElevator_9",
    "ThroneRoomLeftPillar_8",
    "ThroneRoomRightPillar_10",
    "ThroneRoomForegroundElevator",
];
const FIRE: &str = "/Game/Panda_Main/Maps/M008/VFX/PS_M008_Fire.PS_M008_Fire";
const FIRE_BONE: &str = "Pillar_Buttom";
const ANIMS: &str = "/Game/Panda_Main/Maps/M008/Animations/";

// the skeletal entry a level script reference names: a SkeletalMeshActor's is its actor's name, a Blueprint actor's
// component "<actor>.<component>" (the core, this part)
const SM: &str = "SkeletalMeshComponent";
fn entry_of(actor: &str, comp: &str) -> String {
    if comp == SM { actor.to_owned() } else { format!("{actor}.{comp}") }
}

// The level script's animation calls on one component each, by the presentation call they belong to: (field, actor,
// component, the PlayAnimation asset or None for Play(false)). PlayAnimation 142999990 is SetAnimationAsset(asset, not
// looping, rate 1.0), SetPlaying(true), SetLooping(false); Play(false) plays the asset the component has from where it
// is, at its rate. The stage's Skelmesh SetPosition(0.0, false) @5401 comes before its PlayAnimation @5461 (the same as
// PlayAnimation's own start at 0). These are every Play / PlayAnimation the graph makes on a referenced component.
// (The call offsets, @7644 and the like, are the graph's addresses; only the order is used.)
#[rustfmt::skip]
const CALLS: [(&str, &str, &str, Option<&str>); 15] = [
    ("introGears", "M008_Gears_BehindThrone_01_SkelMesh_5", SM, None),
    ("introCentre", "M008_Center_Alcove_SkelMesh_2", SM, None),
    ("introSides", "M008_Alcove_01_SkelMesh2", SM, None),
    ("introSides", "M008_Alcove_01_SkelMesh_2", SM, None),
    ("introThrone", "M008_Throne_01_SkelMesh_2", SM, None),
    ("towerFold", "ThroneRoomLeftPillar_8", "SkeletalMesh", Some("M008_Tower_01_Fold")),
    ("towerFold", "ThroneRoomRightPillar_10", "SkeletalMesh", Some("M008_Tower_01_Fold")),
    ("floorFold", "ThroneRoomForegroundElevator", "SkeletalMesh", Some("M008_FloorFG_Fold")),
    ("elevatorFold", "ThroneRoomLeftElevator_7", "SkeletalMesh", Some("M008_Elevator_01_Fold")),
    ("elevatorFold", "ThroneRoomRightElevator_9", "SkeletalMesh", Some("M008_Elevator_01_Fold")),
    ("wallGears", "M008_Gears_Walls_01_SkelMesh_5", SM, None),
    ("wallGears", "M008_Gears_Walls_01_SkelMesh2", SM, None),
    ("stageFold", "M008_Stage_01_BP_2", "Skelmesh", Some("M008_Stage_01_Fold")),
    ("stageFold", "M008_Alcove_01_SkelMesh_2", SM, Some("M008_Alcove_01_Tower_Fold")),
    ("stageFold", "M008_Alcove_01_SkelMesh2", SM, Some("M008_Alcove_01_Tower_Fold")),
];
// BeginPlay's GetPlayRate cache in its order (@8194, 8266, 8338, 8410, 8482, 8554, 8626, 8698, 8770, 8842): (actor,
// component); ResetAnims and PlayAnimsInReverse walk its keys.
const CACHED: [(&str, &str); 10] = [
    ("ThroneRoomLeftPillar_8", "SkeletalMesh"),
    ("ThroneRoomRightPillar_10", "SkeletalMesh"),
    ("ThroneRoomForegroundElevator", "SkeletalMesh"),
    ("M008_Gears_Walls_01_SkelMesh2", SM),
    ("M008_Gears_Walls_01_SkelMesh_5", SM),
    ("ThroneRoomLeftElevator_7", "SkeletalMesh"),
    ("ThroneRoomRightElevator_9", "SkeletalMesh"),
    ("M008_Stage_01_BP_2", "Skelmesh"),
    ("M008_Alcove_01_SkelMesh_2", SM),
    ("M008_Alcove_01_SkelMesh2", SM),
];

fn fail<T>(what: impl std::fmt::Display) -> Result<T, KindError> {
    Err(KindError::Failed(format!("scenery m008: {what}")))
}

/// The level script's `cached` and `anims` (per component its calls in the cycle's order: [field, op, asset]; op
/// 'asset' PlayAnimation, 'play' Play(false), 'reset' ResetAnims, 'reverse' PlayAnimsInReverse).
fn level_script<'a>(ctx: &dyn SceneryBindCtx<'a>) -> Result<(Json, Json), KindError> {
    let cached: Vec<String> = CACHED.iter().map(|(a, c)| entry_of(a, c)).collect();
    let mut anims: IndexMap<String, Vec<Json>> = IndexMap::new();
    let call = |field: &str, op: &str, asset: Json| Json::Arr(vec![Json::from(field), Json::from(op), asset]);
    for name in &cached {
        anims.entry(name.clone()).or_default().push(call("transform", "reset", Json::Null));
    }
    for (field, a, c, asset) in CALLS {
        let asset = match asset {
            Some(n) => Json::from(ctx.anim_path(&format!("{ANIMS}{n}.{n}"), false)?),
            None => Json::Null,
        };
        anims.entry(entry_of(a, c)).or_default().push(call(
            field,
            if asset == Json::Null { "play" } else { "asset" },
            asset,
        ));
    }
    for name in &cached {
        anims.entry(name.clone()).or_default().push(call("reverse", "reverse", Json::Null));
    }
    let anims = anims
        .into_iter()
        .map(|(name, calls)| Json::object([("skeletal", Json::from(name)), ("calls", Json::Arr(calls))]))
        .collect();
    Ok((Json::Arr(cached.into_iter().map(Json::from).collect()), Json::Arr(anims)))
}

pub struct ThroneRoom;

impl SceneryPart for ThroneRoom {
    fn name(&self) -> &'static str {
        "m008"
    }
    fn blueprints(&self) -> &'static [&'static str] {
        &[
            "M008_Elevator_BP_C",
            "M008_Stage_01_BP_C",
            "M008_Pillar_01_SkelMesh_Blueprint_C",
            "M008_Pillar_01_SkelMesh2_Blueprint_C",
            "M008_Pillar_01_SkelMesh3_Blueprint_C",
            "M008_Pillar_01_SkelMesh4_Blueprint_C",
        ]
    }
    fn levels(&self) -> &'static [&'static str] {
        &["Map_M008_C"]
    }
    fn run<'a>(&self) -> Box<dyn SceneryPartRun<'a> + 'a> {
        Box::new(Run::default())
    }
}

#[derive(Default)]
struct Run {
    movers: [Option<Json>; 5],
    pillars: [Option<Json>; 4],
    stage_skeletal: Option<String>,
    stage_mesh: Option<String>,
    script: Option<(Json, Json)>,
}

impl<'a> SceneryPartRun<'a> for Run {
    fn level(&mut self, cls: &str, ctx: &dyn SceneryBindCtx<'a>) -> Result<SceneryLevelRun, KindError> {
        if cls != LEVEL {
            return fail(format!("level {cls}"));
        }
        if self.script.is_none() {
            self.script = Some(level_script(ctx)?);
        }
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
        if cls != STAGE || name_of(e) != "StaticMesh" || self.stage_mesh.is_some() {
            return fail(format!("{}: static mesh component {} of {cls}", owner.path(), name_of(e)));
        }
        self.stage_mesh = Some(format!("{}.{}", name_of(owner), name_of(e)));
        let props = props_of_actor(ctx, owner)?;
        let mut out = p.clone();
        out.extend(ctx.place_of(e, Some(&props))?);
        Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: out, extra: JsonProps::new(), placed: true }))
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
        if cls == ELEVATOR {
            if name_of(e) != "SkeletalMesh" {
                return Ok(false);
            }
            let r = ROLES.iter().position(|r| *r == name_of(owner));
            let Some(r) = r.filter(|&r| self.movers[r].is_none()) else {
                return fail(format!("{}: not one of the level script's movers", owner.path()));
            };
            let rp = component_props(ctx, comp_of(&ctx.level().objects, owner, "DefaultSceneRoot")?)?;
            if ["AttachParent", "RelativeRotation", "RelativeScale3D"]
                .iter()
                .any(|k| rp.get(*k).is_some_and(json_truthy))
            {
                return fail(format!("{}: root", owner.path()));
            }
            let root = Json::Arr(vec(rp.get("RelativeLocation"), [0.0; 3]).into_iter().map(Json::Num).collect());
            ctx.add_skeletal(&name, &p, &ctx.place_of(e, Some(&props))?, &SkeletalOpts::default())?;
            self.movers[r] = Some(Json::object([
                ("actor", Json::from(name_of(owner))),
                ("skeletal", Json::from(name)),
                ("root", root),
            ]));
            return Ok(true);
        }
        if cls == STAGE {
            if name_of(e) != "Skelmesh" || self.stage_skeletal.is_some() {
                return Ok(false);
            }
            ctx.add_skeletal(&name, &p, &ctx.place_of(e, Some(&props))?, &SkeletalOpts::default())?;
            self.stage_skeletal = Some(name);
            return Ok(true);
        }
        let Some(k) = PILLARS.iter().position(|c| *c == cls) else { return Ok(false) };
        if name_of(e) != "SkeletalMeshComponent0" {
            return Ok(false);
        }
        let mesh = ctx.load(&reference(p.get("SkeletalMesh"), &format!("{cls} mesh"))?)?;
        let skel = skeleton::entry(ctx.cx(), &reference(props_of(&mesh).get("Skeleton"), &format!("{cls} skeleton"))?)?;
        if !skel.bones.iter().any(|b| b == FIRE_BONE) {
            return fail(format!("{cls}: no bone {FIRE_BONE}"));
        }
        ctx.add_skeletal(name_of(owner), &p, &ctx.place_of(e, Some(&props))?, &SkeletalOpts::default())?;
        let fire = Json::object([("system", Json::from(ctx.add_system(FIRE)?)), ("bone", Json::from(FIRE_BONE))]);
        self.pillars[k] = Some(Json::object([
            ("actor", Json::from(name_of(owner))),
            ("skeletal", Json::from(name_of(owner))),
            ("fire", fire),
        ]));
        Ok(true)
    }

    fn actor_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut b = JsonProps::new();
        let Some(skeletal) = &self.stage_skeletal else { return Ok(b) };
        // (the pillars array is checked by its length, the last set index + 1, and `some` skips a hole)
        let len = self.pillars.iter().rposition(Option::is_some).map_or(0, |i| i + 1);
        let (Some(mesh), Some((cached, anims))) = (&self.stage_mesh, &self.script) else {
            return fail("Throne Room: actors");
        };
        if self.movers.iter().any(Option::is_none) || len != PILLARS.len() {
            return fail("Throne Room: actors");
        }
        let list = |v: &[Option<Json>]| Json::Arr(v.iter().map(|x| x.clone().unwrap_or_default()).collect());
        let stage = Json::object([("skeletal", Json::from(skeletal.as_str())), ("mesh", Json::from(mesh.as_str()))]);
        let block = Json::object([
            ("movers", list(&self.movers)),
            ("stage", stage),
            ("pillars", list(&self.pillars)),
            ("cached", cached.clone()),
            ("anims", anims.clone()),
        ]);
        b.insert("throneRoom".into(), block);
        Ok(b)
    }

    fn summary(&self) -> String {
        format!(
            ", Throne Room {} movers, {} pillars",
            self.movers.iter().flatten().count(),
            self.pillars.iter().flatten().count()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_and_summary() {
        assert_eq!(entry_of("A", SM), "A");
        assert_eq!(entry_of("A", "Skelmesh"), "A.Skelmesh");
        // every cached component has a call (the walk of the script's keys)
        for (a, c) in CACHED {
            assert!(CALLS.iter().any(|(_, ca, cc, _)| *ca == a && *cc == c), "{a}.{c}");
        }
        let mut run = Run::default();
        run.pillars[3] = Some(Json::Null);
        assert_eq!(run.summary(), ", Throne Room 0 movers, 1 pillars");
        assert!(run.actor_blocks().unwrap().is_empty());
    }
}
