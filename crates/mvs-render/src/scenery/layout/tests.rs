use super::*;

fn instance(name: &str, mesh: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "mesh": mesh, "loc": [0, 0, 0], "rot": [0, 0, 0], "scale": [1, 1, 1],
        "materials": ["/Game/M.M?ctx=shadowmap", "/Game/T.T?ctx=local"],
        "lm": { "tex": 0, "st": [1, 1, 0, 0], "scale": [[1, 1, 1, 1], [1, 1, 1, 1]], "add": [[0, 0, 0, 0], [0, 0, 0, 0]] },
        "channels": [true, false, false], "castShadow": true, "parallax": 1 })
}

/// A base scenery: four instances (the third a decal receiver, the fourth attached to the skeletal `Arm`), one
/// block naming `Ship`, one emitter.
fn base() -> Scenery {
    let mut arm = instance("Donut", "slab");
    arm["attach"] = serde_json::json!(["Arm", "bone"]);
    serde_json::from_value(serde_json::json!({
        "lightmaps": ["lm"], "meshes": { "slab": { "path": "/Game/Slab" }, "ship": { "path": "/Game/Ship" } },
        "instances": [instance("Main", "slab"), instance("Slab", "slab"), instance("Ship", "ship"), arm],
        "skeletal": [{ "name": "Arm", "mesh": "/Game/Arm", "loc": [0, 0, 0], "rot": [0, 0, 0], "scale": [1, 1, 1],
            "materials": [], "channels": [true, false, false] }],
        "emitters": [{ "name": "Fire", "system": "PS", "loc": [0, 0, 0], "rot": [0, 0, 0], "scale": [1, 1, 1] }],
        "decals": [{ "name": "D", "material": "m", "loc": [0, 0, 0], "rot": [0, 0, 0], "half": [1, 1, 1] }],
        "decalReceivers": [2],
        "ship": { "actor": "Ship" }, "arm": { "skeletal": "Arm" }
    }))
    .expect("the scenery")
}

fn layout(extra: serde_json::Value) -> Layout {
    let mut v = serde_json::json!({
        "key": "t", "base": "b", "blocks": ["ship"], "hide": ["Main", "Slab"],
        "pieces": [{ "instance": "Slab", "loc": [0, 100, 50], "scale": [1, 2, 1], "light": "baked", "platform": "S" }],
        "floor": { "parked": true },
        "platforms": [{ "name": "S", "box": { "piece": "S.StaticMesh", "thickness": 20 } }],
        "blast": { "CollisionComp": { "center": [0, 0, 0], "halfExtent": [1, 1, 1], "y": [-1, 1], "z": [-1, 1] } },
        "blastBoxCorners": { "topLeft": [1, 1], "bottomRight": [-1, -1] },
        "spawns": starts("Player"), "respawns": starts("Respawn")
    });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    Layout::read(&v.to_string()).expect("the layout")
}

fn starts(prefix: &str) -> serde_json::Value {
    (1..=4)
        .map(|k| {
            serde_json::json!({ "tag": format!("{prefix}{k}"), "tags": ["1v1", "2v2", "FreeForAll"],
                "loc": [0, 0, 0], "yaw": 0 })
        })
        .collect()
}

fn no_source(key: &str) -> std::result::Result<Scenery, LayoutError> {
    Err(LayoutError::Invalid(format!("no stage {key}")))
}

fn all_draw(_: &str) -> std::result::Result<bool, LayoutError> {
    Ok(true)
}

#[test]
fn hides_remap_the_receivers_and_a_platform_piece_is_added() {
    let mut s = base();
    let l = layout(serde_json::json!({ "hide": ["Main", "Slab", "Arm", "Donut", "Fire"] }));
    apply(&mut s, &l, &mut no_source, &mut all_draw).expect("applied");
    let names: Vec<&str> = s.instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["Ship", "S.StaticMesh"]);
    // the receiver was third, now first
    assert_eq!(s.decal_receivers, [0]);
    assert!(s.skeletal.is_empty() && s.emitters.is_empty());
    let p = &s.instances[1];
    assert_eq!(p.platform, Some(0));
    assert_eq!(p.loc, [0.0, 100.0, 50.0]);
    assert_eq!(p.scale, [1.0, 2.0, 1.0]);
    assert!(p.lm.is_some(), "a baked piece keeps its lightmap");
    assert!(p.fields.contains_key("castShadow") && !p.fields.contains_key("parallax"));
    // the block not kept is stripped
    assert!(s.rest.contains_key("ship") && !s.rest.contains_key("arm"));
}

#[test]
fn a_movable_piece_drops_its_lightmap() {
    let mut s = base();
    let l = layout(serde_json::json!({ "hide": ["Main", "Slab", "Arm", "Donut"],
        "pieces": [{ "instance": "Slab", "loc": [0, 0, 0], "light": "movable", "platform": "S", "floating": true }] }));
    apply(&mut s, &l, &mut no_source, &mut all_draw).expect("applied");
    let p = s.instances.last().expect("the piece");
    assert!(p.lm.is_none());
    assert_eq!(p.materials, [Some("/Game/M.M?ctx=locallm".to_owned()), Some("/Game/T.T?ctx=local".to_owned())]);
    assert_eq!(p.fields.get("floating"), Some(&serde_json::Value::Bool(true)));
}

#[test]
fn refuses_a_movable_piece_whose_material_lacks_the_pass() {
    // (the base's second material lacks the movable base pass)
    let mut lacks_t = |path: &str| Ok(path != "/Game/T.T");
    let pieces = serde_json::json!({ "hide": ["Main", "Slab", "Arm", "Donut"],
        "pieces": [{ "instance": "Slab", "loc": [0, 0, 0], "light": "movable", "platform": "S" }] });
    let e = apply(&mut base(), &layout(pieces), &mut no_source, &mut lacks_t);
    let want = LayoutError::NoMovablePass { piece: "S.StaticMesh".into(), material: "/Game/T.T".into() };
    assert_eq!(e, Err(want));
    // a base piece moved in place, made movable
    let moved = serde_json::json!({ "hide": ["Main", "Arm", "Donut"], "platforms": [],
        "pieces": [{ "instance": "Slab", "loc": [0, 0, 0], "light": "movable" }] });
    let e = apply(&mut base(), &layout(moved), &mut no_source, &mut lacks_t);
    assert!(matches!(e, Err(LayoutError::NoMovablePass { .. })), "{e:?}");
    // the same pieces baked are not asked about
    let baked = serde_json::json!({ "hide": ["Main", "Slab", "Arm", "Donut"] });
    apply(&mut base(), &layout(baked), &mut no_source, &mut lacks_t).expect("a baked piece");
}

#[test]
fn refuses_a_hide_a_kept_block_names() {
    let mut s = base();
    let l = layout(serde_json::json!({ "hide": ["Ship", "Arm", "Donut"] }));
    let e = apply(&mut s, &l, &mut no_source, &mut all_draw);
    assert_eq!(e, Err(LayoutError::HiddenInBlock { instance: "Ship".into(), block: "ship".into() }));
}

#[test]
fn refuses_a_hidden_skeletal_mesh_with_an_instance_on_it() {
    let mut s = base();
    let l = layout(serde_json::json!({ "hide": ["Arm"] }));
    assert!(matches!(apply(&mut s, &l, &mut no_source, &mut all_draw), Err(LayoutError::Invalid(_))));
}

#[test]
fn refuses_an_unknown_hide_and_block() {
    let mut s = base();
    let l = layout(serde_json::json!({ "hide": ["Nope"] }));
    assert!(matches!(apply(&mut s, &l, &mut no_source, &mut all_draw), Err(LayoutError::Invalid(_))));
    let mut s = base();
    let l = layout(serde_json::json!({ "blocks": ["nope"], "hide": [] }));
    assert!(matches!(apply(&mut s, &l, &mut no_source, &mut all_draw), Err(LayoutError::Invalid(_))));
}

#[test]
fn a_borrowed_piece_takes_its_stages_mesh() {
    let mut s = base();
    let l = layout(serde_json::json!({ "hide": [], "platforms": [],
        "pieces": [{ "stage": "other", "instance": "Rock", "name": "Rock", "loc": [0, 0, 0], "light": "movable" }] }));
    let mut other = |key: &str| {
        assert_eq!(key, "other");
        let mut o = base();
        o.meshes.insert("slab".into(), super::super::SceneryMesh { path: "/Game/OtherSlab".into() });
        o.instances.push(serde_json::from_value(instance("Rock", "slab")).expect("rock"));
        Ok(o)
    };
    apply(&mut s, &l, &mut other, &mut all_draw).expect("applied");
    let p = s.instances.last().expect("the piece");
    // the base's `slab` is another mesh: the key is renamed
    assert_eq!(p.mesh, "other:slab");
    assert_eq!(s.meshes["other:slab"].path, "/Game/OtherSlab");
    assert!(p.lm.is_none());
}

fn store() -> Option<mvs_store::Store> {
    match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) => Some(s),
        Err(_) => {
            eprintln!("no dev store: skipped");
            None
        }
    }
}

/// The developer's store: the shipped layout over its base applies, and its pieces' mesh bounds read. Prints which
/// static mesh materials of every scenery lack the movable base pass (a piece made movable with one does not draw).
#[test]
fn shipped_layout_over_the_store() {
    let Some(store) = store() else { return };
    let l = Layout::read(include_str!("../../../../mvs-stage/layouts/rising.json")).expect("rising");
    let mut s = read_scenery(&store, &l.base).expect("the base's scenery");
    apply_from_store(&mut s, &l, &store).expect("applied");
    assert_eq!(s.instances.iter().filter(|i| i.platform.is_some()).count(), l.platforms.len());
    let b = PieceBounds::read(&store, &l).expect("bounds");
    assert!(l.pieces.iter().all(|p| b.get(&l, p).is_some()));

    let pack = store.kind("material").expect("the material kind");
    let scenery = store.kind("scenery").expect("the scenery kind");
    let (mut seen, mut lacking) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    let mut blends: Vec<(String, String)> = Vec::new();
    for entry in scenery.paths() {
        let Ok(bytes) = scenery.bytes(entry) else { continue };
        let Ok(h) = JsonBin::split(b"MVSC", &bytes, entry).and_then(|j| j.parse::<SceneryHeader>(entry)) else {
            continue;
        };
        for m in h.scenery.instances.iter().flat_map(|i| i.materials.iter().flatten()) {
            let path = m.split_once('?').map_or(m.as_str(), |x| x.0).to_owned();
            if !seen.insert(path.clone()) {
                continue;
            }
            // (a translucent material draws in the translucent pass, not this one)
            if let Ok(mvs_material::Entry::Material(mat)) = mvs_material::read_entry(&pack, &path)
                && matches!(mat.state.blend.as_str(), "BLEND_Opaque" | "BLEND_Masked")
            {
                if !has_movable_base_pass(&mat) {
                    lacking.insert(path.clone());
                }
                blends.push((path, mat.state.blend.clone()));
            }
        }
    }
    let piece_mats: std::collections::BTreeSet<String> = s
        .instances
        .iter()
        .filter(|i| i.platform.is_some())
        .flat_map(|i| i.materials.iter().flatten())
        .map(|m| m.split_once('?').map_or(m.as_str(), |x| x.0).to_owned())
        .collect();
    for m in &piece_mats {
        let blend = blends.iter().find(|b| &b.0 == m).map_or("translucent", |b| b.1.as_str());
        eprintln!(
            "piece material {m} ({blend}): movable base pass {}",
            if lacking.contains(m) { "MISSING" } else { "held" }
        );
    }
    eprintln!(
        "{} of {} opaque or masked scenery static mesh materials ({} in all) lack the movable base pass",
        lacking.len(),
        blends.len(),
        seen.len()
    );
}

/// The developer's store: a Shaggy 1v1 on six_slabs.json (the shipped map's first form). Both fighters land on the
/// ground slab and stand there; the lift's box rises 600 over its first move (after BasePlatformBP's 2 s delay) and
/// comes back; no stage warning.
#[test]
fn a_match_plays_on_a_layout() {
    use mvs_sim::input::InputState;
    use mvs_sim::setup::{MatchPlayer, MatchSetup};
    let Some(store) = store() else { return };
    let l = Layout::read(include_str!("../../../../mvs-stage/fixtures/six_slabs.json")).expect("rising");
    let mut data = mvs_data::GameData::load(&store, &["shaggy"], &l.base).expect("the gamedata kind loads");
    let b = PieceBounds::read(&store, &l).expect("bounds");
    mvs_stage::apply(&mut data, &l, &|p| b.get(&l, p)).expect("composed");
    assert_eq!(data.stage.name, "Custom_rising");
    let player = MatchPlayer { character: Some("shaggy".into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player.clone(), player], flow: true, ..MatchSetup::default() };
    let mut w = mvs_sim::World::new(std::sync::Arc::new(data), 1, &setup).expect("binds");
    let lift = l.platform_index("Lift").expect("the lift") as usize;
    let top0 = f64::from(w.stage.stage_platforms[lift].top);
    let mut high = f64::MIN;
    for _ in 0..480 {
        w.step(&[InputState::default(), InputState::default()]);
        high = high.max(f64::from(w.stage.stage_platforms[lift].top));
    }
    let ground = &w.data.stage.platforms.as_ref().expect("platforms")[0];
    for id in w.players.clone() {
        let body = &w.fighter(id).body;
        assert!(body.on_floor, "fighter {id:?} stands");
        // (the body's place is its capsule's centre, over the slab's top)
        let over = f64::from(body.y) - ground.top;
        assert!((0.0..300.0).contains(&over), "fighter {id:?} at {} over the ground {}", body.y, ground.top);
    }
    assert!((high - top0 - 600.0).abs() < 1.0, "the lift rose from {top0} to {high}");
    let back = f64::from(w.stage.stage_platforms[lift].top);
    assert!((back - top0).abs() < 1.0, "the lift came back to {top0}: {back}");
    let warned = w.warned.borrow();
    assert!(!warned.iter().any(|m| m.contains("stage")), "{warned:?}");
}

/// six_slabs.json with its Ground made to rise (the port's own motion): the slab climbs its speed each tick from
/// the first, wraps to its bottom past its top, and the fighters standing on it are carried up past the outer
/// blast box's top (the ring-out); nobody rides it down. Needs the dev store.
#[test]
fn a_rising_ground_carries_the_fighters_out() {
    use mvs_sim::input::InputState;
    use mvs_sim::setup::{MatchPlayer, MatchSetup};
    let Some(store) = store() else { return };
    let mut l = Layout::read(include_str!("../../../../mvs-stage/fixtures/six_slabs.json")).expect("rising");
    let (bottom, top, speed) = (-1800.0, 3700.0, 600.0);
    l.platforms[0].rise = Some(mvs_data::stage::PlatformRise { bottom, top, speed });
    let mut data = mvs_data::GameData::load(&store, &["shaggy"], &l.base).expect("the gamedata kind loads");
    let b = PieceBounds::read(&store, &l).expect("bounds");
    mvs_stage::apply(&mut data, &l, &|p| b.get(&l, p)).expect("composed");
    let blast_top = data.stage.blast_boxes.get("CollisionComp").expect("the blast box").z[1];
    let player = MatchPlayer { character: Some("shaggy".into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player.clone(), player], flow: true, ..MatchSetup::default() };
    let mut w = mvs_sim::World::new(std::sync::Arc::new(data), 1, &setup).expect("binds");
    let half = (w.data.stage.platforms.as_ref().expect("platforms")[0].z[1]
        - w.data.stage.platforms.as_ref().expect("platforms")[0].z[0])
        / 2.0;
    let mut last = f64::from(w.stage.stage_platforms[0].top);
    let mut wraps = 0;
    let mut high = vec![f64::MIN; w.players.len()];
    for _ in 0..700 {
        w.step(&[InputState::default(), InputState::default()]);
        let t = f64::from(w.stage.stage_platforms[0].top);
        if t < last {
            wraps += 1;
            assert!((t - half - (bottom + (last - half + speed / 60.0 - top))).abs() < 1.0, "wrapped to {t}");
        } else {
            assert!((t - last - speed / 60.0).abs() < 1e-2, "climbed {last} -> {t}");
        }
        last = t;
        for (k, id) in w.players.clone().into_iter().enumerate() {
            high[k] = high[k].max(f64::from(w.fighter(id).body.y));
        }
    }
    assert_eq!(wraps, 1, "one wrap in 700 ticks");
    for (k, h) in high.iter().enumerate() {
        assert!(*h > blast_top, "fighter {k} carried to {h}, under the blast box's top {blast_top}");
    }
    let warned = w.warned.borrow();
    assert!(!warned.iter().any(|m| m.contains("stage") || m.contains("platform")), "{warned:?}");
}
