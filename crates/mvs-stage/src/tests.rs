use super::*;
use mvs_data::StageData;

/// A base stage with the fields a stage data needs and one platform, its own floor and edges.
fn base() -> StageData {
    serde_json::from_value(serde_json::json!({
        "name": "Map_Base", "units": "cm", "meshes": [], "materials": {},
        "spawns": [], "respawns": [], "waypoints": [], "lights": [],
        "floor": { "center": [0, 0, -1000], "halfExtent": [500, 2000, 1000], "top": 0, "profile": "Floor",
            "tags": ["TraceEdges"] },
        "platforms": [{ "name": "P", "center": [0, 0, 600], "halfExtent": [300, 400, 10], "top": 610,
            "y": [-400, 400], "z": [590, 610] }],
        "edges": [{ "owner": "Floor2", "cls": "StaticMeshActor", "loc": [0, 0, 0], "every": false, "tag": "None",
            "points": [[1, 1]] }],
        "queryBodies": [{ "name": "Q", "center": [0, 0, 0], "halfExtent": [1, 1, 1], "profile": "Custom",
            "objectType": "Portal" }],
        "postProcess": [],
        "skyLight": { "cubemap": "", "intensity": 1, "color": [1, 1, 1], "lowerHemisphere": [0, 0, 0] },
        "blastBoxes": {}, "camera": { "CameraFOV": 26 }, "cameraCenter": [9000, 0, 1000],
        "blastBoxCorners": { "topLeft": [0, 0], "bottomRight": [0, 0] }
    }))
    .expect("the base")
}

/// Every start the team styles need, at z 200.
fn starts(prefix: &str) -> serde_json::Value {
    let list: Vec<serde_json::Value> = (1..=4)
        .map(|k| {
            serde_json::json!({ "tag": format!("{prefix}{k}"), "tags": ["1v1", "2v2", "FreeForAll"],
                "loc": [0, 100 * k, 200], "yaw": -90 })
        })
        .collect();
    serde_json::Value::Array(list)
}

/// A layout over the base: one slab copied from the base's instance `Slab` at scale (1, 2, 1) drawing the platform
/// `S`, whose box is the slab's top 20 cm; a parked floor; the blast box.
fn layout() -> serde_json::Value {
    serde_json::json!({
        "key": "test", "base": "base", "hide": ["Slab"],
        "pieces": [{ "instance": "Slab", "loc": [-450, 100, 1000], "scale": [1, 2, 1], "light": "baked",
            "platform": "S" }],
        "floor": { "parked": true },
        "platforms": [{ "name": "S", "box": { "piece": "S.StaticMesh", "thickness": 20 } }],
        "blast": { "CollisionComp": { "center": [0, 0, 880], "halfExtent": [1000, 5000, 2400],
            "y": [-5000, 5000], "z": [-1520, 3280] } },
        "blastBoxCorners": { "topLeft": [4970, 3030], "bottomRight": [-4970, -1520] },
        "spawns": starts("Player"), "respawns": starts("Respawn"),
        "waypoints": [{ "name": "W", "loc": [0, 0, -3000], "position": 0, "edge": false, "recovery": 0 }],
        "items": [{ "origin": [0, 0, 1200], "extent": [100, 2000, 500] }]
    })
}

fn read(v: serde_json::Value) -> Layout {
    Layout::read(&v.to_string()).expect("the layout")
}

/// The slab's mesh: 280 deep, 438 across, its top 3.5 over its origin.
fn bounds(_: &Piece) -> Option<MeshBounds> {
    Some(MeshBounds { origin: [0.0, 0.0, -151.5], extent: [280.0, 438.0, 155.0] })
}

#[test]
fn composes_the_map() {
    let l = read(layout());
    let c = compose_stage(&base(), &[], &l, &bounds).expect("composed");
    let d = &c.data;
    assert_eq!(d.name, "Custom_test");
    // the parked floor: 25000 under the lowest waypoint
    assert_eq!(d.floor.top, -3000.0 - PARK_DEPTH + PARK_HALF);
    assert_eq!(d.floor.profile.as_deref(), Some("Floor"));
    let p = &d.platforms.as_ref().expect("platforms")[0];
    assert_eq!(p.name, "S");
    // top: 1000 - 151.5 + 155; across: 100 +- 2 x 438
    assert_eq!(p.top, 1003.5);
    assert_eq!(p.y, [100.0 - 876.0, 100.0 + 876.0]);
    assert_eq!(p.z, [983.5, 1003.5]);
    let edges = d.edges.as_ref().expect("edges");
    assert_eq!(edges.len(), 1, "the base's floor edges gone, the platform's own");
    assert_eq!(edges[0].platform, Some(0));
    assert!(d.query_bodies.is_none());
    assert_eq!(d.blast_boxes.len(), 1);
    assert_eq!(c.spawn_areas.len(), 1);
    assert_eq!(c.spawn_areas[0].big_object_x, [0.0]);
}

#[test]
fn a_rotated_piece_s_box_holds_its_corners() {
    let p = Piece {
        stage: None,
        instance: "X".into(),
        name: None,
        loc: [0.0, 0.0, 0.0],
        rot: [0.0, 90.0, 0.0],
        scale: [1.0, 2.0, 1.0],
        light: Light::Movable,
        floating: false,
        platform: None,
    };
    let b = piece_box(&p, MeshBounds { origin: [0.0; 3], extent: [10.0, 20.0, 5.0] });
    // yaw 90 turns local Y (40 across, scaled) onto world X
    assert!((b.half[0] - 40.0).abs() < 1e-9 && (b.half[1] - 10.0).abs() < 1e-9, "{b:?}");
}

#[test]
fn refuses_more_than_32_platforms() {
    let mut v = layout();
    let list: Vec<serde_json::Value> = (0..33)
        .map(
            |i| serde_json::json!({ "name": format!("P{i}"), "box": { "center": [0, 0, 0], "halfExtent": [1, 1, 1] } }),
        )
        .collect();
    v["platforms"] = serde_json::Value::Array(list);
    v["pieces"] = serde_json::json!([]);
    assert_eq!(check(&read(v)), Err(LayoutError::TooManyPlatforms(33)));
}

#[test]
fn refuses_a_missing_start() {
    let mut v = layout();
    v["respawns"].as_array_mut().unwrap().retain(|s| s["tag"] != "Respawn3");
    assert_eq!(check(&read(v)), Err(LayoutError::NoStart { tag: "Respawn3".into(), style: "2v2" }));
}

#[test]
fn refuses_a_missing_blast_box() {
    let mut v = layout();
    v["blast"] = serde_json::json!({ "CollisionCompWarningBox": v["blast"]["CollisionComp"].clone() });
    assert_eq!(check(&read(v)), Err(LayoutError::NoBlastBox));
}

#[test]
fn refuses_a_baked_piece_of_another_stage() {
    let mut v = layout();
    v["pieces"][0]["stage"] = serde_json::json!("m008");
    let e = check(&read(v));
    assert_eq!(e, Err(LayoutError::BakedFromOtherStage { piece: "S.StaticMesh".into(), stage: "m008".into() }));
}

#[test]
fn refuses_unknown_fields() {
    let mut v = layout();
    v["rise"] = serde_json::json!(1);
    assert!(matches!(Layout::read(&v.to_string()), Err(LayoutError::Json(_))));
}

#[test]
fn the_base_s_look_stays() {
    let mut data_stage = base();
    data_stage.name = "Map_Base".into();
    let l = read(layout());
    let c = compose_stage(&data_stage, &[], &l, &bounds).expect("composed");
    assert_eq!(c.data.sky_light.intensity, 1.0, "the base's look stays");
}

/// The team styles here are the sim's.
#[test]
fn start_styles_are_the_sims() {
    let tags: Vec<&str> = START_STYLES.iter().map(|s| s.0).collect();
    assert_eq!(tags, mvs_sim::setup::START_TAGS);
}

/// The custom stages' name prefix is the sim's.
#[test]
fn the_prefix_is_the_sims() {
    assert_eq!(CUSTOM_PREFIX, mvs_sim::stages::statics::CUSTOM_PREFIX);
}

/// The layout that ships reads and passes the checks.
#[test]
fn the_shipped_layout_checks() {
    let l = Layout::read(include_str!("../layouts/rising.json")).expect("rising");
    check(&l).expect("rising");
    assert_eq!(l.key, "rising");
}

/// A new map over a base takes its blast box, starts, waypoints and item area, parks the floor and composes.
#[test]
fn a_starter_over_the_base_composes() {
    let mut b = base();
    let full = read(layout());
    b.blast_boxes = full.blast.clone();
    b.spawns = full.spawns.clone();
    b.respawns = full.respawns.clone();
    b.waypoints = full.waypoints.clone();
    let area = mvs_data::SpawnArea {
        level: "L".into(),
        actor: "A".into(),
        origin: vec![1.0, 2.0, 3.0],
        extent: vec![4.0, 5.0, 6.0],
        root: vec![1.0, 2.0, 3.0],
        big_object_x: vec![0.0],
    };
    let l = starter(&b, std::slice::from_ref(&area), "new", "base");
    assert_eq!((l.key.as_str(), l.base.as_str(), l.floor.parked), ("new", "base", true));
    assert!(l.pieces.is_empty() && l.platforms.is_empty() && l.hide.is_empty());
    assert_eq!(l.items[0].origin, [1.0, 2.0, 3.0]);
    let c = compose_stage(&b, &[area], &l, &bounds).expect("composed");
    assert!(c.data.platforms.is_none());
    assert_eq!(c.data.spawns.len(), 4);
}
