//! Replays the particle tape (the oracle's player over the shaggy and base systems, or
//! the systems it was given with the Collision modules' surfaces and view) on the Rust player with the oracle's random
//! draws and checks, bit for bit, every recorded particle and fill. The tape is `MVS_VFX_TAPE` or
//! `.scratch/vfx/vfx.tape` at the tree's root; without one the test passes with a note.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use mvs_store::{Store, dev_store_dir};
use mvs_vfx::emitter::Particle;
use mvs_vfx::fill::{Blend, SpriteSort, View, fill_meshes, fill_ribbon, fill_sprites};
use mvs_vfx::template::Kind;
use mvs_vfx::{Attachment, Player, RandomSource, Surface, SystemId};
use trial_tape::{Cursor, Tape, same_bits};

/// A (system, emitter) pair of the tape.
type EmitterKey = (usize, usize);
/// One emitter's fill: the vertex count and the bytes.
type Filled = (u32, Vec<u8>);

/// The oracle's draws in order.
struct Replay {
    vals: Arc<Vec<f64>>,
    at: Arc<AtomicUsize>,
}

impl RandomSource for Replay {
    fn next_f64(&mut self) -> f64 {
        let i = self.at.fetch_add(1, Ordering::Relaxed);
        *self.vals.get(i).unwrap_or_else(|| panic!("random draw {i}: the oracle made only {}", self.vals.len()))
    }
}

fn fields(p: &Particle) -> Vec<f64> {
    let mut v = Vec::with_capacity(29);
    v.extend(p.loc);
    v.extend(p.old);
    v.extend(p.vel);
    v.extend(p.size);
    v.extend(p.color);
    v.push(p.rot);
    v.push(p.rel_time);
    v.push(p.sub_image);
    v.extend(p.dyn_param);
    v.extend(p.orbit);
    v.extend(p.mesh_rot);
    v
}

const FIELD_NAMES: [&str; 29] = [
    "loc.x",
    "loc.y",
    "loc.z",
    "old.x",
    "old.y",
    "old.z",
    "vel.x",
    "vel.y",
    "vel.z",
    "size.x",
    "size.y",
    "size.z",
    "color.r",
    "color.g",
    "color.b",
    "color.a",
    "rot",
    "relTime",
    "subImage",
    "dyn.0",
    "dyn.1",
    "dyn.2",
    "dyn.3",
    "orbit.x",
    "orbit.y",
    "orbit.z",
    "meshRot.x",
    "meshRot.y",
    "meshRot.z",
];

fn f32_at(v: &[u8], i: usize) -> f32 {
    f32::from_le_bytes(v[4 * i..4 * i + 4].try_into().unwrap())
}

fn same32(a: f32, b: f64) -> bool {
    a.to_bits() == (b as f32).to_bits() || (a.is_nan() && b.is_nan())
}

/// The first difference, kept with where it is.
#[derive(Default)]
struct Diffs {
    first: Option<String>,
    count: usize,
    checked: usize,
}

impl Diffs {
    fn check(&mut self, ok: bool, what: impl FnOnce() -> String) {
        self.checked += 1;
        if !ok {
            self.count += 1;
            if self.first.is_none() {
                self.first = Some(what());
            }
        }
    }
}

#[test]
fn replay_vfx_tape() {
    let path = std::env::var_os("MVS_VFX_TAPE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/vfx/vfx.tape"));
    let Ok(tape) = Tape::open(&path) else {
        eprintln!("no tape at {}: skipped (the TS oracle's VFX recorder writes it)", path.display());
        return;
    };
    let store = Store::open(dev_store_dir()).expect("the store the tape was made from");
    let id = |n: &str| tape.header.id(n);
    let (spawn, random, tick, deact, kill, attach, state, fill, view_id) = (
        id("spawn"),
        id("random"),
        id("tick"),
        id("deactivate"),
        id("kill"),
        id("attach"),
        id("state"),
        id("fill"),
        id("view"),
    );
    let (alpha_id, component, activate, location, clear, immortal) =
        (id("alpha"), id("component"), id("activate"), id("location"), id("clear"), id("immortal"));
    // (a tape of the shaggy and base systems has neither)
    let maybe = |n: &str| tape.header.calls.iter().find(|c| c.name == n).map(|c| c.id);
    let (surfaces_id, collision_view) = (maybe("surfaces"), maybe("collision view"));
    let vals: Vec<f64> = tape.records().filter(|r| r.id == random).map(|r| Cursor::new(r).f64()).collect();
    let view = tape
        .records()
        .find(|r| r.id == view_id)
        .map(|r| {
            let mut c = Cursor::new(r);
            View { origin: [c.f64(), c.f64(), c.f64()], forward: [c.f64(), c.f64(), c.f64()] }
        })
        .expect("a view record");
    let at = Arc::new(AtomicUsize::new(0));
    let mut player = Player::new(&store, 0).unwrap();
    player.set_random(Box::new(Replay { vals: Arc::new(vals), at: at.clone() }));
    let mut ids: Vec<Option<SystemId>> = Vec::new();
    let mut frame = 0usize;
    // this frame's fills, per (system, emitter): (count, vertices)
    let mut fills: Vec<(EmitterKey, Filled)> = Vec::new();
    let mut draws = Diffs::default();
    let mut states = Diffs::default();
    let mut fill_diffs = Diffs::default();
    let mut alive_diffs = Diffs::default();
    let mut calls = Diffs::default();
    let mut alpha = 1.0;
    let name_of = |c: &mut Cursor| {
        let n = c.u32() as usize;
        String::from_utf8((0..n).map(|_| c.u8()).collect()).unwrap()
    };
    for r in tape.records() {
        let mut c = Cursor::new(r);
        if r.id == alpha_id {
            alpha = c.f64();
        } else if Some(r.id) == surfaces_id {
            let n = c.u32() as usize;
            let surfaces = (0..n)
                .map(|_| {
                    let z = c.f64();
                    let rect = (c.u8() == 1).then(|| [c.f64(), c.f64(), c.f64(), c.f64()]);
                    Surface { z, rect }
                })
                .collect();
            player.set_surfaces(Some(surfaces));
        } else if Some(r.id) == collision_view {
            let loc = [c.f64(), c.f64(), c.f64()];
            player.set_collision_view(loc, c.f64());
        } else if r.id == immortal {
            let name = name_of(&mut c);
            let want = c.u8() == 1;
            let got = player.immortal(&name);
            calls.check(got == want, || format!("{name}: immortal {got}, the oracle {want}"));
        } else if r.id == component {
            let name = name_of(&mut c);
            let loc = [c.f64(), c.f64(), c.f64()];
            let rot = [c.f64(), c.f64(), c.f64()];
            let scale = c.f64();
            let id = player.attach_component(&name, loc, rot, scale, &[]);
            calls.check(id.is_some_and(|id| player.deactivated(id)), || {
                format!("{name}: a kept component starts active")
            });
            ids.push(id);
        } else if r.id == activate {
            if let Some(Some(id)) = ids.get(c.u32() as usize) {
                player.activate(*id);
                calls.check(!player.deactivated(*id), || format!("frame {frame}: activate left it inactive"));
            }
        } else if r.id == location {
            let n = c.u32() as usize;
            let loc = [c.f64(), c.f64(), c.f64()];
            if let Some(Some(id)) = ids.get(n) {
                player.set_location(*id, loc);
            }
        } else if r.id == clear {
            player.clear();
        } else if r.id == spawn {
            let n = c.u32() as usize;
            let name = String::from_utf8((0..n).map(|_| c.u8()).collect()).unwrap();
            let attached = c.u8() == 1;
            let before = c.u8() == 1;
            let loc = [c.f64(), c.f64(), c.f64()];
            if attached {
                let mut rows = [[0.0; 3]; 3];
                for row in &mut rows {
                    for x in row.iter_mut() {
                        *x = c.f64();
                    }
                }
                let scale = c.f64();
                ids.push(player.spawn_attached(&name, &Attachment { loc, rot: rows, scale: None }, scale, &[]));
            } else {
                let rot = [c.f64(), c.f64(), c.f64()];
                let scale = [c.f64(), c.f64(), c.f64()];
                let s = if before {
                    player.spawn_before_manager(&name, loc, rot, scale, &[])
                } else {
                    player.spawn(&name, loc, rot, scale, &[])
                };
                ids.push(s);
            }
        } else if r.id == attach {
            let n = c.u32() as usize;
            let loc = [c.f64(), c.f64(), c.f64()];
            let mut rows = [[0.0; 3]; 3];
            for row in &mut rows {
                for x in row.iter_mut() {
                    *x = c.f64();
                }
            }
            let a = Attachment { loc, rot: rows, scale: None };
            if let Some(Some(id)) = ids.get(n) {
                player.attach(*id, &a);
            }
        } else if r.id == deact {
            if let Some(Some(id)) = ids.get(c.u32() as usize) {
                player.deactivate(*id);
            }
        } else if r.id == kill {
            if let Some(Some(id)) = ids.get(c.u32() as usize) {
                player.kill(*id);
            }
        } else if r.id == view_id {
            player.set_view(view.origin, view.forward);
        } else if r.id == tick {
            let dt = c.f64();
            let want = c.u32() as usize;
            player.tick(dt);
            let got = at.load(Ordering::Relaxed);
            draws.check(got == want, || format!("frame {frame}: {got} random draws so far, the oracle {want}"));
            // the frame's fills, every live emitter with particles in spawn order (ribbon fills write the particles'
            // interp counts, as the oracle's do)
            fills.clear();
            for (n, sid) in ids.iter().enumerate() {
                let Some(sid) = sid else { continue };
                if !player.alive(*sid) {
                    continue;
                }
                let s = player.system_mut(*sid).unwrap();
                for e in 0..s.emitters.len() {
                    let em = &mut s.emitters[e];
                    if em.particle_count() == 0 {
                        continue;
                    }
                    let t = em.t.clone();
                    let mut out = Vec::new();
                    let blend = Blend { alpha, from: s.draw_from };
                    let count = match t.kind {
                        Kind::Sprite => {
                            fill_sprites(&t, &s.sys, em, &view, &blend, &mut SpriteSort::default(), &mut out)
                        }
                        Kind::Mesh => fill_meshes(&t, &s.sys, em, &view, &blend, &mut out),
                        Kind::Ribbon => fill_ribbon(&t, &s.sys, em, &view, &mut out),
                        // (the recorder keeps no camera up axis: a tape with a beam emitter needs one first)
                        Kind::Beam => panic!("{}: a beam emitter on the tape", t.name),
                    };
                    fills.push(((n, e), (count, out)));
                }
            }
            frame += 1;
        } else if r.id == state {
            let n = c.u32() as usize;
            let _alive = c.u8();
            let e = c.u32() as usize;
            let count = c.u32() as usize;
            let f = frame - 1;
            let Some(Some(sid)) = ids.get(n) else {
                alive_diffs.check(false, || format!("frame {f}: system {n} not spawned here"));
                continue;
            };
            let Some(ems) = player.emitters(*sid) else {
                alive_diffs.check(false, || format!("frame {f}: system {n} ended here, live in the oracle"));
                continue;
            };
            alive_diffs.check(true, String::new);
            let em = &ems[e];
            states.check(em.particle_count() == count, || {
                format!(
                    "frame {f}: system {n} emitter {e} ({}): {} particles, the oracle {count}",
                    em.t.name,
                    em.particle_count()
                )
            });
            if em.particle_count() != count {
                continue;
            }
            for (i, p) in em.particles().enumerate() {
                let got = fields(p);
                for (k, g) in got.iter().enumerate() {
                    let w = c.f64();
                    states.check(same_bits(*g, w), || {
                        format!(
                            "frame {f}: system {n} emitter {e} ({}) particle {i} {}: {g:e}, the oracle {w:e}",
                            em.t.name, FIELD_NAMES[k]
                        )
                    });
                }
            }
        } else if r.id == fill {
            let n = c.u32() as usize;
            let e = c.u32() as usize;
            let kind = c.u8();
            let pushes = c.u32() as usize;
            let f = frame - 1;
            let Some((_, (count, v))) = fills.iter().find(|(k, _)| *k == (n, e)) else {
                fill_diffs.check(false, || format!("frame {f}: system {n} emitter {e}: no fill here"));
                continue;
            };
            let at = |what: &str, i: usize| format!("frame {f}: system {n} emitter {e} {what} {i}");
            match kind {
                0 => {
                    fill_diffs.check(*count as usize == pushes, || {
                        at("sprites", *count as usize) + &format!(", the oracle {pushes}")
                    });
                    if *count as usize != pushes {
                        continue;
                    }
                    for i in 0..pushes {
                        let q: Vec<f64> = (0..19).map(|_| c.f64()).collect();
                        let b = &v[i * 80..(i + 1) * 80];
                        // (vertex float: oracle push value)
                        let map =
                            [(0, 0), (1, 1), (2, 2), (4, 3), (5, 4), (6, 5), (8, 6), (9, 7), (10, 8), (11, 9), (3, 10)];
                        for (vi, qi) in map.into_iter().chain((0..8).map(|k| (12 + k, 11 + k))) {
                            fill_diffs.check(same32(f32_at(b, vi), q[qi]), || {
                                at("sprite", i) + &format!(" float {vi}: {:e}, the oracle {:e}", f32_at(b, vi), q[qi])
                            });
                        }
                    }
                }
                1 => {
                    fill_diffs.check(*count as usize == pushes, || {
                        at("meshes", *count as usize) + &format!(", the oracle {pushes}")
                    });
                    if *count as usize != pushes {
                        continue;
                    }
                    for i in 0..pushes {
                        let q: Vec<f64> = (0..32).map(|_| c.f64()).collect();
                        let b = &v[i * 112..(i + 1) * 112];
                        let mut map: Vec<(usize, usize)> = (0..4).map(|k| (k, 12 + k)).collect();
                        for col in 0..3 {
                            map.extend([
                                (4 + 4 * col, col),
                                (5 + 4 * col, 3 + col),
                                (6 + 4 * col, 6 + col),
                                (7 + 4 * col, 9 + col),
                            ]);
                        }
                        map.extend((0..4).map(|k| (16 + k, 28 + k)));
                        map.extend([(22, 24), (23, 25)]);
                        map.extend((0..4).map(|k| (24 + k, 16 + k)));
                        for (vi, qi) in map {
                            fill_diffs.check(same32(f32_at(b, vi), q[qi]), || {
                                at("mesh", i) + &format!(" float {vi}: {:e}, the oracle {:e}", f32_at(b, vi), q[qi])
                            });
                        }
                        for k in 0..4 {
                            let s = i16::from_le_bytes(b[80 + 2 * k..82 + 2 * k].try_into().unwrap());
                            fill_diffs.check(f64::from(s) == q[20 + k], || {
                                at("mesh", i) + &format!(" SubUVParams {k}: {s}, the oracle {}", q[20 + k])
                            });
                        }
                    }
                }
                _ => {
                    let mut vi = 0usize;
                    let mut first = true;
                    for i in 0..pushes {
                        let new_strip = c.u8() == 1;
                        let q: Vec<f64> = (0..18).map(|_| c.f64()).collect();
                        if new_strip && !first {
                            vi += 2;
                        }
                        first = false;
                        for side in 0..2 {
                            let Some(b) = v.get(vi * 96..(vi + 1) * 96) else {
                                fill_diffs.check(false, || at("ribbon pair", i) + ": past the Rust strip");
                                break;
                            };
                            let w = if side == 0 { q[6] } else { -q[6] };
                            let mut want = vec![q[0] + q[3] * w, q[1] + q[4] * w, q[2] + q[5] * w];
                            want.extend(&q[10..14]);
                            want.extend([q[7], f64::from(side), q[8], if side == 0 { 0.0 } else { q[9] }]);
                            want.extend(&q[14..18]);
                            let idx = [0, 1, 2, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23];
                            for (k, &x) in idx.iter().enumerate() {
                                fill_diffs.check(same32(f32_at(b, x), want[k]), || {
                                    at("ribbon pair", i)
                                        + &format!(
                                            " side {side} float {x}: {:e}, the oracle {:e}",
                                            f32_at(b, x),
                                            want[k]
                                        )
                                });
                            }
                            vi += 1;
                        }
                    }
                    fill_diffs.check(*count as usize == vi, || {
                        at("ribbon vertices", *count as usize) + &format!(", the oracle's pairs make {vi}")
                    });
                }
            }
        }
    }
    let report = |what: &str, d: &Diffs| {
        eprintln!(
            "{what}: {} checked, {} differ{}",
            d.checked,
            d.count,
            d.first.as_ref().map_or(String::new(), |f| format!("; first: {f}"))
        );
    };
    report("random draws", &draws);
    report("systems alive", &alive_diffs);
    report("particles", &states);
    report("fills", &fill_diffs);
    report("component and immortal calls", &calls);
    assert!(draws.checked > 0 && states.checked > 0 && fill_diffs.checked > 0 && calls.checked > 0);
    assert!(
        draws.count + alive_diffs.count + states.count + fill_diffs.count + calls.count == 0,
        "the replay differs from the oracle"
    );
}
