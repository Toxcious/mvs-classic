//! Sky Arena's scenery part (`m011`): its own Blueprint actors and the Animation sub-level's level Blueprint.
//!
//! Floating objects: the Animation sub-level's level Blueprint moves every actor tagged Floating each tick. An
//! instance of one has `floating: true`; the entry's `floating` has the Blueprint's two CurveVector assets and the
//! ranges of its RandomFloatInRange calls in InitializeFloatingAnimation. The arm (`arm`): M011_Arm_C's Mesh is one of
//! the entry's `skeletal`; the range of its random delay, the animation it then plays once and the time that
//! animation's curve show_DonutBitten_01 passes the Blueprint's 0.9; its Donut and DonutBitten components are
//! instances with `attach` (the skeletal mesh and the bone) and DonutBitten `hidden`. The ship (`ship`): the level
//! Blueprint's random delay before its one Animate call and the timeline's length and two vector tracks; its Mesh is
//! an instance with `ship: true` at the actor's root.

use crate::game::MAPS_ROOT;
use crate::json::Json;
use crate::kind::KindError;
use crate::kinds::animation;
use crate::kinds::scenery::part::{
    InstanceActor, JsonProps, SceneryBindCtx, SceneryLevelRun, SceneryMeshOut, SceneryPart, SceneryPartRun,
    SkeletalOpts, StaticMeshOut,
};
use crate::objview::ObjView;

use super::util::{
    CurveKey, cdo_props, class_inner, component_props, event_times, json_truthy, name_of, props_of, reference, tags_of,
    vector_curve,
};

// InitializeFloatingAnimation's RandomFloatInRange calls, the same in the four Animation levels
// (Map_M011_Animation*.txt @411 anim length, @517 time offset, @623 move amplitude, @729 rotate amplitude), and the
// delay before the level Blueprint's one M011_Ship Animate call (@2314).
const FLOAT_LENGTH: [f64; 2] = [7.0, 10.0];
const FLOAT_OFFSET: [f64; 2] = [0.0, 0.9];
const FLOAT_MOVE: [f64; 2] = [80.0, 120.0];
const FLOAT_ROTATE: [f64; 2] = [7.0, 20.0];
const SHIP_DELAY: [f64; 2] = [20.0, 120.0];
// M011_Arm: BeginPlay's Delay(RandomFloatInRange(20, 60)) @373, then Mesh.PlayAnimation(M011_Arm_GrabDonut, not
// looping) @15; each tick until it is true, AnimInstance.GetCurveValue(show_DonutBitten_01) > 0.9 (@168, @231) swaps
// the donuts.
const ARM_DELAY: [f64; 2] = [20.0, 60.0];
const ARM_GRAB: &str = "/Game/Panda_Main/Maps/M011/Meshes/M011_Arm_GrabDonut.M011_Arm_GrabDonut";
const ARM_CURVE: &str = "show_DonutBitten_01";
const ARM_BITTEN: f64 = 0.9;
// M011_Ship: PlayRings SpawnEmitterAtLocation(PS_Ship_ExhaustSmoke) at the Mesh's world transform @160 and posts
// sfx_map_M011_Ship_Appear at its location @290; PlayThrust SpawnEmitterAttached(PS_Spaceship_Thruster, Mesh, offset
// (0, 0, -450)) @668, kept in ThrusterVFX, and posts sfx_map_M011_Ship_Disappear @423; StopThrust and the timeline's
// end destroy ThrusterVFX; the timeline's update sets the Mesh's relative scale to ScaleRoamingEye @871 and adds
// MoveRoamingEye as a local offset @917.
const RINGS: &str = "/Game/Panda_Main/Maps/M011/VFX/PS_Ship_ExhaustSmoke.PS_Ship_ExhaustSmoke";
const RINGS_SOUND: &str = "sfx_map_M011_Ship_Appear";
const THRUST: &str = "/Game/Panda_Main/Maps/M011/VFX/PS_Spaceship_Thruster.PS_Spaceship_Thruster";
const THRUST_SOUND: &str = "sfx_map_M011_Ship_Disappear";
const THRUST_OFFSET: [f64; 3] = [0.0, 0.0, -450.0];

fn fail<T>(what: impl std::fmt::Display) -> Result<T, KindError> {
    Err(KindError::Failed(format!("scenery m011: {what}")))
}

fn nums(v: &[f64]) -> Json {
    Json::Arr(v.iter().map(|&n| Json::Num(n)).collect())
}

/// A curve's keys as the entry writes them (arrays of [time, value, arrive, leave, mode]).
fn curves(c: &[Vec<CurveKey>]) -> Json {
    Json::Arr(c.iter().map(|ch| Json::Arr(ch.iter().map(|k| nums(k)).collect())).collect())
}

/// An array property (`x as Props[]`), else a failure.
fn array<'j>(v: Option<&'j Json>, what: &str) -> Result<&'j [Json], KindError> {
    match v {
        Some(Json::Arr(a)) => Ok(a),
        _ => fail(format!("{what} is not an array")),
    }
}

/// The level Blueprint's floating animation: its curves from the class defaults.
fn floating_of<'a>(ctx: &dyn SceneryBindCtx<'a>, cls: &str) -> Result<Json, KindError> {
    let level = &ctx.level().level;
    let base = &level[level.rfind('/').map_or(0, |i| i + 1)..];
    let d = cdo_props(ctx, &format!("{MAPS_ROOT}{level}.{base}_C"))?;
    if cls != format!("{base}_C")
        || !d.get("Move Curves").is_some_and(json_truthy)
        || !d.get("Rotate Curves").is_some_and(json_truthy)
    {
        return fail(format!("{level}: no floating animation"));
    }
    let curve = |v: Option<&Json>| -> Result<Json, KindError> {
        let o = ctx.load(&reference(v, "a floating curve")?)?;
        Ok(curves(&vector_curve(&props_of(&o))?))
    };
    Ok(Json::object([
        ("move", curve(d.get("Move Curves"))?),
        ("rotate", curve(d.get("Rotate Curves"))?),
        ("length", nums(&FLOAT_LENGTH)),
        ("offset", nums(&FLOAT_OFFSET)),
        ("moveAmplitude", nums(&FLOAT_MOVE)),
        ("rotateAmplitude", nums(&FLOAT_ROTATE)),
    ]))
}

pub struct SkyArena;

impl SceneryPart for SkyArena {
    fn name(&self) -> &'static str {
        "m011"
    }
    fn blueprints(&self) -> &'static [&'static str] {
        &["M011_Arm_C", "M011_Ship_C"]
    }
    fn levels(&self) -> &'static [&'static str] {
        &[
            "Map_M011_Animation_LargeNoPlat_C",
            "Map_M011_Animation_SmallNoPlat_C",
            "Map_M011_Animation_C",
            "Map_M011_Animation_Small_C",
        ]
    }
    fn level_tags(&self) -> &'static [&'static str] {
        &["Floating"]
    }
    fn ignore(&self) -> &'static [(&'static str, &'static str)] {
        &[
            (
                "Map_M011_Background_LargeNoPlat_C",
                "BeginPlay forces LOD 2 on the cloud meshes only at a visual effect quality of 1 or lower or on Xbox One / PS4 (Map_M011_Background*.txt): neither here",
            ),
            (
                "Map_M011_Background_SmallNoPlat_C",
                "BeginPlay forces LOD 2 on the cloud meshes only at a visual effect quality of 1 or lower or on Xbox One / PS4 (Map_M011_Background*.txt): neither here",
            ),
            (
                "Map_M011_Background_C",
                "BeginPlay forces LOD 2 on the cloud meshes only at a visual effect quality of 1 or lower or on Xbox One / PS4 (Map_M011_Background*.txt): neither here",
            ),
            (
                "Map_M011_Background_Small_C",
                "BeginPlay forces LOD 2 on the cloud meshes only at a visual effect quality of 1 or lower or on Xbox One / PS4 (Map_M011_Background*.txt): neither here",
            ),
        ]
    }
    /// M011_Whip_A_01_M: its base pass vertex shader's wind.
    fn wpo(&self) -> &'static [&'static str] {
        &["/Game/Panda_Main/Maps/M011/Materials/M011_Whip_A_01_M.M011_Whip_A_01_M"]
    }
    fn run<'a>(&self) -> Box<dyn SceneryPartRun<'a> + 'a> {
        Box::new(Run::default())
    }
}

#[derive(Default)]
struct Run {
    floating: Option<Json>,
    arm: Option<Json>,
    ship: Option<Json>,
}

impl Run {
    fn ship_mesh<'a>(
        &mut self,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        p: &JsonProps,
    ) -> Result<StaticMeshOut, KindError> {
        // the Mesh under the actor's root; BeginPlay hides it
        let root = ctx.root_of(owner)?;
        if root.get("RelativeScale3D").is_some_and(json_truthy) || self.ship.is_some() {
            return fail(format!("{}: ship", owner.path()));
        }
        let mut out = p.clone();
        for k in ["RelativeLocation", "RelativeRotation"] {
            // `root.k ?? null`, then the null ones deleted
            match root.get(k).filter(|v| !matches!(v, Json::Null)) {
                Some(v) => {
                    out.insert(k.into(), v.clone());
                }
                None => {
                    out.shift_remove(k);
                }
            }
        }
        let tl = props_of(&class_inner(ctx, owner.class(), "Timeline_0_Template")?);
        let track_of = |list: &str, name: &str| -> Result<Json, KindError> {
            let found =
                array(tl.get(list), list)?.iter().find(|x| x.get("TrackName").and_then(Json::as_str) == Some(name));
            found.cloned().map_or_else(|| fail(format!("M011_Ship timeline has no {name}")), Ok)
        };
        let track = |name: &str| -> Result<Json, KindError> {
            let t = track_of("VectorTracks", name)?;
            let o = ctx.load(&reference(t.get("CurveVector"), name)?)?;
            Ok(curves(&vector_curve(&props_of(&o))?))
        };
        let events = |name: &str| -> Result<Json, KindError> {
            let t = track_of("EventTracks", name)?;
            let o = ctx.load(&reference(t.get("CurveKeys"), name)?)?;
            Ok(nums(&event_times(&props_of(&o))?))
        };
        if array(tl.get("EventTracks"), "EventTracks")?.len() != 3 {
            return fail("M011_Ship effects changed");
        }
        let mut ship = Json::obj();
        ship.set("delay", nums(&SHIP_DELAY));
        if let Some(l) = tl.get("TimelineLength") {
            ship.set("length", l.clone());
        }
        ship.set("move", track("MoveRoamingEye")?);
        ship.set("scale", track("ScaleRoamingEye")?);
        let rings = Json::object([
            ("system", Json::from(ctx.add_system(RINGS)?)),
            ("at", events("PlayRings")?),
            ("sound", Json::from(RINGS_SOUND)),
        ]);
        ship.set("rings", rings);
        let thrust = Json::object([
            ("system", Json::from(ctx.add_system(THRUST)?)),
            ("at", events("PlayThrust")?),
            ("sound", Json::from(THRUST_SOUND)),
            ("stop", events("StopThrust")?),
            ("offset", nums(&THRUST_OFFSET)),
        ]);
        ship.set("thrust", thrust);
        self.ship = Some(ship);
        let extra = JsonProps::from([("ship".to_owned(), Json::Bool(true))]);
        Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: out, extra, placed: false }))
    }
}

impl<'a> SceneryPartRun<'a> for Run {
    fn level(&mut self, cls: &str, ctx: &dyn SceneryBindCtx<'a>) -> Result<SceneryLevelRun, KindError> {
        if self.floating.is_some() {
            return fail("two floating animations");
        }
        self.floating = Some(floating_of(ctx, cls)?);
        Ok(SceneryLevelRun { tags: vec!["Floating".into()] })
    }

    fn instance(&mut self, actor: InstanceActor<'_, 'a>) -> Result<JsonProps, KindError> {
        let mut out = JsonProps::new();
        if let InstanceActor::Placed(o) = actor
            && tags_of(&props_of(o)).iter().any(|t| t == "Floating")
        {
            out.insert("floating".into(), Json::Bool(true));
        }
        Ok(out)
    }

    fn skeletal_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        component: &ObjView<'a>,
    ) -> Result<bool, KindError> {
        if cls != "M011_Arm_C" {
            return Ok(false);
        }
        // M011_Arm_C: the Mesh under the root
        let q = component_props(ctx, component)?;
        let placed = |k: &str| q.get(k).is_some_and(json_truthy);
        if placed("RelativeLocation") || placed("RelativeRotation") || placed("RelativeScale3D") || self.arm.is_some() {
            return fail(format!("{}: arm mesh", owner.path()));
        }
        ctx.add_skeletal(name_of(owner), &q, &ctx.root_of(owner)?, &SkeletalOpts::default())?;
        // (the animation kind's curve keys: interp 1 constant)
        let clip = animation::header(ctx.cx(), ARM_GRAB)?;
        let keys: &[_] = clip.curves.iter().find(|c| c.name == ARM_CURVE).map_or(&[], |c| &c.keys);
        let k = keys.iter().position(|key| key.value > ARM_BITTEN);
        let Some(k) = k.filter(|&k| k >= 1 && keys.iter().all(|key| key.interp == 1.0)) else {
            return fail(format!("{ARM_CURVE} keys"));
        };
        self.arm = Some(Json::object([
            ("skeletal", Json::from(name_of(owner))),
            ("delay", nums(&ARM_DELAY)),
            ("anim", Json::from(ctx.anim_path(ARM_GRAB, false)?)),
            ("bittenAt", Json::Num(keys[k].time)),
        ]));
        Ok(true)
    }

    fn static_mesh(
        &mut self,
        cls: &str,
        ctx: &dyn SceneryBindCtx<'a>,
        owner: &ObjView<'a>,
        _component: &ObjView<'a>,
        p: &JsonProps,
    ) -> Result<StaticMeshOut, KindError> {
        if cls == "M011_Ship_C" {
            return self.ship_mesh(ctx, owner, p);
        }
        if cls != "M011_Arm_C" {
            return fail(format!("{}: static mesh component of {cls}", owner.path()));
        }
        // M011_Arm_C's Donut and DonutBitten, on a bone of its Mesh
        let mut out = p.clone();
        out.shift_remove("bVisible");
        let mut extra = JsonProps::new();
        let socket = p.get("AttachSocketName").cloned().unwrap_or_default();
        extra.insert("attach".into(), Json::Arr(vec![Json::from(name_of(owner)), socket]));
        if p.get("bVisible") == Some(&Json::Bool(false)) {
            extra.insert("hidden".into(), Json::Bool(true));
        }
        Ok(StaticMeshOut::Drawn(SceneryMeshOut { p: out, extra, placed: false }))
    }

    fn level_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut b = JsonProps::new();
        if let Some(f) = &self.floating {
            b.insert("floating".into(), f.clone());
        }
        Ok(b)
    }

    fn actor_blocks(&mut self) -> Result<JsonProps, KindError> {
        let mut b = JsonProps::new();
        if let Some(a) = &self.arm {
            b.insert("arm".into(), a.clone());
        }
        if let Some(s) = &self.ship {
            b.insert("ship".into(), s.clone());
        }
        Ok(b)
    }

    fn summary(&self) -> String {
        let Some(ship) = &self.ship else { return String::new() };
        let list = |k: &str, f: &str| {
            let a = ship.get(k).and_then(|r| r.get(f)).and_then(Json::as_array).unwrap_or(&[]);
            a.iter().map(|n| n.as_f64().map_or_else(String::new, js_number)).collect::<Vec<_>>().join(",")
        };
        format!(
            ", ship rings at {}, thrust at {} to {}",
            list("rings", "at"),
            list("thrust", "at"),
            list("thrust", "stop")
        )
    }
}

/// A number as JavaScript prints it.
fn js_number(n: f64) -> String {
    String::from_utf8(crate::json::to_vec(&Json::Num(n)).unwrap_or_default()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_and_summary() {
        assert_eq!(SkyArena.levels().len(), 4);
        let mut run = Run::default();
        assert_eq!(run.summary(), "");
        run.ship = Some(Json::object([
            ("rings", Json::object([("at", nums(&[1.5, 2.0]))])),
            ("thrust", Json::object([("at", nums(&[3.0])), ("stop", nums(&[9.25]))])),
        ]));
        assert_eq!(run.summary(), ", ship rings at 1.5,2, thrust at 3 to 9.25");
    }
}
