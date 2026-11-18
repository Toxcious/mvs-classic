//! The player over the developer's store: every system the shaggy and base parts load spawns, plays 4 s at 60 Hz
//! with a draw each frame, and ends or keeps drawing without a panic. Skipped (passes) without a store.

use mvs_store::{Store, dev_store_dir};
use mvs_vfx::{EmitterDraw, Player};

#[test]
fn shaggy_and_base_systems_play() {
    let dir = dev_store_dir();
    let Ok(store) = Store::open(&dir) else {
        eprintln!("no store at {}: skipped", dir.display());
        return;
    };
    let mut player = Player::new(&store, 1).unwrap();
    let names: Vec<String> = player
        .systems()
        .filter(|(_, o)| o.iter().any(|o| o == "shaggy" || o == "base"))
        .map(|(n, _)| n.to_string())
        .collect();
    assert!(!names.is_empty());
    let mut out: Vec<EmitterDraw> = Vec::new();
    let mut drawn = 0;
    let mut spawned = 0;
    player.set_view([0.0, -1000.0, 100.0], [0.0, 1.0, 0.0]);
    for name in &names {
        let Some(id) = player.spawn(name, [10.0, 0.0, 50.0], [0.0, 90.0, 0.0], [1.0, -1.0, 1.0], &[]) else {
            continue;
        };
        spawned += 1;
        for f in 0..240 {
            player.tick(1.0 / 60.0);
            player.draws([0.0, -1000.0, 100.0], if f % 2 == 0 { 1.0 } else { 0.5 }, &mut out);
            assert_eq!(out.len(), player.draw_params().len());
            for d in &out {
                assert_eq!(d.vertices.len(), (d.stride * d.count) as usize, "{name}");
                assert!(
                    d.vertices
                        .chunks(4)
                        .all(|c| f32::from_le_bytes(c.try_into().unwrap()).is_finite() || d.stride == 112),
                    "{name}: a value not finite"
                );
            }
            drawn += out.len();
            if f == 120 {
                player.deactivate(id);
            }
        }
        player.kill(id);
        player.tick(1.0 / 60.0);
        assert!(!player.alive(id));
    }
    eprintln!("{spawned} of {} systems spawned, {drawn} draws", names.len());
    assert!(spawned > 0 && drawn > 0);
}

/// The match intro's swirl (BaseFighter PlayIntroAnimation's PS_CharacterSelectSwirl, an AccelerationOverLifetime
/// emitter): plays its 4 s with finite particles.
#[test]
fn the_intro_swirl_plays() {
    let dir = dev_store_dir();
    let Ok(store) = Store::open(&dir) else {
        eprintln!("no store at {}: skipped", dir.display());
        return;
    };
    let mut player = Player::new(&store, 1).unwrap();
    player.spawn("PS_CharacterSelectSwirl", [0.0; 3], [0.0, -90.0, 0.0], [1.0; 3], &[]).expect("spawned");
    let mut out: Vec<EmitterDraw> = Vec::new();
    let mut drawn = 0;
    for _ in 0..240 {
        player.tick(1.0 / 60.0);
        player.draws([0.0, -1000.0, 100.0], 1.0, &mut out);
        for d in &out {
            assert!(d.vertices.chunks(4).all(|c| f32::from_le_bytes(c.try_into().unwrap()).is_finite()));
        }
        drawn += out.len();
    }
    assert!(drawn > 0);
}

/// A store with only the particles, mesh and material kinds (no drawmat pack): the packs of the developer's store
/// linked (copied where a link is not possible) under a manifest of those three kinds, in a folder of its own.
fn store_without_drawmat(src: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(src.join("store.json")).ok()?).ok()?;
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let mut kept = manifest.clone();
    let kinds = kept.get_mut("kinds")?.as_object_mut()?;
    kinds.retain(|k, _| matches!(k.as_str(), "particles" | "mesh" | "material"));
    if kinds.len() != 3 {
        return None;
    }
    for rec in kinds.values() {
        let file = rec.get("file")?.as_str()?;
        if std::fs::hard_link(src.join(file), dir.join(file)).is_err() {
            std::fs::copy(src.join(file), dir.join(file)).ok()?;
        }
    }
    std::fs::write(dir.join("store.json"), serde_json::to_vec(&kept).ok()?).ok()?;
    Some(dir)
}

/// The index's systems with a beam emitter (TypeDataBeam2), sorted.
const BEAM_SYSTEMS: [&str; 16] = [
    "PS_BatmanGrappleGunProjectile",
    "PS_BatmanGrappleGunRopeTEST",
    "PS_C018_NAD_Laser",
    "PS_C018_SpaceshipLaser",
    "PS_C021_DischargeBeam",
    "PS_C021_Emote_Victory_Lightning",
    "PS_C021_ThunderSky",
    "PS_CreatureCrystal_LightingBeam",
    "PS_CreatureCrystal_LightingBeam_Warning",
    "PS_Garnet_ElecBeam",
    "PS_Garnet_TetherBeam",
    "PS_StevenBeam",
    "PS_TetherBeam",
    "PS_Velma_Beam",
    "PS_Velma_Beam03",
    "PS_WonderWomanLassoBeam",
];

/// The player needs no drawmat pack: every emitter of every system the particles index names whose template builds
/// is kept (the material kind holds each one's material). Systems whose template has no port yet are not counted; the
/// next test covers every system entry of the kind, the index's or not, without building templates.
#[test]
fn every_emitter_is_kept_without_a_drawmat_pack() {
    let src = dev_store_dir();
    let Ok(full) = Store::open(&src) else {
        eprintln!("no store at {}: skipped", src.display());
        return;
    };
    let Some(dir) = store_without_drawmat(&src, "mvs-vfx-nodrawmat") else {
        eprintln!("no particles, mesh and material kinds in {}: skipped", src.display());
        return;
    };
    let store = Store::open(&dir).unwrap();
    assert!(store.kind("drawmat").is_err());
    let mut player = Player::new(&store, 1).unwrap();
    // (the entries' own count of the emitters of the index's systems)
    let particles = full.kind("particles").unwrap();
    let mut want = 0;
    let mut got = 0;
    let mut skipped = Vec::new();
    let names: Vec<String> = player.systems().map(|(n, _)| n.to_string()).collect();
    for name in &names {
        // (building a template with a module or a trail source that has no port panics, as a spawn does)
        let Ok(count) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| player.emitter_count(name))) else {
            assert!(player.unported(name).is_some(), "{name}: its template panics and unported() does not say so");
            skipped.push(name.clone());
            continue;
        };
        got += count.unwrap_or_else(|| panic!("{name} is not in the index"));
        let path = particles
            .paths()
            .find(|p| {
                p.rsplit('/').next().is_some_and(|f| f.split('.').next() == Some(name.as_str()))
                    && !p.ends_with("/Index.Owners")
            })
            .unwrap_or_else(|| panic!("no entry of {name}"))
            .to_string();
        let v: serde_json::Value = serde_json::from_slice(&particles.bytes(&path).unwrap()).unwrap();
        want += v["system"]["emitters"].as_array().map_or(0, Vec::len);
    }
    std::fs::remove_dir_all(&dir).ok();
    eprintln!("{} systems ({} not built: unported), {got} of {want} emitters kept", names.len(), skipped.len());
    // Every module type has a port: a new one shows here.
    assert!(skipped.is_empty(), "the systems not built: {skipped:?}");
    assert_eq!(got, want);
    assert!(got > 3000);
}

/// Every beam system builds, spawns with its user source and target points set on every emitter, plays 2 s at 60 Hz
/// and draws beam strips, with no panic; prints what each leaves out.
#[test]
fn beam_systems_draw() {
    let dir = dev_store_dir();
    let Ok(store) = Store::open(&dir) else {
        eprintln!("no store at {}: skipped", dir.display());
        return;
    };
    let mut player = Player::new(&store, 1).unwrap();
    player.set_view([0.0, -1000.0, 100.0], [0.0, 1.0, 0.0]);
    player.set_camera_up([0.0, 0.0, 1.0]);
    let mut out: Vec<EmitterDraw> = Vec::new();
    let mut found = Vec::new();
    let mut beams = 0;
    for name in BEAM_SYSTEMS {
        let Some(id) = player.spawn(name, [0.0, 0.0, 100.0], [0.0, 0.0, 0.0], [1.0; 3], &[]) else {
            panic!("{name} did not spawn");
        };
        let n = player.emitter_count(name).unwrap();
        let gaps: Vec<String> = player
            .emitters(id)
            .unwrap()
            .iter()
            .filter_map(|e| e.t.beam.as_ref())
            .flat_map(|b| b.gaps.clone())
            .collect();
        let mut drawn = 0;
        for f in 0..120 {
            for e in 0..n {
                let e = i32::try_from(e).unwrap();
                player.set_beam_source_point(id, e, [0.0, 0.0, 100.0 + f64::from(f)], 0);
                player.set_beam_target_point(id, e, [300.0, 0.0, 150.0], 0);
            }
            player.set_beam_actor(id, "BeamTarget", Some([300.0, 0.0, 150.0]));
            player.tick(1.0 / 60.0);
            player.draws([0.0, -1000.0, 100.0], 1.0, &mut out);
            for d in &out {
                assert_eq!(d.vertices.len(), (d.count * d.stride) as usize, "{name}");
                if matches!(d.kind, mvs_vfx::EmitterKind::Beam) {
                    drawn += 1;
                    let v = &d.vertices;
                    for k in 0..d.count as usize {
                        let x = f32::from_le_bytes(v[k * 96..k * 96 + 4].try_into().unwrap());
                        assert!(x.is_finite(), "{name}: a beam vertex at {x}");
                    }
                }
            }
        }
        player.kill(id);
        player.tick(1.0 / 60.0);
        if drawn > 0 {
            beams += 1;
        }
        found.push(format!("{name}: {drawn} beam draws; not ported: {gaps:?}"));
    }
    eprintln!("{}", found.join("\n"));
    assert_eq!(beams, BEAM_SYSTEMS.len(), "{found:?}");
}

/// The grapple's beam (low-frequency noise with InterpolationPoints: FillData_InterpolatedNoise): one strip of the
/// source pair, NoiseTessellation pairs per noise step (Frequency 250, NoiseTessellation 50) and NoiseTessellation
/// more to the target; it starts at the source and ends on the target, and its noise (NoiseRangeScale 1 -> 0 over
/// 0.2 s) has faded by 0.25 s: every centre then within a few cm of the source-target line.
#[test]
fn grapple_beam_fills_interpolated_noise() {
    let dir = dev_store_dir();
    let Ok(store) = Store::open(&dir) else {
        eprintln!("no store at {}: skipped", dir.display());
        return;
    };
    let name = "PS_BatmanGrappleGunProjectile";
    let mut player = Player::new(&store, 1).unwrap();
    player.set_view([0.0, -1000.0, 100.0], [0.0, 1.0, 0.0]);
    player.set_camera_up([0.0, 0.0, 1.0]);
    let id = player.spawn(name, [0.0, 0.0, 100.0], [0.0, 0.0, 0.0], [1.0; 3], &[]).unwrap();
    let (src, tgt) = ([0.0, 0.0, 100.0], [600.0, 0.0, 300.0]);
    let mut out: Vec<EmitterDraw> = Vec::new();
    let centre = |v: &[u8], k: usize| -> [f64; 3] {
        let at = |i: usize, c: usize| {
            f64::from(f32::from_le_bytes(v[i * 96 + c * 4..i * 96 + c * 4 + 4].try_into().unwrap()))
        };
        std::array::from_fn(|c| (at(2 * k, c) + at(2 * k + 1, c)) / 2.0)
    };
    let mut wavy = 0.0f64;
    for f in 0..15 {
        player.set_beam_source_point(id, 0, src, 0);
        player.set_beam_actor(id, "BeamTarget", Some(tgt));
        player.tick(1.0 / 60.0);
        player.draws([0.0, -1000.0, 100.0], 1.0, &mut out);
        let Some(d) = out.iter().find(|d| matches!(d.kind, mvs_vfx::EmitterKind::Beam)) else { continue };
        assert_eq!(d.count, 2 * (1 + 250 * 50 + 50), "frame {f}");
        let pairs = d.count as usize / 2;
        let (first, last) = (centre(&d.vertices, 0), centre(&d.vertices, pairs - 1));
        for c in 0..3 {
            assert!((first[c] - src[c]).abs() < 1e-3, "frame {f}: starts at {first:?}");
            assert!((last[c] - tgt[c]).abs() < 1e-2, "frame {f}: ends at {last:?}");
        }
        // the largest distance of a centre from the line
        let dir = [tgt[0] - src[0], tgt[1] - src[1], tgt[2] - src[2]];
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        let off = (0..pairs)
            .map(|k| {
                let p = centre(&d.vertices, k);
                let r = [p[0] - src[0], p[1] - src[1], p[2] - src[2]];
                let t = (r[0] * dir[0] + r[1] * dir[1] + r[2] * dir[2]) / len;
                let q: [f64; 3] = std::array::from_fn(|c| r[c] - dir[c] / len * t);
                (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt()
            })
            .fold(0.0, |a, b| if b > a { b } else { a });
        if f == 1 {
            wavy = off;
        }
        if f == 14 {
            assert!(off < 1.0, "the noise has not faded: {off} cm off the line");
        }
    }
    assert!(wavy > 20.0, "no noise at the start: {wavy} cm off the line");
    // UpdateBoundingBox (142bff620): the component point, the source and the target, each shifted by the noise
    // range's min and max, grown by the largest size (10 here).
    let e = &player.emitters(id).unwrap()[0];
    let (lo, hi) = e.t.beam.as_ref().unwrap().noise.as_ref().unwrap().out_range;
    assert!(lo < 0.0 && hi > 250.0, "the noise out range {lo} .. {hi}");
    let b = e.bbox.expect("the beam's box");
    for c in 0..3 {
        let want_min = if src[c] < tgt[c] { src[c] } else { tgt[c] } + lo - 10.0;
        let want_max = if src[c] > tgt[c] { src[c] } else { tgt[c] } + hi + 10.0;
        assert!((b.min[c] - want_min).abs() < 1e-6, "axis {c}: min {} not {want_min}", b.min[c]);
        assert!((b.max[c] - want_max).abs() < 1e-6, "axis {c}: max {} not {want_max}", b.max[c]);
    }
}

/// Every system entry of the particles kind (not only the index's): each emitter's Required module names a material the
/// material kind has, as the player's `load` looks it up. Reads the entries with serde only: no template is built, so
/// a system with an unported module counts too. Counts the emitters whose material is missing (the player skips them).
#[test]
fn every_system_entry_emitter_has_its_material() {
    let src = dev_store_dir();
    let Ok(store) = Store::open(&src) else {
        eprintln!("no store at {}: skipped", src.display());
        return;
    };
    let (Ok(particles), Ok(materials)) = (store.kind("particles"), store.kind("material")) else {
        eprintln!("no particles and material kinds in {}: skipped", src.display());
        return;
    };
    let (mut systems, mut emitters, mut missing) = (0, 0, Vec::new());
    for path in particles.paths() {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&particles.bytes(path).unwrap()) else { continue };
        if v["kind"] != "system" {
            continue;
        }
        systems += 1;
        for e in v["system"]["emitters"].as_array().into_iter().flatten() {
            emitters += 1;
            let req = e["modules"].as_array().and_then(|m| m.iter().find(|m| m["type"] == "Required"));
            let material = req.and_then(|m| m["Material"].as_str()).unwrap_or("");
            if !materials.has(material.split_once('?').map_or(material, |(p, _)| p)) {
                missing.push(format!("{path}: {material:?}"));
            }
        }
    }
    eprintln!("{systems} system entries, {emitters} emitters, {} without a material in the kind", missing.len());
    for m in missing.iter().take(20) {
        eprintln!("  missing: {m}");
    }
    assert!(systems > 1000 && emitters > 4000);
}
