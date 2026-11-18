//! Gizmo's effects. His barrier
//! (C023A_ProjectileBarrier_C) is the shared barrier view's (fx/shared/barrier.rs, entered for the class: BARRIERS).
//!
//! - C023A_Arrow_Projectile_C: its ParticleSystem (PS_C023A_ArrowTrail at (0, 120, 0), scale 1.75 under the mesh) is
//!   the shared component draw; with IsFire the construction script and UpdateVisualsMulticast set its template to
//!   PS_C023A_FlameArrow (@313, @827): `made` puts that system in the component's place.
//! - C023A_MusicPickup_C: no ParticleSystem component; PS_C023_SingNote and PS_C023_Damage under MusicMesh (bVisible
//!   false, not drawn): `more`. PS_C023_Damage's visibility is the sim's damage_fx (Follow projectile @6039 on,
//!   LingerDamageMulticast @7330 off).
//! - C023A_PianoNote_C: ParticleSystem (PS_C023A_PianoNoteScroll) and PS_C023_PianoNote (scale 0.13) under the
//!   StaticMesh (bVisible false); the construction script sets the scroll's world scale to (1, 1, 1.2) (@10): `made`
//!   spawns it at that scale in the component's place. Deviation: the actor's scale at the spawn is taken for good
//!   (the scale follows a later change).
//! - C023A_Car_SpawnedActor_C's ParticleSystem (PS_C023A_OverheatCar) shown once SetOverheatState has run
//!   (OverheatVisuals @13485: the sim's has_overheat; the FFA cars' the same: ../items.rs). Unverified: drawn at the
//!   SkeletalMesh component's origin and at the actor's sim place (the attached car's mesh rides the driver's socket:
//!   mvs-view chars/gizmo.rs).
//! - The car's TurnVisuals (@14828, DriverTurnEvent(FacingLeft) while IsDriven: the sim's `turns`):
//!   SpawnEmitterAtLocation PS_RunTrail at the actor's place, rotation 0, scale 1 @14866, and PS_C023A_TireTracks at
//!   the place - (0, FacingLeft ? 75 : -75, 50), scale (4, FacingLeft ? -4 : 4, 4) @14963-15197, Team the owner's
//!   @15475. FacingLeft: the driver's (else the owner's) facing at the step that first sees the count go up.
//!   Unverified: checked once a World step; a rollback that takes
//!   a turn back and replays it spawns nothing again.
//! - C023A_PopCorn_C: PS_C023A_PopcornBall on the Capsule, hidden at Explode / Shotgun, its relative scale
//!   VLerp(1, 2, Grow) (the sim's hidden and grow_scale).
//! - The arrow-charge components' particle systems: ArrowFireCharged (PS_C023A_LitArrow on the socket VB ArrowAttach,
//!   relative (-90, 0, 0) along the mesh's axes) and ArrowFire (on bowArrow at (150, 0, 0)), started when their mesh is
//!   shown and killed when it is hidden (Unverified, as Batman's bomb auras), Team = the pawn's.
//!   Unverified: the mesh's socket VFX is not in the data: ArrowFireCharged sits at the mesh's origin plus its
//!   relative (-90, 0, 0).
//! - Reset: the Lab's reset has ended the systems: the handles are dropped.

use indexmap::IndexMap;
use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::ProjectileExt;
use mvs_sim::actors::projectile::{Projectile, attach_parent};
use mvs_sim::fighter::common::actor_def;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, Rows, V3, rotation_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{rot_rows, text, xyz};
use crate::fx::{Follow, Fx, under};

const DEG: f64 = std::f64::consts::PI / 180.0;
const ARROW: &str = "C023A_Arrow_Projectile_C";
const FLAME: &str = "PS_C023A_FlameArrow";
const LIT: &str = "PS_C023A_LitArrow";
const CHARGE_TAG: &str = "arrowcharge";
const FIRE_TAG: &str = "firearrownullmesh";

/// The barrier classes this module enters (fx/shared/barrier.rs ENTERED): C023A_ProjectileBarrier_C's SetTeamColor
/// literal, PS_C023A_HideBox @1253 (attached to the root, world scale 1 @1349, Team @1736 as the parent's).
/// Unverified: the emitter's EX_RotationConst @1253 taken as the parent's (its bytecode is not in the export).
pub(crate) const BARRIERS: &[(&str, &str)] = &[("C023A_ProjectileBarrier_C", "PS_C023A_HideBox")];

/// One of the pawn's lit-arrow systems: its mesh's tag, its socket and relative place, its handle.
#[derive(Debug)]
struct Lit {
    tag: &'static str,
    socket: String,
    loc: V3,
    rot: Rows,
    h: Option<SystemId>,
}

/// What Gizmo's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Gizmo {
    /// By fighter id: ArrowFireCharged and ArrowFire.
    lits: IndexMap<i32, Vec<Lit>>,
    warned: std::collections::HashSet<String>,
    /// By car projectile seq: the sim's turn count last seen.
    turns: IndexMap<i32, i32>,
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (ARROW, ProjRule { made: Some(arrow_made), ..ProjRule::NONE }),
    (
        "C023A_MusicPickup_C",
        ProjRule { more: &["PS_C023_SingNote", "PS_C023_Damage"], tick: Some(pickup_tick), ..ProjRule::NONE },
    ),
    ("C023A_PianoNote_C", ProjRule { more: &["PS_C023_PianoNote"], made: Some(note_made), ..ProjRule::NONE }),
    (
        "C023A_Car_SpawnedActor_C",
        ProjRule { shown: Some(car_shown), tick: Some(car_tick), end: Some(car_end), ..ProjRule::NONE },
    ),
    ("C023A_PopCorn_C", ProjRule { shown: Some(popcorn_shown), scale: Some(popcorn_scale), ..ProjRule::NONE }),
];

fn car_shown(_w: &World, pr: &Projectile) -> bool {
    matches!(&pr.ext, ProjectileExt::GizmoCar(c) if c.car.has_overheat)
}

const RUN_TRAIL: &str = "PS_RunTrail";
const TIRE_TRACKS: &str = "PS_C023A_TireTracks";

fn me(c: &mut CharFx) -> &mut Gizmo {
    match c {
        CharFx::Gizmo(s) => s,
        _ => unreachable!("a Gizmo rule entered by another module"),
    }
}

/// TurnVisuals for each DriverTurnEvent since the last step (the header).
fn car_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let Some(car) = mvs_sim::chars::gizmo::car::car_of(&pr.ext) else { return };
    let g = me(c);
    let was = g.turns.insert(st.seq, car.turns);
    if was.is_none_or(|was| car.turns <= was) || !car.driven {
        return;
    }
    let Some(by) = car.driver.or(pr.owner) else { return };
    let left = w.fighter(by).body.facing_dir < 0.0;
    let team = [("Team", fx.team(pr.owner_id))];
    if g.has(fx, RUN_TRAIL) {
        fx.player.spawn(RUN_TRAIL, [0.0, f64::from(-pr.x), f64::from(pr.y)], [0.0; 3], [1.0; 3], &team);
    }
    if g.has(fx, TIRE_TRACKS) {
        let side = if left { 75.0 } else { -75.0 };
        let sy = if left { -4.0 } else { 4.0 };
        fx.player.spawn(
            TIRE_TRACKS,
            [0.0, f64::from(-pr.x - side), f64::from(pr.y - 50.0)],
            [0.0; 3],
            [4.0, sy, 4.0],
            &team,
        );
    }
}

fn car_end(c: &mut CharFx, _fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    me(c).turns.shift_remove(&st.seq);
}

fn popcorn_shown(_w: &World, pr: &Projectile) -> bool {
    !matches!(&pr.ext, ProjectileExt::Popcorn(p) if p.hidden)
}

fn popcorn_scale(_w: &World, pr: &Projectile, ms: V3) -> Option<V3> {
    let g = match &pr.ext {
        ProjectileExt::Popcorn(p) => p.grow_scale,
        _ => 1.0,
    };
    Some(ms.map(|v| v * f64::from(g)))
}

fn pickup_tick(_c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let on = matches!(&pr.ext, ProjectileExt::MusicPickup(m) if m.damage_fx);
    if let Some(Some(h)) = st.more.get(1) {
        fx.player.set_visible(*h, on);
    }
}

/// A component's follow under the actor (the shared draw's composition): relative place and rotation, scale.
fn component_follow(w: &World, st: &ProjFxState, name: &str) -> Option<(String, Follow)> {
    let pr = w.actors.projectiles.get(st.id)?;
    let cs = &actor_def(w, ActorRef::Projectile(st.id), pr.cls)?.components;
    let c = cs.get(name)?.as_obj()?;
    let template = text(c, "Template")?.to_owned();
    let par = attach_parent(cs, name);
    let (rel, rs, _, rel_rot) = under(
        &par,
        xyz(c.get("RelativeLocation"), 0.0),
        &rot_rows(c.get("RelativeRotation")),
        xyz(c.get("RelativeScale3D"), 1.0),
        None,
        par.scale.map(f64::from),
    );
    Some((template, Follow::Projectile { seq: st.seq, rel, rel_rot, scale: rs }))
}

/// IsFire: the construction script and UpdateVisualsMulticast set the ParticleSystem's template to the flame.
fn arrow_made(_c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    if !matches!(&pr.ext, ProjectileExt::Arrow(a) if a.fire) {
        return;
    }
    if let Some(h) = st.h.take() {
        fx.player.kill(h);
    }
    if let Some((_, f)) = component_follow(w, st, "ParticleSystem") {
        let team = fx.team(pr.owner_id);
        st.h = fx.spawn_follow(w, FLAME, f, 1.0, &[("Team", team)]);
    }
}

/// UserConstructionScript @10 ParticleSystem.SetWorldScale3D((1, 1, 1.2)): the scroll is not scaled by the
/// StaticMesh's 6.
fn note_made(_c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    if let Some(h) = st.h.take() {
        fx.player.kill(h);
    }
    let Some((template, f)) = component_follow(w, st, "ParticleSystem") else { return };
    let Follow::Projectile { seq, rel, rel_rot, .. } = f else { return };
    let s = if pr.scale == 0.0 { 1.0 } else { pr.scale };
    let team = fx.team(pr.owner_id);
    let f = Follow::Projectile { seq, rel, rel_rot, scale: [1.0, 1.0, 1.2].map(|v| v * s).map(f64::from) };
    st.h = fx.spawn_follow(w, &template, f, 1.0, &[("Team", team)]);
}

impl Gizmo {
    fn has(&mut self, fx: &mut Fx, ps: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: Gizmo: {ps} is not loaded");
        }
        false
    }

    /// `arrowProps`' lit systems of fighter `f` from the data (None: not in the data, warned).
    fn lits_of(&mut self, w: &World, f: mvs_sim::fighter::FighterId) -> Option<Vec<Lit>> {
        let d = &w.char_data(f).fighter.components.pawn;
        let charge = d.get("ArrowCharge_GEN_VARIABLE");
        let fire = d.get("FireArrowNullMesh_GEN_VARIABLE");
        let charged = d.get("ArrowFireCharged_GEN_VARIABLE");
        let (Some(charge), Some(fire), Some(charged)) = (charge, fire, charged) else {
            if self.warned.insert("components".into()) {
                eprintln!("warning: Gizmo: the arrow-charge components are not in the data: not drawn");
            }
            return None;
        };
        let (Some(cp), Some(csock), Some(fp), Some(fsock), Some(pp)) =
            (charge.p.as_obj(), charge.socket.clone(), fire.p.as_obj(), fire.socket.clone(), charged.p.as_obj())
        else {
            return None;
        };
        let c_loc = xyz(cp.get("RelativeLocation"), 0.0);
        let r = cp.get("RelativeRotation").and_then(Value::as_obj);
        let num = |k: &str| r.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
        let c_rot = rotation_rows(num("Pitch") * DEG, num("Yaw") * DEG, num("Roll") * DEG);
        // ArrowFireCharged under the mesh (scale 1): its relative location along the mesh's axes
        let pl = xyz(pp.get("RelativeLocation"), 0.0);
        let lit: V3 =
            std::array::from_fn(|k| c_loc[k] + pl[0] * c_rot[k] + pl[1] * c_rot[3 + k] + pl[2] * c_rot[6 + k]);
        Some(vec![
            Lit { tag: CHARGE_TAG, socket: csock, loc: lit, rot: c_rot, h: None },
            Lit { tag: FIRE_TAG, socket: fsock, loc: xyz(fp.get("RelativeLocation"), 0.0), rot: IDENTITY, h: None },
        ])
    }

    fn kill_all(&mut self, fx: &mut Fx) {
        for h in self.lits.drain(..).flat_map(|(_, v)| v).filter_map(|l| l.h) {
            fx.player.kill(h);
        }
    }
}

impl CharFxHooks for Gizmo {
    /// gizmoFx's place (once a presentation tick, after the view's pose): the lit systems ride their mesh's
    /// visibility (the sim's sub-mesh state).
    fn place(&mut self, fx: &mut Fx, w: &World) {
        for f in w.all_ids() {
            if w.names.str(w.fighter(f).ch) != mvs_sim::chars::gizmo::KEY {
                continue;
            }
            let id = w.fighter(f).id;
            if !self.lits.contains_key(&id) {
                let l = self.lits_of(w, f).unwrap_or_default();
                self.lits.insert(id, l);
            }
            let fi = w.fighter(f);
            // (visible: !dead and !host.hidden, the pawn's mesh hidden while dead or invisible)
            let alive = fi.life.dead == 0.0 && !fi.vis.invisible;
            let listed = &w.char_data(f).def.sub_meshes;
            let team = fx.team(id);
            let mut lits = std::mem::take(&mut self.lits[&id]);
            for l in &mut lits {
                let hidden = w.names.get(l.tag).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
                let on = alive && listed.iter().any(|t| t == l.tag) && !hidden;
                if !on {
                    if let Some(h) = l.h.take() {
                        fx.player.kill(h);
                    }
                } else if !l.h.is_some_and(|h| fx.player.alive(h)) && self.has(fx, LIT) {
                    let follow = fx.socket_follow(w, id, Some(&l.socket), l.loc, &l.rot, [1.0; 3]);
                    l.h = fx.attach_to_mesh(w, id, LIT, &l.socket, follow, &[("Team", team)]);
                }
            }
            self.lits[&id] = lits;
        }
    }

    fn reset(&mut self, _fx: &mut Fx) {
        // the player's systems are cleared: the handles are dropped
        for l in self.lits.values_mut().flatten() {
            l.h = None;
        }
        // (the car views go with the World's projectiles)
        self.turns.clear();
    }

    fn end(&mut self, fx: &mut Fx) {
        self.kill_all(fx);
    }
}
