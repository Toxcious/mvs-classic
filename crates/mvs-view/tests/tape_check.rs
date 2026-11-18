//! The posing against a pose tape the oracle recorded: a World loaded from each frame of the trace match the tape was
//! recorded on (the oracle's own state and events, not stepped: the float32 sim drifts from it by design), placed
//! after each load, presented at the end of each run of equal inputs (as the oracle drew) and
//! at each tape frame's tick and alpha, and every mesh's transform, bones (alone and times the transform), shown
//! sections, stencil and materials compared. The tape is `MVS_POSE_TAPE` (default
//! `.scratch/tapes/lab-picture.tape` of the main tree, beside the traces), the trace `<MVS_TRACES>/<the tape's run>.trace`; without them, or
//! without the dev store, the test says so and passes.
//!
//! A frame's alpha is the oracle's presentation alpha (1: the latest tick's own state). With `MVS_TAPE_ALL` set, every
//! difference is printed as it is found, not only each kind's first.

use std::collections::HashMap;
use std::path::PathBuf;

use mvs_view::{FighterPose, MaterialRef, MeshPose, PoseTape, ViewPoser};

/// Within this of the tape: cm (translation) and unitless (the 3x3). The translation's is above a float32 ulp of a
/// bone's place: the World is loaded from the trace's own (the oracle's double sim) frames, whose montage time
/// (`anim_time`) the load rounds to float32 (Shaggy's EatSandwich fingers in lab-picture at t303-t304 move
/// 0.0034 cm from it; with the double's time they match the tape). The float32 sim's own drift is not in this test.
const TOL_CM: f64 = 0.005;
/// The trace header's hash interval, where the recorder cut its runs.
const HASH_EVERY: usize = 600;
const TOL_M: f64 = 1e-4;
/// And within this share of an entry's magnitude (a product's: of its factors' entries): a bone whose reference pose
/// has a tiny scale (LeBron's fml_un_C_scaler, 1e-12) has skin matrix entries far above 1, where a float32 ulp is above
/// the absolute tolerances, and the product with to_world rounds at its factors' size.
const TOL_REL: f64 = 1e-6;

/// (largest translation, largest 3x3) difference of two FMatrix3x4.
fn diff(a: &[f32; 12], b: &[f32; 12]) -> (f64, f64) {
    let (mut t, mut m) = (0.0f64, 0.0f64);
    for i in 0..12 {
        let d = (f64::from(a[i]) - f64::from(b[i])).abs();
        if i % 4 == 3 { t = t.max(d) } else { m = m.max(d) }
    }
    (t, m)
}

/// Whether an entry of two FMatrix3x4 differs past the tolerances (absolute, plus TOL_REL of the larger of the entry
/// and `mag`).
fn over(a: &[f32; 12], b: &[f32; 12], mag: f64) -> bool {
    (0..12).any(|i| {
        let (x, y) = (f64::from(a[i]), f64::from(b[i]));
        let tol = if i % 4 == 3 { TOL_CM } else { TOL_M };
        (x - y).abs() > tol + TOL_REL * x.abs().max(y.abs()).max(mag)
    })
}

/// The largest entry of a matrix.
fn largest(a: &[f32; 12]) -> f64 {
    a.iter().fold(0.0f64, |m, v| m.max(f64::from(v.abs())))
}

/// a b of two FMatrix3x4 (rows, the fourth row 0 0 0 1).
fn mul(a: &[f32; 12], b: &[f32; 12]) -> [f32; 12] {
    let mut o = [0.0f32; 12];
    for r in 0..3 {
        for c in 0..4 {
            let mut s = if c == 3 { a[r * 4 + 3] } else { 0.0 };
            for k in 0..3 {
                s += a[r * 4 + k] * b[k * 4 + c];
            }
            o[r * 4 + c] = s;
        }
    }
    o
}

/// A runtime texture (no object path) is the tape's `<runtime>`.
fn textures(m: &MaterialRef) -> Vec<(String, String)> {
    m.textures
        .iter()
        .map(|(n, t)| (n.to_string(), if t.starts_with('<') { "<runtime>".to_owned() } else { t.to_string() }))
        .collect()
}

fn scalars(m: &MaterialRef) -> Vec<(String, f32)> {
    let mut s: Vec<_> = m.scalars.iter().map(|(n, v)| (n.to_string(), *v)).collect();
    s.sort_by(|x, y| x.0.cmp(&y.0));
    s
}

#[derive(Default)]
struct Report {
    /// (cm, 3x3, where) of the bones alone, the bones times to_world, and to_world.
    worst: [(f64, f64, String, String); 3],
    /// per kind of difference: (count, first).
    notes: Vec<(String, usize, String)>,
    bad: bool,
}

impl Report {
    fn note(&mut self, kind: &str, s: String) {
        self.bad = true;
        if std::env::var_os("MVS_TAPE_ALL").is_some() {
            eprintln!("{kind}: {s}");
        }
        match self.notes.iter_mut().find(|n| n.0 == kind) {
            Some(n) => n.1 += 1,
            None => self.notes.push((kind.to_owned(), 1, s)),
        }
    }

    fn matrix(&mut self, k: usize, got: &[f32; 12], want: &[f32; 12], mag: f64, at: impl Fn() -> String) {
        let (t, m) = diff(got, want);
        let w = &mut self.worst[k];
        if t > w.0 {
            w.0 = t;
            w.2 = at();
        }
        if m > w.1 {
            w.1 = m;
            w.3 = at();
        }
        if over(got, want, mag) {
            self.note(["bone", "bone x to_world", "to_world"][k], format!("{}: {t:.6} cm, 3x3 {m:.3e}", at()));
        }
    }
}

fn compare(
    rep: &mut Report,
    tick: u32,
    got: &FighterPose,
    want: &FighterPose,
    names: &mut impl FnMut(&str) -> (Vec<String>, Vec<usize>),
    fighter_stencils: &[u8],
) {
    let f = want.fighter;
    let short = |m: &MeshPose| m.mesh.rsplit('.').next().unwrap_or("").to_owned();
    if got.meshes.len() != want.meshes.len() {
        let l = |v: &[MeshPose]| v.iter().map(short).collect::<Vec<_>>();
        rep.note("meshes", format!("t{tick} f{f}: {:?} / tape {:?}", l(&got.meshes), l(&want.meshes)));
    }
    // meshes paired by path and occurrence; the actors' (fighter 99), whose order is the oracle's scene children's (the
    // views' making order, not the sim's), by path and the nearest place among those not paired yet
    let nth = |v: &[MeshPose], i: usize| v[..i].iter().filter(|m| m.mesh == v[i].mesh).count();
    let mut taken = vec![false; got.meshes.len()];
    for (i, wm) in want.meshes.iter().enumerate() {
        let n = nth(&want.meshes, i);
        let pick = if f == 99 {
            let far = |m: &MeshPose| diff(&m.to_world, &wm.to_world).0;
            (0..got.meshes.len())
                .filter(|&k| !taken[k] && got.meshes[k].mesh == wm.mesh)
                .min_by(|&a, &b| far(&got.meshes[a]).total_cmp(&far(&got.meshes[b])))
        } else {
            (0..got.meshes.len()).filter(|&k| got.meshes[k].mesh == wm.mesh).nth(n)
        };
        let Some(k) = pick else { continue };
        taken[k] = true;
        let gm = &got.meshes[k];
        let at = format!("t{tick} f{f} {}#{n}", short(wm));
        rep.matrix(2, &gm.to_world, &wm.to_world, 0.0, || format!("{at} to_world"));
        if gm.bones.len() != wm.bones.len() {
            rep.note("bone count", format!("{at}: {} bones, tape {}", gm.bones.len(), wm.bones.len()));
        } else {
            let (bn, _) = names(&wm.mesh);
            for (b, (gb, wb)) in gm.bones.iter().zip(&wm.bones).enumerate() {
                let name = || format!("{at} bone {b} {}", bn.get(b).map_or("", String::as_str));
                if bn.get(b).is_some_and(|n| DYNAMICS_BONES.contains(&n.as_str())) {
                    continue;
                }
                rep.matrix(0, gb, wb, 0.0, name);
                let mag = largest(gb).max(largest(wb)) * largest(&gm.to_world).max(largest(&wm.to_world));
                rep.matrix(1, &mul(&gm.to_world, gb), &mul(&wm.to_world, wb), mag, name);
            }
        }
        // (a section of a TLM_Surface slot the oracle hid with its null material: TLM_SURFACE)
        let (_, slots) = names(&wm.mesh);
        let tlm = |i: usize| slots.get(i).and_then(|&k| gm.materials.get(k)).is_some_and(|m| tlm_surface(&m.path));
        let same_shown = gm.shown.len() == wm.shown.len()
            && gm.shown.iter().zip(&wm.shown).enumerate().all(|(i, (a, b))| a == b || (*a && !*b && tlm(i)));
        if !same_shown {
            rep.note("shown", format!("{at}: {:?}, tape {:?}", gm.shown, wm.shown));
        }
        // (a static mesh the oracle drew without a custom depth copy, which the view outlines with the fighter's
        // stencil: Comp_Pawn_MeshManager AddMesh @1291-1332 outlines every tagged mesh child, static ones too; the
        // oracle gave a static socket mesh or a pawn prop none)
        // (a pawn prop only: the fighter's own stencil, not an actor's: an item's outline is compared)
        // (the actors' pass has the props too: any stencil a fighter of the frame has; an item's is not told apart)
        let own = |s: u8| fighter_stencils.contains(&s);
        let static_unoutlined = gm.bones.is_empty() && wm.custom_stencil == 0 && own(gm.custom_stencil);
        if gm.custom_stencil != wm.custom_stencil && !static_unoutlined {
            rep.note("stencil", format!("{at}: {}, tape {}", gm.custom_stencil, wm.custom_stencil));
        }
        if gm.materials.len() != wm.materials.len() {
            rep.note("material count", format!("{at}: {}, tape {}", gm.materials.len(), wm.materials.len()));
        }
        // (a part the oracle gave its skin entries by section, not by slot: BY_SECTION)
        let by_section = BY_SECTION.contains(&short(wm).as_str());
        // the (view slot, tape slot) pairs: by index, or by section (entry k against the tape's slot of section k). The
        // drawn slot's own material is not compared: the view draws the skin's entry k there and the oracle its entry 0
        // (a different material on Taz's baguettes: the oracle's difference, ApplySubMeshes)
        let pairs: Vec<(usize, usize)> = if by_section {
            (0..gm.materials.len()).filter_map(|s| Some((s, *slots.get(s)?))).collect()
        } else {
            (0..gm.materials.len()).map(|s| (s, s)).collect()
        };
        for (s, t) in pairs {
            let a = &gm.materials[s];
            let Some(b) = wm.materials.get(t) else { continue };
            // (the recorder writes a slot whose material it does not track as the mesh's own slot, '' for none: the
            // oracle's stand-in for UMaterial::GetDefaultMaterial)
            if b.path.is_empty() && &*a.path == WORLD_GRID {
                continue;
            }
            if a.path != b.path {
                rep.note("material", format!("{at} slot {s}: {} / tape {}", a.path, b.path));
                continue;
            }
            let (mut sa, sb) = (scalars(a), scalars(b));
            // (LeBron's ball stretch parameters, written as material parameters: the oracle set shader
            // uniforms, which its recorder does not record: UNRECORDED)
            let mut va = a.vectors.clone();
            if UNRECORDED.contains(&short(wm).as_str()) {
                let names = ["Amount", "Direction", "Center"];
                sa.retain(|x| !names.contains(&x.0.as_str()) || sb.iter().any(|y| y.0 == x.0));
                va.retain(|x| !names.contains(&&*x.0) || b.vectors.iter().any(|y| y.0 == x.0));
            }
            if sb.is_empty() && b.vectors.is_empty() && textures(b).is_empty() && tlm_surface(&a.path) {
                continue;
            }
            let same = sa.len() == sb.len() && sa.iter().zip(&sb).all(|(x, y)| x.0 == y.0 && (x.1 - y.1).abs() < 1e-5);
            if !same || va != b.vectors || textures(a) != textures(b) {
                rep.note(
                    "parameters",
                    format!(
                        "{at} slot {s}: {sa:?} {:?} {:?} / tape {sb:?} {:?} {:?}",
                        va,
                        textures(a),
                        b.vectors,
                        textures(b)
                    ),
                );
            }
        }
    }
}

/// The TLM_Surface materials the native fighter kind names where the oracle's slotReader left the slot null (ledger
/// "Fighter kind TLM_Surface slots", fighter kind 15): the oracle drew the mesh's own material there with none of the
/// fighter set's parameters, so such a tape slot with no parameter is the oracle's difference, not the view's
/// (superman-combat, superman-flight, gizmo-combat).
const TLM_SURFACE: [&str; 5] =
    ["Superman_Cornea_MIC", "JuiceGlass_MAT", "ClockGlass_MAT", "Windshield_MAT", "BottleGlass_MAT"];

fn tlm_surface(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, n)| TLM_SURFACE.contains(&n))
}

/// The skin parts the oracle never drew that the view shows since an AnimBP's rule was ported (Iron Giant C017_S01's
/// Superman seal, ExtrasMesh facing right: C017_AnimBP SetSealVisibility, 28d28d9b6): a fighter's mesh of these the
/// tape lacks is the oracle's difference (skin-props-irongiant).
const ORACLE_UNDRAWN: [&str; 1] = ["C017_S01_SkelMesh"];

/// The bones the skins' AnimDynamics / Trail / SpringBone nodes drive (gizmo.rs, creature/skin_controls.rs: Gizmo's
/// headband, rope and holiday hat, Reindog's chef, tea party and mummy bones): the oracle ran no such node for them, so
/// their bones are the build's behaviour and not compared (gizmo-combat, skin-reindog, skin-reindog-tea: the bones that
/// differ there are these and no others).
const DYNAMICS_BONES: [&str; 14] = [
    "headband_shortEnd1",
    "headband_shortEnd2",
    "headband_longEnd1",
    "headband_longEnd2",
    "headband_longEnd3",
    "rope_shortEnd",
    "rope_longEnd",
    "holiday_hat1",
    "holiday_hat2",
    "holiday_hat3",
    "fml_un_rdg_chef_C_hat_end",
    "fml_un_tparty_C_hat",
    "mummyWrap_Leg_R",
    "mummyWrap_Leg_L1",
];

/// The parts whose ball-stretch parameters (Amount, Direction, Center) the oracle set as shader uniforms and its tape
/// does not hold (LeBron's C016_Ball_SkelMesh).
const UNRECORDED: [&str; 1] = ["C016_Ball_SkelMesh"];

/// The frames where the build draws Jake_Eat_Attack_Montage's OriginsSkin track (Jake_S02_Eat_Attack_In / Hit) on a
/// skin other than Jake_S02 (OriginsSkin has no IsOriginsSkin gate) and the oracle drew the default track: the
/// bones of the frame's fighter are the oracle's difference (checked: jake-combat passes at t6839 with the track off).
const ORACLE_DEFAULT_TRACK: [(&str, u32); 1] = [("jake-combat", 6839)];

/// `got` without its meshes of ORACLE_UNDRAWN that `want` lacks, and with Jake's bones of a frame of
/// ORACLE_DEFAULT_TRACK (`default_track`) the tape's.
fn without_undrawn(got: &FighterPose, want: &FighterPose, default_track: bool) -> FighterPose {
    let name = |m: &MeshPose| m.mesh.rsplit('.').next().unwrap_or("").to_owned();
    let mut g = got.clone();
    g.meshes.retain(|m| {
        let n = name(m);
        !ORACLE_UNDRAWN.contains(&n.as_str()) || want.meshes.iter().any(|w| name(w) == n)
    });
    if default_track {
        for m in g.meshes.iter_mut().filter(|m| !m.bones.is_empty()) {
            if let Some(w) = want.meshes.iter().find(|w| w.mesh == m.mesh) {
                m.bones.clone_from(&w.bones);
            }
        }
    }
    g
}

/// The skin parts whose sections are not on the slots of the same index, where the oracle put skin entry k on section k
/// and the view puts it on slot k (Comp_Pawn_SkinManager ApplySubMeshes: Materials[i] to slot i, ledger "Red / blue
/// variants"): the tape's slot of section k is compared with the view's slot k, and a slot no section draws is not
/// compared (skin-props-taz: C015_Baguette1 / 2 draw slot 1 alone).
const BY_SECTION: [&str; 2] = ["C015_Baguette1_SkelMesh", "C015_Baguette2_SkelMesh"];

/// UMaterial::GetDefaultMaterial(MD_Surface): the draws' material of a slot with none.
const WORLD_GRID: &str = "/Engine/EngineMaterials/WorldGridMaterial.WorldGridMaterial";

#[test]
fn the_posing_matches_the_pose_tape() {
    // (beside the traces: a worktree reads the main tree's)
    let traces = mvs_sim_trace::run::default_traces();
    let tape_path =
        std::env::var_os("MVS_POSE_TAPE").map_or_else(|| traces.join("../tapes/lab-picture.tape"), PathBuf::from);
    let Ok(tape) = PoseTape::open(&tape_path) else {
        eprintln!("no pose tape at {}: skipped", tape_path.display());
        return;
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("no dev store: skipped");
        return;
    };
    let trace_path = traces.join(format!("{}.trace", tape.header.run));
    let Ok(trace) = mvs_trace::Trace::read(&trace_path) else {
        eprintln!("no trace at {}: skipped", trace_path.display());
        return;
    };
    let (mut w, book) = mvs_sim_trace::world_for(&trace.header, &store).expect("the trace's World");
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    let mut cache: HashMap<String, (Vec<String>, Vec<usize>)> = HashMap::new();
    let mut names = |path: &str| {
        cache
            .entry(path.to_owned())
            .or_insert_with(|| {
                mvs_view::mesh::MeshInfo::load(&store, path)
                    .map(|m| (m.bones().to_vec(), m.sections.iter().map(|s| s.slot).collect()))
                    .unwrap_or_default()
            })
            .clone()
    };
    let stem = tape_path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_owned();
    let default_track = |tick: u32| ORACLE_DEFAULT_TRACK.contains(&(stem.as_str(), tick));
    let mut rep = Report::default();
    let (mut frames, mut bad_frames, mut loaded) = (0, 0, 0usize);
    for tick in tape.ticks() {
        let Some(tf) = tape.frame(tick).expect("the tape's frame") else { continue };
        // one load per trace frame (frame i is tick i + 1): the oracle's own state as a seeded run loads it, each
        // placed as the oracle places after each tick (the views' own state, an AnimBP's, runs on every place)
        let at = tf.tick as usize;
        assert!(at <= trace.len(), "the trace ends before the tape: tick {at}, {} frames", trace.len());
        while loaded < at {
            let fr = trace.frame(loaded).expect("the trace's frame");
            let l = mvs_sim_trace::load_frame(&mut w, &book, &fr);
            // (the frame's own events: the views' material and effect part reads the step's events)
            w.events = l.events;
            loaded += 1;
            poser.place(&w);
            w.events.clear();
            // the oracle drew one frame at the end of each run of equal inputs, runs cut where a hash is checked
            // (the oracle's pose-tape play: one harness step per run); a view that runs its AnimBP
            // once a drawn frame advanced only there
            if loaded < at {
                let next = trace.frame(loaded).expect("the trace's frame");
                if next.inputs != fr.inputs || loaded % HASH_EVERY == 0 {
                    poser.present(&w, 1.0, &mut fighters, &mut actors);
                }
            }
        }
        let alpha = tf.alpha;
        // the camera a view reads (the Jenkins pin's scale): the tape's, its up the view's Y axis in the world.
        // Unverified: the root is taken as the tape's origin, the oracle's lens with its shake; the oracle's pin reads
        // the root with none (render/camera cameraRoot), the same while no shake runs.
        let v = &tf.camera.translated_world_to_view;
        let len = (v[0][1] * v[0][1] + v[1][1] * v[1][1] + v[2][1] * v[2][1]).sqrt();
        poser.set_camera(tf.camera.origin, [0, 1, 2].map(|i| v[i][1] / len));
        poser.present(&w, alpha, &mut fighters, &mut actors);
        // (the oracle's tape keeps an actor's group only when it has meshes: a rule's view that draws nothing is none)
        actors.retain(|a| !a.meshes.is_empty());
        // (the oracle's tape keeps a view whose root is shown: a hidden one, posed for the effects, is none)
        fighters.retain(|f| !f.hidden);
        frames += 1;
        rep.bad = false;
        if fighters.len() != tf.fighters.len() {
            rep.note("fighters", format!("t{}: {} fighters, tape {}", tf.tick, fighters.len(), tf.fighters.len()));
        }
        let stencils: Vec<u8> = fighters
            .iter()
            .filter_map(|f| f.meshes.iter().find(|m| !m.bones.is_empty()))
            .map(|m| m.custom_stencil)
            .collect();
        for want in &tf.fighters {
            match fighters.iter().find(|f| f.fighter == want.fighter) {
                Some(got) => compare(
                    &mut rep,
                    tf.tick,
                    &without_undrawn(got, want, default_track(tf.tick)),
                    want,
                    &mut names,
                    &stencils,
                ),
                None => rep.note("fighter", format!("t{}: fighter {} not posed", tf.tick, want.fighter)),
            }
        }
        // the actors' meshes, all together (an actor's view is the oracle's scene child, in no order the sim gives),
        // paired by path and nearest place (compare): fighter 99 in the notes. (the recorder writes an actor with a
        // mesh drawn)
        let drawn = actors.iter().filter(|a| !a.meshes.is_empty()).count();
        if drawn != tf.actors.len() {
            rep.note("actors", format!("t{}: {} actors, tape {}", tf.tick, drawn, tf.actors.len()));
        }
        let all = |v: &[mvs_view::ActorPose]| FighterPose {
            fighter: 99,
            meshes: v.iter().flat_map(|a| a.meshes.iter().cloned()).collect(),
            priority: None,
            ..FighterPose::default()
        };
        compare(&mut rep, tf.tick, &all(&actors), &all(&tf.actors), &mut names, &stencils);
        if rep.bad {
            bad_frames += 1;
        }
    }
    for (kind, n, first) in &rep.notes {
        eprintln!("{kind}: {n} x, first {first}");
    }
    for (k, w) in rep.worst.iter().enumerate() {
        let what = ["bones", "bones x to_world", "to_world"][k];
        eprintln!("{what}: largest translation {:.6} cm at {}, largest 3x3 {:.3e} at {}", w.0, w.2, w.1, w.3);
    }
    eprintln!("{frames} frames, {bad_frames} with a difference");
    assert_eq!(bad_frames, 0, "frames that differ from the tape");
}
