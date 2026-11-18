use mvs_data::value::{Props, Value};

use super::*;

/// A pose of `n` bones at identity rotation and unit scale, each translated by `t` (UE cm) under its parent.
fn pose(t: &[V3]) -> Pose {
    let mut p = Pose::new(t.len());
    for (i, v) in t.iter().enumerate() {
        // UE (X, Y, Z) cm -> the view's (X, Z, Y) m
        p.t[i * 3] = (v[0] / 100.0) as f32;
        p.t[i * 3 + 1] = (v[2] / 100.0) as f32;
        p.t[i * 3 + 2] = (v[1] / 100.0) as f32;
        p.r[i * 4 + 3] = -1.0; // UE w 1 is the view's w -1
        for k in 0..3 {
            p.s[i * 3 + k] = 1.0;
        }
    }
    p
}

fn names(n: &[&str]) -> Vec<String> {
    n.iter().map(|s| s.to_string()).collect()
}

fn props(json: &str) -> Props {
    fn conv(v: &serde_json::Value) -> Value {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => Value::Num(n.as_f64().expect("a number")),
            serde_json::Value::String(s) => Value::Str(s.clone()),
            serde_json::Value::Array(a) => Value::Arr(a.iter().map(conv).collect()),
            serde_json::Value::Object(o) => Value::Obj(o.iter().map(|(k, v)| (k.clone(), conv(v))).collect()),
        }
    }
    match conv(&serde_json::from_str(json).expect("json")) {
        Value::Obj(o) => o,
        _ => panic!("an object"),
    }
}

fn close(a: &[f64], b: &[f64], tol: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

#[test]
fn a_component_transform_set_moves_the_bones_below() {
    let n = names(&["root", "a", "b"]);
    let parents = [-1, 0, 1];
    let mut p = pose(&[[0.0; 3], [0.0, 0.0, 10.0], [0.0, 0.0, 10.0]]);
    let mut cs = CsPose::new(&n, &parents);
    let mut e = cs.begin(&mut p);
    assert!(close(&e.get(2).t, &[0.0, 0.0, 20.0], 1e-5));
    let mut x = BoneXfs::default();
    x.push(1, UeXf { t: [5.0, 0.0, 10.0], ..UeXf::default() });
    e.local_blend(&x, 1.0);
    assert!(close(&e.get(1).t, &[5.0, 0.0, 10.0], 1e-5));
    // b keeps its local transform and follows a
    assert!(close(&e.get(2).t, &[5.0, 0.0, 20.0], 1e-5));
    // half way: the local translation blended
    let mut x = BoneXfs::default();
    x.push(1, UeXf { t: [15.0, 0.0, 10.0], ..UeXf::default() });
    e.local_blend(&x, 0.5);
    assert!(close(&e.get(1).t, &[10.0, 0.0, 10.0], 1e-5));
}

#[test]
fn a_spring_at_rest_stays_and_lags_a_move() {
    let n = names(&["root", "tip"]);
    let parents = [-1, 0];
    let skel = AnimNodeSkel { names: &n, parents: &parents, rest: None };
    let mut node = anim_node_of("AnimNode_SpringBone")(&props(r#"{"SpringBone":{"BoneName":"tip"}}"#), &skel);
    let mut cs = CsPose::new(&n, &parents);
    let comp = mesh_xf(0.0, 0.0, 1.0, 1.0);
    let mut p = pose(&[[0.0; 3], [0.0, 0.0, 50.0]]);
    node.update(0.0, 1.0);
    node.evaluate(&mut cs.begin(&mut p), &comp);
    for _ in 0..10 {
        node.update(1.0 / 60.0, 1.0);
        node.evaluate(&mut cs.begin(&mut p), &comp);
    }
    assert!(close(&cs.begin(&mut p).get(1).t, &[0.0, 0.0, 50.0], 1e-3));
    // the root moves 10 cm: the tip trails it, short of the full move
    let mut q = pose(&[[0.0, 10.0, 0.0], [0.0, 0.0, 50.0]]);
    node.update(1.0 / 60.0, 1.0);
    node.evaluate(&mut cs.begin(&mut q), &comp);
    let y = cs.begin(&mut q).get(1).t[1];
    assert!(y > 0.0 && y < 10.0, "{y}");
}

#[test]
fn a_two_bone_leg_reaches_its_ik_foot() {
    // root -> hip (z 100) -> knee (z 50 under it) -> foot (z 50 under it); ik foot under root
    let n = names(&["root", "hip", "knee", "foot", "ik"]);
    let parents = [-1, 0, 1, 2, 0];
    let skel = AnimNodeSkel { names: &n, parents: &parents, rest: None };
    let node_props = props(
        r#"{"LegsDefinition":[{"IKFootBone":{"BoneName":"ik"},"FKFootBone":{"BoneName":"foot"},"NumBonesInLimb":2}]}"#,
    );
    let mut node = anim_node_of("AnimNode_LegIK")(&node_props, &skel);
    let mut cs = CsPose::new(&n, &parents);
    let mut p = pose(&[[0.0; 3], [0.0, 0.0, 100.0], [0.0, 0.0, -50.0], [0.0, 0.0, -50.0], [30.0, 0.0, 20.0]]);
    // a knee bent a little forward so the solve has a plane
    p.t[2 * 3] = 0.01;
    node.update(1.0 / 60.0, 1.0);
    node.evaluate(&mut cs.begin(&mut p), &UeXf::default());
    let mut e = cs.begin(&mut p);
    assert!(close(&e.get(3).t, &[30.0, 0.0, 20.0], 1e-2), "{:?}", e.get(3).t);
    assert!(close(&e.get(1).t, &[0.0, 0.0, 100.0], 1e-4));
}

#[test]
fn a_constraint_takes_the_axes_it_has_on() {
    let n = names(&["root", "bone", "target"]);
    let parents = [-1, 0, 0];
    let skel = AnimNodeSkel { names: &n, parents: &parents, rest: None };
    let node_props = props(
        r#"{"BoneToModify":{"BoneName":"bone"},"ConstraintSetup":[{"TargetBone":{"BoneName":"target"},
        "OffsetOption":"EConstraintOffsetOption::None","TransformType":"ETransformConstraintType::Translation",
        "PerAxis":{"bX":true,"bY":false,"bZ":true}}],"ConstraintWeights":[1]}"#,
    );
    let mut node = anim_node_of("AnimNode_Constraint")(&node_props, &skel);
    let mut cs = CsPose::new(&n, &parents);
    let mut p = pose(&[[0.0; 3], [1.0, 2.0, 3.0], [10.0, 20.0, 30.0]]);
    node.update(0.0, 1.0);
    node.evaluate(&mut cs.begin(&mut p), &UeXf::default());
    assert!(close(&cs.begin(&mut p).get(1).t, &[10.0, 0.0, 30.0], 1e-4));
}

#[test]
fn copy_bone_copies_the_flagged_channels() {
    let n = names(&["root", "src", "dst"]);
    let parents = [-1, 0, 0];
    let skel = AnimNodeSkel { names: &n, parents: &parents, rest: None };
    let node_props = props(
        r#"{"SourceBone":{"BoneName":"src"},"TargetBone":{"BoneName":"dst"},"bCopyTranslation":true,
        "ControlSpace":"BCS_WorldSpace"}"#,
    );
    let mut node = anim_node_of("AnimNode_CopyBone")(&node_props, &skel);
    let mut cs = CsPose::new(&n, &parents);
    let mut p = pose(&[[0.0; 3], [4.0, 5.0, 6.0], [0.0; 3]]);
    node.update(0.0, 1.0);
    node.evaluate(&mut cs.begin(&mut p), &UeXf::default());
    assert!(close(&cs.begin(&mut p).get(2).t, &[4.0, 5.0, 6.0], 1e-4));
}

#[test]
fn a_driven_bone_follows_its_source_from_the_reference() {
    // C023B_AnimBP's node: fml_un_COM TranslationY x -2 -> fml_un_C_pelvis_att TranslationY, AddToInput
    let n = names(&["root", "com", "pelvis"]);
    let parents = [-1, 0, 0];
    let rest = pose(&[[0.0; 3], [0.0, 0.0, 90.0], [0.0, 0.0, 90.0]]);
    let skel = AnimNodeSkel { names: &n, parents: &parents, rest: Some(&rest) };
    let node_props = props(
        r#"{"SourceBone":{"BoneName":"com"},"TargetBone":{"BoneName":"pelvis"},
        "SourceComponent":"EComponentType::TranslationY","Multiplier":-2,
        "DestinationMode":"EDrivenDestinationMode::Bone","ModificationMode":"EDrivenBoneModificationMode::AddToInput",
        "bAffectTargetTranslationY":true}"#,
    );
    let mut node = anim_node_of("AnimNode_BoneDrivenController")(&node_props, &skel);
    let mut cs = CsPose::new(&n, &parents);
    let mut p = pose(&[[0.0; 3], [0.0, 5.0, 90.0], [0.0, 0.0, 90.0]]);
    node.update(0.0, 1.0);
    node.evaluate(&mut cs.begin(&mut p), &UeXf::default());
    assert!(close(&cs.begin(&mut p).get(2).t, &[0.0, -10.0, 90.0], 1e-4));
}

#[test]
#[should_panic(expected = "no anim node entered")]
fn an_unknown_node_panics() {
    anim_node_of("AnimNode_Unknown");
}

// ---- AnimPhys, AnimDynamics, Trail ----

#[test]
fn a_free_body_falls_one_step_of_gravity() {
    use super::anim_phys::{Body, Limits, physics_update};
    let dt = 1.0f32 / 60.0;
    let mut b = [Body::new([1.0; 3], [0.0; 3], [0.0, 0.0, 0.0, 1.0])];
    physics_update(&mut b, &mut Limits::default(), dt, [0.0, 0.0, -980.0], [0.0; 3], (4, 1));
    // the momentum damped (nothing yet) then gravity x dt; the position by the new momentum
    assert!((b[0].lin[2] - (-980.0 * dt)).abs() < 1e-4, "{:?}", b[0].lin);
    assert!((b[0].pos[2] - (-980.0 * dt * dt)).abs() < 1e-4, "{:?}", b[0].pos);
}

#[test]
fn an_undamped_spinning_cube_turns_by_its_angular_velocity() {
    use super::anim_phys::{Body, Limits, physics_update};
    let mut b = Body::new([1.0; 3], [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
    b.lin_damp = Some(0.0);
    b.ang_damp = Some(0.0);
    b.gravity_scale = 0.0;
    // a cube of half extent 1: I^-1 = 1 / (2 / 3) = 1.5 on each axis, so L 1 about Z turns at 1.5 rad/s
    b.ang = [0.0, 0.0, 1.0];
    let mut bodies = [b];
    physics_update(&mut bodies, &mut Limits::default(), 0.1, [0.0; 3], [0.0; 3], (1, 1));
    let h = 0.15f32 * 0.5;
    let q = bodies[0].q;
    assert!(close(&q.map(f64::from), &[0.0, 0.0, f64::from(h.sin()), f64::from(h.cos())], 1e-5), "{q:?}");
}

/// A chain root -> a -> b -> c (each 20 cm down UE Z) under one AnimDynamics node from a to c.
fn dynamics_chain(extra: &str) -> (Vec<String>, [i32; 4], Box<dyn AnimNode>) {
    let n = names(&["root", "a", "b", "c"]);
    let parents = [-1, 0, 1, 2];
    let json = format!(
        r#"{{"BoundBone":{{"BoneName":"a"}},"ChainEnd":{{"BoneName":"c"}},"bChain":true,"BoxExtents":{{"X":4,"Y":4,"Z":4}},
        "LocalJointOffset":{{"X":0,"Y":0,"Z":10}},"GravityScale":1,
        "SimulationSpace":"AnimPhysSimSpaceType::World",
        "ConstraintSetup":{{"LinearXLimitType":"AnimPhysLinearConstraintType::Limited",
        "LinearYLimitType":"AnimPhysLinearConstraintType::Limited","LinearZLimitType":"AnimPhysLinearConstraintType::Limited",
        "LinearAxesMin":{{"X":0,"Y":0,"Z":0}},"LinearAxesMax":{{"X":0,"Y":0,"Z":0}},
        "AngularConstraintType":"AnimPhysAngularConstraintType::Angular","TwistAxis":"AnimPhysTwistAxis::AxisX",
        "AngularTargetAxis":"AnimPhysTwistAxis::AxisX","AngularLimitsMin":{{"X":-45,"Y":-45,"Z":-45}},
        "AngularLimitsMax":{{"X":45,"Y":45,"Z":45}}}}{extra}}}"#
    );
    let node = {
        let skel = AnimNodeSkel { names: &n, parents: &parents, rest: None };
        anim_node_of("AnimNode_AnimDynamics")(&props(&json), &skel)
    };
    (n, parents, node)
}

#[test]
fn an_anim_dynamics_chain_at_rest_hangs_where_it_is() {
    let (n, parents, mut node) = dynamics_chain("");
    let mut cs = CsPose::new(&n, &parents);
    // a hanging chain, each body 10 cm below its bone: gravity pulls along it, the limits hold every joint
    let mut p = pose(&[[0.0; 3], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0]]);
    node.initialize();
    node.update(0.0, 1.0);
    for _ in 0..30 {
        node.update(1.0 / 60.0, 1.0);
        let mut q = pose(&[[0.0; 3], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0]]);
        node.evaluate(&mut cs.begin(&mut q), &UeXf::default());
        p.copy(&q);
    }
    let mut e = cs.begin(&mut p);
    for (i, z) in [(1, -20.0), (2, -40.0), (3, -60.0)] {
        assert!(close(&e.get(i).t, &[0.0, 0.0, z], 0.5), "bone {i}: {:?}", e.get(i).t);
    }
}

#[test]
fn an_anim_dynamics_chain_lags_its_root_and_keeps_its_joints() {
    let (n, parents, mut node) = dynamics_chain("");
    let mut cs = CsPose::new(&n, &parents);
    node.initialize();
    node.update(0.0, 1.0);
    let rest = [[0.0; 3], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0]];
    let mut p = pose(&rest);
    node.evaluate(&mut cs.begin(&mut p), &UeXf::default());
    // the root jumps 10 cm along X: the chain's end trails it
    let mut q = pose(&[[10.0, 0.0, 0.0], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0]]);
    node.update(1.0 / 60.0, 1.0);
    node.evaluate(&mut cs.begin(&mut q), &UeXf::default());
    let mut e = cs.begin(&mut q);
    let (a, c) = (e.get(1).t, e.get(3).t);
    assert!(c[0] < 10.0 - 1e-3, "the end follows at once: {c:?}");
    // the first joint is pulled toward the bound bone's new place by its locked linear limit (through the velocity,
    // not at once), and the end trails it
    assert!(a[0] > 1.0 && a[0] < 10.0 && c[0] < a[0], "{a:?} {c:?}");
}

#[test]
fn a_trail_at_rest_keeps_the_pose_and_lags_a_move() {
    let n = names(&["root", "a", "b", "c"]);
    let parents = [-1, 0, 1, 2];
    let skel = AnimNodeSkel { names: &n, parents: &parents, rest: None };
    let mut node = anim_node_of("AnimNode_Trail")(
        &props(
            r#"{"TrailBone":{"BoneName":"c"},"ChainLength":3,"ChainBoneAxis":"EAxis::Z","bInvertChainBoneAxis":true}"#,
        ),
        &skel,
    );
    let mut cs = CsPose::new(&n, &parents);
    let at = |x: f64| pose(&[[x, 0.0, 0.0], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0], [0.0, 0.0, -20.0]]);
    node.initialize();
    for _ in 0..5 {
        let mut p = at(0.0);
        node.update(1.0 / 60.0, 1.0);
        node.evaluate(&mut cs.begin(&mut p), &UeXf::default());
        let mut e = cs.begin(&mut p);
        assert!(close(&e.get(3).t, &[0.0, 0.0, -60.0], 1e-3), "{:?}", e.get(3).t);
    }
    let mut p = at(10.0);
    node.update(1.0 / 60.0, 1.0);
    node.evaluate(&mut cs.begin(&mut p), &UeXf::default());
    let mut e = cs.begin(&mut p);
    // the chain's start follows the root; the end trails it and keeps about its bone length
    assert!(close(&e.get(1).t, &[10.0, 0.0, -20.0], 1e-3));
    let (b, c) = (e.get(2).t, e.get(3).t);
    assert!(c[0] < 10.0 - 1e-3, "{c:?}");
    // the Trail keeps no length without bLimitStretch: about the bone length
    let len = ((c[0] - b[0]).powi(2) + (c[1] - b[1]).powi(2) + (c[2] - b[2]).powi(2)).sqrt();
    assert!((len - 20.0).abs() < 0.1, "{len}");
}
