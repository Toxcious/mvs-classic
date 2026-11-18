//! Morty's effects (the CharFx hooks): the markers' emitters, the mortar cursor,
//! the accelerators' portal, the ships' portal and rings, Buff_OnExitedArenaTeleportToAlly's rescue ring and the
//! four events, the Cleaner perk's scrub shield's emitters. The ship's mesh is mvs-view's (chars/morty.rs).
//! The shots' rules: the grenade's dilation and its PS_DamageVisual, the whip's Team, the blaster's
//! scale and its reflect, the whip's Cable; the marker's rewind trail. The cable's and the trail's geometry is
//! mvs-view's (meshes::Cable, meshes::SplineMesh), drawn as component meshes (crate::cmesh).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use mvs_data::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{Projectile, attach_parent, class_p, hitbox_active};
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::morty::marker::{Seg, trail};
use mvs_sim::chars::morty::mortar::aim_x;
use mvs_sim::events::Event;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::actor_def;
use mvs_sim::fighter::rescue_vfx_born;
use mvs_sim::name;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};
use mvs_vfx::player::Attachment;
use mvs_view::WarmMesh;
use mvs_view::meshes::{AlphaRule, Cable, CableDef, SplineMesh, SplineSlice};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule, component_warm, fighter_of};
use crate::cmesh::{CMeshGeometry, CMeshId, SplineSource, object_path, spline_geometry};
use crate::fx::attach::{att, warn_once, xyz};
use crate::fx::{Follow, Fx};

/// What Morty's effects keep between steps, by the actors' sequence numbers (the ids).
#[derive(Debug, Default)]
pub(crate) struct Morty {
    markers: BTreeMap<i32, MarkerFx>,
    mortars: HashMap<i32, Option<SystemId>>,
    accelerators: HashMap<i32, Option<SystemId>>,
    ships: HashMap<i32, Option<SystemId>>,
    /// Per pawn id: the rescue buff's VFX born tick and its emitter.
    rescue: HashMap<i32, (f64, Option<SystemId>)>,
    /// Per grenade: its PS_DamageVisual while shown.
    kb: HashMap<i32, Option<SystemId>>,
    /// Per blaster shot: the owner's id and the reflect pause last seen.
    blasters: HashMap<i32, (i32, u8)>,
    /// Per whip: its Cable.
    whips: BTreeMap<i32, Whip>,
    /// Per scrub shield (C019ScrubShield_C, by the actor's sequence number): its emitters.
    scrubs: BTreeMap<i32, Scrub>,
}

/// A scrub shield's emitters: the sparkle trail of its BeginPlay, a vacuum trail per Clean, and the Cleans seen.
#[derive(Debug, Default)]
struct Scrub {
    hs: Vec<SystemId>,
    cleaned: i32,
}

#[derive(Debug)]
struct MarkerFx {
    h: Option<SystemId>,
    travel: Option<SystemId>,
    trail: Option<Trail>,
}

/// A marker's rewind trail: its spline mesh components laid as one mesh.
#[derive(Debug)]
struct Trail {
    src: Arc<SplineSource>,
    mesh: SplineMesh,
    /// SplineParams' scales, offsets and rolls.
    params: SplineSlice,
    id: CMeshId,
    segs: Vec<Seg>,
}

/// A whip's Cable: the component's place under the actor (its relative location turned and scaled by its parents),
/// rotation and scale; the end's follow; the last world tick it ticked.
#[derive(Debug)]
struct Whip {
    c: Cable,
    id: CMeshId,
    end: Follow,
    tick: i32,
    rel: V3,
    rot: Rows,
    scale: V3,
}

const STEP: f64 = 1.0 / 60.0;
/// The Reveal timeline's length, in ticks.
const REVEAL_TICKS: i32 = 42;

fn yaw_rows(facing: f64) -> Rows {
    let h = std::f64::consts::FRAC_PI_2;
    rotation_rows(0.0, if facing > 0.0 { -h } else { h }, 0.0)
}

fn comp<'a>(w: &'a World, owner: FighterId, cls: mvs_sim::names::Name, name: &str) -> Option<&'a Value> {
    actor_def(w, ActorRef::Fighter(owner), cls)?.components.get(name)
}

fn field<'a>(v: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    v?.as_obj()?.get(k)
}

/// The ship mesh component's place at the actor's (x, y), `dz` over its origin in its own scale (shipMeshLoc).
fn ship_mesh_loc(w: &World, owner: FighterId, cls: mvs_sim::names::Name, x: f64, y: f64, dz: f64) -> V3 {
    let c = comp(w, owner, cls, "Ship_Skelmesh");
    let l = xyz(field(c, "RelativeLocation"), 0.0);
    let s = xyz(field(c, "RelativeScale3D"), 1.0);
    let r = rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0);
    let z = l[2] + dz * s[2];
    let mut loc = [0.0, -x, y];
    for (k, v) in loc.iter_mut().enumerate() {
        *v += l[0] * r[k] + l[1] * r[3 + k] + z * r[6 + k];
    }
    loc
}

impl Morty {
    /// C019Marker's visuals. ParticleSystem_0 (PS_C019_SPD_Marker_Air at relative Z 120; BeginPlay sets Team) lives
    /// with the actor. TravelVisuals(false) at StartMoveAlongPath: SpawnEmitterAttached(PS_C019_SPD_Rewind_Looping,
    /// the owner's main mesh, None, scale 1) with Team, destroyed by TravelVisuals(true) when the timeline ends (or at
    /// EndPlay). EndVisuals' PS_C019_SPD_Rewind_Back (ReturnVFX2) is not drawn: K2_DestroyActor follows in the same
    /// frame and ReceiveEndPlay's RemoveTeleportVisuals destroys the component before it ticks.
    fn place_markers(&mut self, fx: &mut Fx, w: &World) {
        let live: Vec<(i32, _)> = w.actors.markers.iter().filter(|(_, m)| !m.dead).map(|(_, m)| (m.seq, m)).collect();
        self.markers.retain(|seq, r| {
            if live.iter().any(|(s, _)| s == seq) {
                return true;
            }
            for h in [r.h, r.travel].into_iter().flatten() {
                fx.player.kill(h);
            }
            if let Some(t) = &r.trail {
                fx.components.remove(t.id);
            }
            false
        });
        for (_, m) in w.actors.markers.iter().filter(|(_, m)| !m.dead) {
            let oid = w.fighter(m.owner).id;
            let team = fx.team(oid);
            let r = self.markers.entry(m.seq).or_insert_with(|| {
                let c = comp(w, m.owner, m.cls, "ParticleSystem_0");
                let l = xyz(field(c, "RelativeLocation"), 0.0);
                // The actor's rotation is the spawner's yaw (90 facing left, else -90): the relative location turns
                // with it.
                let rot = yaw_rows(f64::from(m.facing));
                let mut loc = [0.0, -f64::from(m.x), f64::from(m.y)];
                for (k, v) in loc.iter_mut().enumerate() {
                    *v += l[0] * rot[k] + l[1] * rot[3 + k] + l[2] * rot[6 + k];
                }
                let h = field(c, "Template").and_then(Value::as_str).and_then(|t| {
                    fx.player.spawn_attached(t, &att(loc.map(f64::from), &rot, None), 1.0, &[("Team", team)])
                });
                let trail = new_trail(fx, comp(w, m.owner, m.cls, "NODE_AddSplineMeshComponent-0"), team);
                MarkerFx { h, travel: None, trail }
            });
            if let Some(t) = &mut r.trail {
                // local (X, Y, Z) -> world: X along the facing (world -Y x facing), Y = world X x facing
                let sy = if m.facing > 0.0 { 1.0 } else { -1.0 };
                let ud = field(comp(w, m.owner, m.cls, "NODE_AddSplineMeshComponent-0"), "SplineUpDir");
                let g = |k: &str, d: f64| field(ud, k).and_then(Value::as_f64).unwrap_or(d);
                let up = [g("Y", 0.0) * sy, -g("X", 0.0) * sy, g("Z", 1.0)];
                trail(m, &mut t.segs);
                let slices: Vec<SplineSlice> = t
                    .segs
                    .iter()
                    .map(|s| SplineSlice {
                        a: [0.0, f64::from(-s.ax), f64::from(s.ay)],
                        at: [0.0, f64::from(-s.atx), f64::from(s.aty)],
                        b: [0.0, f64::from(-s.bx), f64::from(s.by)],
                        bt: [0.0, f64::from(-s.btx), f64::from(s.bty)],
                        ..t.params
                    })
                    .collect();
                t.mesh.update(&slices, up);
                if let Some(c) = fx.components.get_mut(t.id) {
                    c.geometry = spline_geometry(&t.src, &t.mesh);
                }
            }
            if m.moving && r.travel.is_none() {
                let f = fx.socket_follow(w, oid, None, [0.0; 3], &IDENTITY, [1.0; 3]);
                r.travel = fx.attach_to_mesh(w, oid, "PS_C019_SPD_Rewind_Looping", "None", f, &[("Team", team)]);
            } else if !m.moving
                && let Some(h) = r.travel.take()
            {
                fx.player.kill(h);
            }
        }
    }

    /// C019_MortarAim's cursor: the PS_C019_AimCursor component (PS_C019_SPF_AimCursor), a child of Aim, so at
    /// relative (CurrentOffset, 0, 0) of the actor with its rotation (the owner's yaw); the construction script sets
    /// its world scale to 1; BeginPlay sets Team. ActivateObject's RemoveCursorVisuals hides it and
    /// K2_DestroyComponent follows. The component's WarmupTime (0.1 s) runs at its activation (the player's).
    fn place_mortars(&mut self, fx: &mut Fx, w: &World) {
        let live: Vec<(i32, _)> =
            w.actors.mortars.iter().filter(|(_, m)| !m.dead && !m.active).map(|(_, m)| (m.seq, m)).collect();
        self.mortars.retain(|seq, h| {
            if live.iter().any(|(s, _)| s == seq) {
                return true;
            }
            if let Some(h) = h {
                fx.player.kill(*h);
            }
            false
        });
        for (seq, mo) in live {
            let a = att([0.0, f64::from(-aim_x(mo)), f64::from(mo.y)], &yaw_rows(f64::from(mo.facing)), Some([1.0; 3]));
            match self.mortars.get(&seq) {
                Some(Some(h)) => fx.player.attach(*h, &a),
                Some(None) => {}
                None => {
                    let team = fx.team(w.fighter(mo.owner).id);
                    let t = field(comp(w, mo.owner, mo.cls, "PS_C019_AimCursor"), "Template").and_then(Value::as_str);
                    let h = t.and_then(|t| fx.player.spawn_attached(t, &a, 1.0, &[("Team", team)]));
                    self.mortars.insert(seq, h);
                }
            }
        }
    }

    /// ProjectileAccelerator SetTeamColor (the parent's BeginPlay): SpawnEmitterAttached(
    /// PS_C019_SPFAIR_Portal_Outro01_Looping, the ParticleSystem component, None, location 0, scale 1,
    /// SnapToTargetIncludingScale) with Team: at the component (relative Z 50, scale (0.4, 0.4, 0.8)) with the
    /// actor's rotation. It ends with the actor.
    fn place_accelerators(&mut self, fx: &mut Fx, w: &World) {
        let live: Vec<(i32, _)> =
            w.actors.accelerators.iter().filter(|(_, a)| !a.dead).map(|(_, a)| (a.seq, a)).collect();
        self.accelerators.retain(|seq, h| {
            if live.iter().any(|(s, _)| s == seq) {
                return true;
            }
            if let Some(h) = h {
                fx.player.kill(*h);
            }
            false
        });
        for (seq, ac) in live {
            if self.accelerators.contains_key(&seq) {
                continue;
            }
            let c = comp(w, ac.owner, ac.cls, "ParticleSystem");
            let l = xyz(field(c, "RelativeLocation"), 0.0);
            let s = xyz(field(c, "RelativeScale3D"), 1.0);
            let a = att([0.0, f64::from(-ac.x), f64::from(ac.y) + l[2]], &yaw_rows(f64::from(ac.facing)), Some(s));
            let team = fx.team(w.fighter(ac.owner).id);
            let h = fx.player.spawn_attached("PS_C019_SPFAIR_Portal_Outro01_Looping", &a, 1.0, &[("Team", team)]);
            self.accelerators.insert(seq, h);
        }
    }

    /// C019_Ship's effects. ReceiveBeginPlay: SpawnEmitterAtLocation(PS_C019_Ship_Portal00, Ship_Skelmesh's location +
    /// (0, 0, 500), rotation 0, scale 1) with Team (0.6 local, else the colour value); the Reveal timeline's end
    /// (TimelineLength 0.7 s): SpawnEmitterAttached(PS_C019_ShipRings, Ship_Skelmesh, None, (0, 0, 105), rotation 0,
    /// scale 1, KeepRelativeOffset), which goes with the actor. The actor's rotation is yaw -90 (BeginPlay's
    /// K2_SetActorRotation) and the mesh component's scale 1.1 scales the rings' relative location.
    // Deviation: the timeline's time is taken as the actor's ticks of 1/60 s, the spawn frame's included, so it ends on
    // the 42nd.
    fn place_ships(&mut self, fx: &mut Fx, w: &World) {
        let live: Vec<(i32, _)> = w.actors.ships.iter().filter(|(_, s)| !s.dead).map(|(_, s)| (s.seq, s)).collect();
        self.ships.retain(|seq, h| {
            if live.iter().any(|(s, _)| s == seq) {
                return true;
            }
            if let Some(h) = h {
                fx.player.kill(*h);
            }
            false
        });
        for (seq, sh) in live {
            if let std::collections::hash_map::Entry::Vacant(e) = self.ships.entry(seq) {
                e.insert(None);
                // where the actor spawned, before the moves of its ticks so far
                let age = f64::from(sh.age);
                let at = ship_mesh_loc(
                    w,
                    sh.owner,
                    sh.cls,
                    f64::from(sh.x) - f64::from(sh.vx) * STEP * age,
                    f64::from(sh.y) - f64::from(sh.vy) * STEP * age,
                    0.0,
                );
                let team = fx.team(w.fighter(sh.owner).id);
                fx.player.spawn(
                    "PS_C019_Ship_Portal00",
                    [at[0], at[1], at[2] + 500.0],
                    [0.0; 3],
                    [1.0; 3],
                    &[("Team", team)],
                );
            }
            let rows = rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0);
            let a = att(ship_mesh_loc(w, sh.owner, sh.cls, f64::from(sh.x), f64::from(sh.y), 105.0), &rows, None);
            match self.ships[&seq] {
                Some(h) => fx.player.attach(h, &a),
                None if sh.age >= REVEAL_TICKS => {
                    let h = fx.player.spawn_attached("PS_C019_ShipRings", &a, 1.0, &[]);
                    self.ships.insert(seq, h);
                }
                None => {}
            }
        }
    }

    /// C019ScrubShield's emitters (the decode's
    /// constants read with kismet_pseudo --extra on a fresh dump). Off a dedicated server: BeginPlay @139
    /// SpawnEmitterAttached(PS_C019_Plumbus_SparkleTrail, Capsule, None, 0, rot 0, scale 1, KeepRelativeOffset), no
    /// Team; each CleanedVisuals @1406 SpawnEmitterAttached(PS_C019_Plumbus_Vacuum_Trail, Capsule, None, 0, rot 0,
    /// scale 1, SnapToTargetIncludingScale) with Team = SelectFloat(0.6, the Instigator's ColorValue, IsLocalOnline)
    /// @1776. Both are the actor's components and go with it (its life is the notify's). A Clean is seen as the sim's
    /// count of the Cleans that answered Success going up. Each CleanedVisuals also spawns @1502
    /// SpawnEmitterAtLocation(PS_C019_Plumbus_Vacuum, the floor trace's Location: the sim's `hits`, rot 0, scale 1,
    /// auto destroy) with the same Team @1834: not the actor's, so it outlives the shield.
    // Unverified: the Capsule's rotation is taken as the actor's place has it (rotation 0, scale 1): the actor
    // rides the owner's Root socket, whose rotation is not followed here.
    fn place_scrubs(&mut self, fx: &mut Fx, w: &World) {
        for (aid, a) in w.actors.actors.iter() {
            let WorldActorState::ScrubShield(sc) = &a.state else { continue };
            if a.dead {
                continue;
            }
            let follow = || Follow::Actor {
                turned: true,
                a: ActorRef::Actor(aid),
                rel: [0.0; 3],
                rel_rot: IDENTITY,
                scale: [1.0; 3],
            };
            let r = match self.scrubs.entry(a.seq) {
                std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::btree_map::Entry::Vacant(e) => {
                    let r = e.insert(Scrub::default());
                    r.hs.extend(fx.spawn_follow(w, "PS_C019_Plumbus_SparkleTrail", follow(), 1.0, &[]));
                    r
                }
            };
            if sc.cleaned > r.cleaned {
                // Unverified: Team from the actor's owner; the graph reads GetInstigator (@1571), 0.0 when it is no
                // fighter (@2018-2041): the port's shield has no instigator of its own, the owner is taken as it.
                let team = fx.team(w.fighter(a.owner).id);
                for k in r.cleaned..sc.cleaned {
                    r.hs.extend(fx.spawn_follow(w, "PS_C019_Plumbus_Vacuum_Trail", follow(), 1.0, &[("Team", team)]));
                    let at = usize::try_from(k).ok().and_then(|k| sc.hits.get(k));
                    if let Some(&(x, y)) = at {
                        fx.player.spawn(
                            "PS_C019_Plumbus_Vacuum",
                            [0.0, f64::from(-x), f64::from(y)],
                            [0.0; 3],
                            [1.0; 3],
                            &[("Team", team)],
                        );
                    }
                }
                r.cleaned = sc.cleaned;
            }
        }
        self.scrubs.retain(|seq, r| {
            let live = w
                .actors
                .actors
                .iter()
                .any(|(_, a)| a.seq == *seq && !a.dead && matches!(a.state, WorldActorState::ScrubShield(_)));
            if live {
                return true;
            }
            for h in r.hs.drain(..) {
                fx.player.kill(h);
            }
            false
        });
    }

    /// Buff_OnExitedArenaTeleportToAlly's Visuals: SpawnEmitterAttached(PS_C019_SPD_RewindIntro_Ally, the owner's
    /// CapsuleComponent, VFX_Root (no such socket on a capsule: the component), location 0, rotation 0, scale 1,
    /// KeepRelativeOffset) with Team (0.6 local, else the colour value); RemoveBuffVisuals destroys it and attaches
    /// PS_C019_SPD_RewindIntro_Ally_Destroyed to the capsule the same way. It is on the buff's owner (each ally of
    /// the caster), never its instigator; the relative location is the capsule's centre, the actor location.
    fn place_rescue(&mut self, fx: &mut Fx, w: &World) {
        for (fid, f) in w.fighters.iter() {
            let id = f.id;
            let born = rescue_vfx_born(w, fid);
            let r = self.rescue.entry(id).or_insert((-1.0, None));
            if f64::from(born) == r.0 {
                continue;
            }
            let team = fx.team(id);
            let at = || Follow::Capsule { id, rel: [0.0; 3], turned: false, cap_up: false };
            if r.0 >= 0.0 {
                if let Some(h) = r.1.take() {
                    fx.player.kill(h);
                }
                fx.spawn_follow(w, "PS_C019_SPD_RewindIntro_Ally_Destroyed", at(), 1.0, &[("Team", team)]);
            }
            r.0 = f64::from(born);
            r.1 = if born >= 0.0 {
                fx.spawn_follow(w, "PS_C019_SPD_RewindIntro_Ally", at(), 1.0, &[("Team", team)])
            } else {
                None
            };
        }
    }
}

/// The marker's NODE_AddSplineMeshComponent-0 components (C019_Marker_Mesh, C019_RewindTrail_MAT with Team, ForwardAxis
/// X, SplineParams' scales, offsets and rolls) as one mesh.
fn new_trail(fx: &mut Fx, st: Option<&Value>, team: f64) -> Option<Trail> {
    let (src, mat) = trail_source(fx, st)?;
    let sp = field(st, "SplineParams");
    let v2 = |k: &str, d: f64| {
        let o = field(sp, k);
        [field(o, "X").and_then(Value::as_f64).unwrap_or(d), field(o, "Y").and_then(Value::as_f64).unwrap_or(d)]
    };
    let roll = |k: &str| field(sp, k).and_then(Value::as_f64).unwrap_or(0.0);
    let params = SplineSlice {
        scale0: v2("StartScale", 1.0),
        scale1: v2("EndScale", 1.0),
        offset0: v2("StartOffset", 0.0),
        offset1: v2("EndOffset", 0.0),
        roll0: roll("StartRoll"),
        roll1: roll("EndRoll"),
        ..SplineSlice::curve([0.0; 3], [0.0; 3], [0.0; 3], [0.0; 3])
    };
    let pos = src.positions.clone();
    let mesh = SplineMesh::new(pos, Vec::new(), Vec::new(), src.indices.clone(), 0, AlphaRule::Extent(src.extent[0]));
    let id = fx.components.add(&trail_material(mat), &[("Team", team)], CMeshGeometry::empty());
    Some(Trail { src, mesh, params, id, segs: Vec::new() })
}

/// The trail component's mesh (read once per match, `Fx::spline_source`) and its material's name.
fn trail_source<'a>(fx: &mut Fx, st: Option<&'a Value>) -> Option<(Arc<SplineSource>, &'a str)> {
    let sm = field(st, "StaticMesh")?.as_str()?;
    let path = object_path(&format!("Panda_Main/Characters/C019/Animations/Trails/{sm}"));
    let src = fx.spline_source(&path).ok()?;
    let mat = field(st, "OverrideMaterials").and_then(Value::as_arr).and_then(|a| a.first()).and_then(Value::as_str)?;
    Some((src, mat))
}

fn trail_material(mat: &str) -> String {
    format!("Panda_Main/Characters/C019/VFX/{mat}#spline")
}

impl CharFxHooks for Morty {
    /// The rewind trail's mesh and material, and the whip's cable (the warm-up, `Fx::warm`).
    fn warm(&mut self, fx: &mut Fx, w: &World, out: &mut Vec<WarmMesh>) {
        let Some(f) = fighter_of(w, mvs_sim::chars::morty::KEY) else { return };
        let st = comp(w, f, name!("C019Marker_C"), "NODE_AddSplineMeshComponent-0");
        if let Some((_, mat)) = trail_source(fx, st) {
            out.push(component_warm(None, &trail_material(mat)));
        }
        // (the whip's class, C019_WhipProjectile_C, is the one of his classes with a Cable)
        let actors = w.char_data(f).fighter.actors.iter().flat_map(|m| m.values());
        for cable in actors.filter_map(|a| a.components.get("Cable")) {
            out.push(component_warm(None, &whip_material(Some(cable))));
        }
    }

    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_markers(fx, w);
        self.place_mortars(fx, w);
        self.place_accelerators(fx, w);
        self.place_ships(fx, w);
        self.place_scrubs(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_rescue(fx, w);
    }

    fn end(&mut self, fx: &mut Fx) {
        for h in std::mem::take(&mut self.scrubs).into_values().flat_map(|s| s.hs) {
            fx.player.kill(h);
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        // The reset drops the markers' and whips' meshes at once (the reset's World has none of their actors).
        for t in self.markers.values().filter_map(|r| r.trail.as_ref()) {
            fx.components.remove(t.id);
        }
        for wh in self.whips.values() {
            fx.components.remove(wh.id);
        }
        *self = Morty::default();
    }

    fn event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        match e {
            Event::MarkerBurst { fighter } => {
                // Unverified: the socket from the last rendered pose.
                let f = fx.socket_follow(w, *fighter, Some("Spine_Socket"), [0.0; 3], &IDENTITY, [1.0; 3]);
                if let Some(a) = fx.place(w, &f) {
                    let team = fx.team(*fighter);
                    fx.player.spawn("PS_C019_NAN_ReturnBurst", a.loc, [0.0; 3], [1.0; 3], &[("Team", team)]);
                }
            }
            // Seismic Boom Visuals: SpawnEmitterAtLocation(PS_C019_SPF_EarthPillar, the location, yaw 180 when the
            // instigator faces left, else 0, scale 1) and (PS_CreatureCrystal_Activate, the actor's location -
            // (0, 0, 100), rotation 0, scale 0.5), both with Team; the beam emitter's template is null.
            Event::MortarBoom { fighter, x, y, ax, ay, facing } => {
                let p = [("Team", fx.team(*fighter))];
                let yaw = if *facing < 0.0 { 180.0 } else { 0.0 };
                fx.player.spawn(
                    "PS_C019_SPF_EarthPillar",
                    [0.0, f64::from(-x), f64::from(*y)],
                    [0.0, yaw, 0.0],
                    [1.0; 3],
                    &p,
                );
                fx.player.spawn(
                    "PS_CreatureCrystal_Activate",
                    [0.0, f64::from(-ax), f64::from(ay - 100.0)],
                    [0.0; 3],
                    [0.5; 3],
                    &p,
                );
            }
            // OverlapVisuals: SpawnEmitterAtLocation(PS_C019_Portal_ProjectileBuff, the location, rotation 0, scale
            // 1); no Team.
            Event::AcceleratorBuff { x, y, .. } => {
                fx.player.spawn(
                    "PS_C019_Portal_ProjectileBuff",
                    [0.0, f64::from(-x), f64::from(*y)],
                    [0.0; 3],
                    [1.0; 3],
                    &[],
                );
            }
            // SpawnedActorParent EndPlay: SpawnEmitterAtLocation(DeathVFX, the actor's location + Death VFX Offset,
            // DeathVFXRotation, DeathVFXScale) with Team; the scale is not uniform here ((0.4, 0.4, 0.8)).
            Event::AcceleratorEnd { fighter, x, y } => {
                let Some(fid) = w.by_id(*fighter) else { return true };
                let def = w.char_data(fid).fighter.actors.as_ref().and_then(|a| a.get("ProjectileAccelerator_C"));
                let p = def.map(|d| &d.p);
                let g = |k: &str| p.and_then(|p| p.get(k));
                let o = xyz(g("Death VFX Offset"), 0.0);
                let s = xyz(g("DeathVFXScale"), 1.0);
                let r = g("DeathVFXRotation").and_then(Value::as_obj);
                let d = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_f64).unwrap_or(0.0).to_radians();
                let rows = rotation_rows(d("Pitch"), d("Yaw"), d("Roll"));
                if let Some(ps) = g("DeathVFX").and_then(Value::as_str) {
                    let team = fx.team(*fighter);
                    let a = att([o[0], f64::from(-x), f64::from(y + mvs_sim::math::narrow(o[2]))], &rows, Some(s));
                    fx.player.spawn_attached(ps, &a, 1.0, &[("Team", team)]);
                }
            }
            _ => return false,
        }
        true
    }
}

/// The classes' own effects, by class name.
fn me(c: &mut CharFx) -> &mut Morty {
    match c {
        CharFx::Morty(m) => m,
        _ => unreachable!("a Morty rule entered by another module"),
    }
}

fn me_ref(c: &CharFx) -> &Morty {
    match c {
        CharFx::Morty(m) => m,
        _ => unreachable!("a Morty rule entered by another module"),
    }
}

// ---- C019_CookGrenade -------------------------------------------------------------------

/// BeginPlay's CustomTimeDilation = 5 / GrenadeLifetime on the PS_Explosion_SatchelAura component.
fn grenade_made(_c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let aura = actor_def(w, ActorRef::Projectile(st.id), pr.cls)
        .is_some_and(|d| d.components.contains_key("PS_Explosion_SatchelAura"));
    let life = class_p(w, pr).get("GrenadeLifetime").and_then(Value::as_f64).unwrap_or(0.0);
    if aura
        && life != 0.0
        && let Some(h) = st.h
    {
        fx.player.set_time_dilation(h, 5.0 / life);
    }
}

/// PS_DamageVisual's template and place: its relative transform under its attach parents (the mesh: Z 13.49, scale
/// 2.25), the mesh's world rotation (lookPitch, lookYaw) on top, all in the actor's scale.
fn kb_at(w: &World, pr: &Projectile, st: &ProjFxState) -> Option<(String, Attachment)> {
    let cs = &actor_def(w, ActorRef::Projectile(st.id), pr.cls)?.components;
    let dv = cs.get("PS_DamageVisual");
    let t = field(dv, "Template")?.as_str()?.to_owned();
    let (l, s) = (xyz(field(dv, "RelativeLocation"), 0.0), xyz(field(dv, "RelativeScale3D"), 1.0));
    let par = attach_parent(cs, "PS_DamageVisual");
    let rel: V3 = [l[0] * f64::from(par.scale[0]), l[1] * f64::from(par.scale[1]), l[2] * f64::from(par.scale[2])];
    let k = pr.scale;
    let rot = rotation_rows(f64::from(pr.look_pitch.to_radians()), f64::from(pr.look_yaw.to_radians()), 0.0);
    let mut loc: V3 = [f64::from(par.loc[0] * k), -st.x + f64::from(par.loc[1] * k), st.y + f64::from(par.loc[2] * k)];
    for (i, v) in loc.iter_mut().enumerate() {
        *v += f64::from(k) * (rel[0] * rot[i] + rel[1] * rot[3 + i] + rel[2] * rot[6 + i]);
    }
    let scale = [
        s[0] * f64::from(par.scale[0]) * f64::from(k),
        s[1] * f64::from(par.scale[1]) * f64::from(k),
        s[2] * f64::from(par.scale[2]) * f64::from(k),
    ];
    Some((t, att(loc, &rot, Some(scale))))
}

/// 'Grenade KB Visuals': the PS_DamageVisual component (PS_ShaggySandwichProjectile, hidden, Team set in BeginPlay)
/// is shown while the ally-hit hitbox is on.
// Unverified: a hidden component is taken as not simulating (started anew when shown).
// Unverified: placed at the actor's place of the latest tick, not following the frame's.
fn grenade_after(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let Some((t, a)) = (if pr.grenade { kb_at(w, pr, st) } else { None }) else { return };
    let on = hitbox_active(pr);
    let team = fx.team(pr.owner_id);
    let kb = me(c).kb.entry(st.seq).or_insert(None);
    match (on, *kb) {
        (false, Some(h)) => {
            fx.player.kill(h);
            *kb = None;
        }
        (true, None) => {
            *kb = fx.player.spawn_attached(&t, &a, 1.0, &[("Team", team)]);
            if kb.is_none() {
                warn_once(format!("Morty: the grenade's {t} is not loaded"));
            }
        }
        (true, Some(h)) => fx.player.attach(h, &a),
        (false, None) => {}
    }
}

fn grenade_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(Some(h)) = me(c).kb.remove(&st.seq) {
        fx.player.kill(h);
    }
}

/// PS_DamageVisual, for a reflect's pause.
fn grenade_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c).kb.get(&st.seq).copied().flatten().into_iter().collect()
}

const GRENADE: ProjRule = ProjRule {
    made: Some(grenade_made),
    after: Some(grenade_after),
    end: Some(grenade_end),
    systems: Some(grenade_systems),
    ..ProjRule::NONE
};

/// The whip cable's component material key (its OverrideMaterials' first, else C019_PlasmaWhip).
fn whip_material(cable: Option<&Value>) -> String {
    let mat = field(cable, "OverrideMaterials")
        .and_then(Value::as_arr)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or("C019_PlasmaWhip");
    format!("Panda_Main/Characters/C019/VFX/{mat}#local")
}

/// C019_WhipProjectile's Cable (UCableComponent, C019_PlasmaWhip; nothing gives the material a 'Team Tint', the
/// parameter TetherPointColor sets on a DMI that is never made): from the component to the owner's main mesh at
/// lightWhipRoot (BeginPlay's SetAttachEndToComponent). The component hangs under Cylinder (pitch 90, scale
/// (0.5, 0.5, 1), under DamageBox): its relative location (101.03, 45.66, 30.11) is scaled and turned to
/// (-30.11, 22.83, 50.51) of the actor, its rotation is the Cylinder's (local X up) and its scale (2, 2, 1) x the
/// Cylinder's = 1, so the tube is CableWidth wide. One component tick per world tick (8 at most a step).
// Unverified: the first tick is the frame after the spawn, with the end already attached.
fn whip_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let cs = actor_def(w, ActorRef::Projectile(st.id), pr.cls).map(|d| &d.components);
    let Some(cd) = cs.and_then(|cs| cs.get("Cable")).filter(|_| pr.whip) else { return };
    let me = me(c);
    if let std::collections::btree_map::Entry::Vacant(e) = me.whips.entry(st.seq) {
        let key = whip_material(Some(cd));
        let par = attach_parent(cs.expect("the class's components"), "Cable");
        let prot: Rows = std::array::from_fn(|k| f64::from(par.rot[k]));
        let rl = xyz(field(Some(cd), "RelativeLocation"), 0.0);
        let rs = xyz(field(Some(cd), "RelativeScale3D"), 1.0);
        let cl = [0, 1, 2].map(|k| f64::from(par.scale[k]) * rl[k]);
        let rel =
            [0, 1, 2].map(|k| f64::from(par.loc[k]) + cl[0] * prot[k] + cl[1] * prot[3 + k] + cl[2] * prot[6 + k]);
        let cable = Cable::new(&CableDef::of(Some(cd)));
        let id = fx.components.add(&key, &[], CMeshGeometry::empty());
        let end = fx.socket_follow(w, pr.owner_id, Some("lightWhipRoot"), [0.0; 3], &IDENTITY, [1.0; 3]);
        let scale = [0, 1, 2].map(|k| f64::from(par.scale[k]) * rs[k]);
        e.insert(Whip {
            c: cable,
            id,
            end,
            tick: w.flow.tick - 1,
            rel: rel.map(f64::from),
            rot: prot,
            scale: scale.map(f64::from),
        });
    }
    let wh = me.whips.get_mut(&st.seq).expect("made above");
    let arot = mvs_view::actors::actor_rot(w, pr);
    let mut start: V3 = [0.0, f64::from(-pr.x), f64::from(pr.y)];
    for (k, v) in start.iter_mut().enumerate() {
        *v += f64::from(pr.scale) * (wh.rel[0] * arot[k] + wh.rel[1] * arot[3 + k] + wh.rel[2] * arot[6 + k]);
    }
    let rot = compose_rows(&wh.rot, &arot);
    let n = (w.flow.tick - wh.tick).min(8);
    // (an end always places; an end not placed here keeps its ticks for the next frame)
    let Some(end) = fx.place(w, &wh.end).map(|a| a.loc) else { return };
    wh.tick = w.flow.tick;
    for _ in 0..n {
        wh.c.tick(STEP, start, end);
    }
    wh.c.build(start, &rot, wh.scale.map(|s| s * f64::from(pr.scale)));
    if let Some(m) = fx.components.get_mut(wh.id) {
        m.geometry = CMeshGeometry::own(wh.c.positions().to_vec(), wh.c.uvs().to_vec(), wh.c.indices().to_vec());
    }
}

fn whip_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(wh) = me(c).whips.remove(&st.seq) {
        fx.components.remove(wh.id);
    }
}

/// C019_WhipProjectile: nothing sets its Team.
const WHIP: ProjRule = ProjRule { no_team: true, tick: Some(whip_tick), end: Some(whip_end), ..ProjRule::NONE };

// ---- C019BlasterChargeProjectile ----------------------------------------------------------------

/// UpdateVisualsMulticast (after a grenade was set off): ParticleSystem.SetRelativeScale3D(1.2) in place of the
/// component's own scale.
fn blaster_scale(_w: &World, pr: &Projectile, ms: V3) -> Option<V3> {
    pr.bomb_visual.then(|| [ms[0] * 1.2, ms[1] * 1.2, ms[2] * 1.2])
}

fn blaster_made(c: &mut CharFx, _fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    me(c).blasters.insert(st.seq, (pr.owner_id, pr.reflect.as_ref().map_or(0, |r| r.pause)));
}

/// MulticastReflect @4068 -> @1549 IsValid(ParticleSystem) -> @1511 ParticleSystem.Activate(true) on every reflect (not
/// behind SpawnedActorParent's pause gate): PS_C019_ElectricityProjectile01 starts again, with the new owner's Team.
// Unverified: a reflect is seen as a new owner or as the reflect pause opening (ReflectState.pause leaving 0), as
// Rick's blaster shots; a second reflect by the same owner inside the pause is missed.
fn blaster_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let Some(o) = me(c).blasters.get_mut(&st.seq) else { return };
    let pause = pr.reflect.as_ref().map_or(0, |r| r.pause);
    let reflected = pr.owner_id != o.0 || (pause != 0 && o.1 == 0);
    o.1 = pause;
    if reflected {
        o.0 = pr.owner_id;
        fx.remake(w, st);
    }
}

fn blaster_end(c: &mut CharFx, _fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    me(c).blasters.remove(&st.seq);
}

const BLASTER: ProjRule = ProjRule {
    scale: Some(blaster_scale),
    made: Some(blaster_made),
    tick: Some(blaster_tick),
    end: Some(blaster_end),
    ..ProjRule::NONE
};

pub(crate) const RULES: &[(&str, ProjRule)] = &[
    ("C019_CookGrenade_C", GRENADE),
    ("C019_CookGrenade_NoGrav_C", GRENADE),
    ("c019_CookGrenade_Split_C", GRENADE),
    ("C019_WhipProjectile_C", WHIP),
    ("C019BlasterChargeProjectile_C", BLASTER),
];
