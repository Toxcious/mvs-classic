//! The Court's two walls (m010_wall_C m010_wall2_5 / m010_wall_2) drawn from their sim state (stages/the_court/wall.rs:
//! the World's stage run actors, read each tick through `SceneryHost::world`).
//! The data: the block `court`'s `walls`; the graph: m010_wall (offsets its ubergraph's).
//! - The barrier ambience: BeginPlay's PF_Post_Event_On_AkComponent(Ak, amb_map_M010_emit_barrier) @1374 at the Ak
//!   component's place, posted at the match's first tick (the scenery's sound hook is set after its begin). The Ak
//!   component moves with the root; the post keeps its first place (the root is back there 1 s after each respawn).
//! - ConnectedFloorGlow (M010_WallFloorGlow_2 / _3) shown while the sim's `glow` (hidden @2037 at the break, shown @15
//!   when WallMove ends); it does not move.
//! - DamageVFXMulticast @1582-1843: PS_M010_WallImpact at the root + (0, 0, 1000) @1746 with the actor's
//!   DamageVFXRotation, scale 1, at each `hit_at`.
//! - The touch @569-1067: PS_M010_Barrier_Touch at SweepResult.ImpactPoint (the sim's touch_x / touch_y; its UE X is
//!   the play plane's 0, and an empty result's (0, 0, 0) is the same), rotation MakeRotator(0, 0, -yaw) (@857: roll,
//!   pitch, yaw) of ConnectedBarrierWall's world yaw (its instance's, about +90 on both after the BeginPlay attach:
//!   the effect's yaw about -90), scale 1, at each `touch_at`.
//!   (Drawn as the data says: both touches face UE -Y, the right wall's away from the play area and culled.)
//! - ConnectedBarrierWall (SM_Barrier_Side_L / _R, M010_Barrier_Side_MESH_MAT: lit surface translucency), attached to
//!   the root at BeginPlay: drawn where the root's WallCollision solid is (its offset from the level place at the
//!   frame's alpha, platform_blend.rs, as the platforms: the teleport and WallMove), with a material instance of its
//!   own whose Damage (@1582, @2273) and Dissolve (@105 by BarrierDissolveTimeline, @2201) are the sim's (`damage`,
//!   `dissolve`), written when they change. The graph never hides it (its material's Dissolve 1 does); the port hides
//!   it once the wall is broken and fully dissolved, which draws the same and spares its pixels (it is shown again at
//!   the respawn, Dissolve 0).
//! - The spawns: SpawnEmitterAtLocation from a hit or an overlap, first ticked the next frame; a stamp seen FX_LATE
//!   ticks late or more (a rollback that played it again) spawns nothing.

// Unverified: the barrier ambience is posted once at the match's first tick at the Ak component's start place and never
// moved by the teleport or WallMove (the build's Ak component rides the root).
// Deviation: the barrier mesh is hidden at `!on && dissolve >= 1`, where the build only sets Dissolve 1: the same picture
// because T_Ramp_WigglyBarEdge01 is black past the ramp (read from the picture, not the texels).
// Unverified: the touch's yaw is -(the barrier's world yaw), as the data says (m010_wall @569-1067); the left wall's faces
// the play area (edge-on), the right wall's away from it.

use mvs_sim::stages::the_court::M010Wall;
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_vfx::Attachment;
use serde::Deserialize;

use super::rows_of;
use crate::Result;
use crate::scenery::host::{InstanceId, SceneryHost, stage_actor};
use crate::scenery::platform_blend::PlatformBlend;
use crate::stage::{bad, transform};
use crate::view::mul;

const FX_LATE: i32 = 6;
/// @1746: Add_FloatFloat(Z, 1000).
const IMPACT_UP: f64 = 1000.0;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WallBlock {
    pub(super) actor: String,
    pub(super) barrier: String,
    pub(super) floor_glow: String,
    pub(super) material: String,
    touch: String,
    impact: String,
    root: [f64; 3],
    ak: [f64; 3],
    impact_rot: [f64; 3],
    ambience: String,
}

#[derive(Debug)]
struct Wall {
    d: WallBlock,
    glow: Vec<InstanceId>,
    barrier: Vec<InstanceId>,
    /// The barrier's slots of its own material.
    slots: Vec<usize>,
    /// Each barrier instance's UE transform as placed.
    base: Vec<mvs_view::M4>,
    /// The values its material instance was last written with.
    damage: f64,
    dissolve: f64,
    barrier_shown: bool,
    blend: PlatformBlend,
    /// The barrier's drawn offset (UE Y, Z).
    drawn: (f64, f64),
    /// The level's own state: the glow actors are visible.
    shown: bool,
    hit_at: i32,
    touch_at: i32,
    impact_at: Attachment,
    touch_place: Attachment,
}

#[derive(Debug)]
pub(super) struct Walls {
    walls: Vec<Wall>,
    /// The ambience is posted in this match.
    ambience: bool,
}

impl Walls {
    pub(super) fn make(blocks: Vec<WallBlock>, host: &SceneryHost) -> Result<Walls> {
        let what = "scenery: the court";
        let mut walls = Vec::new();
        for d in blocks {
            let by_name = |name: &str| -> Vec<InstanceId> {
                (0..host.instances().len())
                    .filter(|&k| host.instances()[k].name == name)
                    .map(|k| InstanceId(k as u32))
                    .collect()
            };
            let barrier = by_name(&d.barrier);
            let glow = by_name(&d.floor_glow);
            if barrier.is_empty() || glow.is_empty() {
                return Err(bad(what, format!("m010 wall {}: no barrier or floor glow instance", d.actor)));
            }
            let first = &host.instances()[barrier[0].0 as usize];
            let slots: Vec<usize> = (0..first.materials.len())
                .filter(|&k| first.materials[k].as_deref().is_some_and(|m| m.split('?').next() == Some(&d.material)))
                .collect();
            if slots.is_empty() {
                return Err(bad(what, format!("wall {}: the barrier has no {}", d.actor, d.material)));
            }
            // (K2_GetActorRotation of the barrier after the attach: its instance's rotator, [pitch, yaw, roll])
            let yaw = first.rot[1];
            let base = barrier
                .iter()
                .map(|id| {
                    let i = &host.instances()[id.0 as usize];
                    transform(i.loc, i.rot, i.scale)
                })
                .collect();
            let at = |loc: [f64; 3], rot: [f64; 3]| Attachment { loc, rot: rows_of(rot), scale: Some([1.0; 3]) };
            walls.push(Wall {
                impact_at: at([d.root[0], d.root[1], d.root[2] + IMPACT_UP], d.impact_rot),
                touch_place: at([0.0; 3], [0.0, -yaw, 0.0]),
                d,
                glow,
                barrier,
                slots,
                base,
                damage: 0.0,
                dissolve: 0.0,
                barrier_shown: true,
                blend: PlatformBlend::default(),
                drawn: (0.0, 0.0),
                shown: true,
                hit_at: -1,
                touch_at: -1,
            });
        }
        Ok(Walls { walls, ambience: false })
    }

    pub(super) fn begin(&mut self, host: &mut SceneryHost) {
        self.ambience = false;
        for v in &mut self.walls {
            v.hit_at = -1;
            v.touch_at = -1;
            if !v.shown {
                v.shown = true;
                host.show(&v.glow, true);
            }
            v.place(host, 0.0, 0.0);
            v.blend = PlatformBlend::default();
            // (the material instance is made at the first write: Damage and Dissolve 0)
            v.write(host, 0.0, 0.0, true);
            if !v.barrier_shown {
                v.barrier_shown = true;
                host.show(&v.barrier, true);
            }
        }
    }

    pub(super) fn tick(&mut self, host: &mut SceneryHost) {
        let Some(w) = host.world() else { return };
        if w.stage_run.is_none() {
            return;
        }
        let tick = w.flow.tick;
        let post = !self.ambience;
        self.ambience = true;
        for v in &mut self.walls {
            if post {
                host.sound(&v.d.ambience, v.d.ak, Some(&format!("m010wall:{}", v.d.actor)));
            }
            let placed = w.data.stage.placed.as_deref().unwrap_or(&[]).iter().position(|p| p.name == v.d.actor);
            let Some(a) = placed.and_then(|i| stage_actor(w, i as u32)) else { continue };
            let StageActorOwn::M010Wall(m) = &a.own else {
                panic!("scenery: {} is not an m010 wall of the sim", v.d.actor)
            };
            let run = w.stage_run.as_ref().expect("a stage run");
            let solid = usize::try_from(m.floor).ok().and_then(|f| run.solids.get(f));
            // (sim x = -UE Y, sim y = UE Z)
            let floor = solid.map(|s| (-f64::from(s.m.dx), f64::from(s.m.dy), (f64::from(s.m.mx), f64::from(s.m.my))));
            let s = Seen::of(m, floor);
            if s.glow != v.shown {
                v.shown = s.glow;
                host.show(&v.glow, s.glow);
            }
            match s.floor {
                Some((dy, dz, run)) => v.blend.tick(dy, dz, Some(run)),
                None => v.blend.tick(0.0, 0.0, None),
            }
            // (RespawnWall's K2_TeleportTo @2346 puts the root 2000 out at once: not drawn on the way)
            if s.respawn_at == tick {
                v.blend.py = v.blend.cy;
                v.blend.pz = v.blend.cz;
            }
            v.write(host, s.damage, s.dissolve, false);
            let shown = s.on || s.dissolve < 1.0;
            if shown != v.barrier_shown {
                v.barrier_shown = shown;
                host.show(&v.barrier, shown);
            }
            if s.hit_at != v.hit_at {
                v.hit_at = s.hit_at;
                if s.hit_at >= 0 && tick - s.hit_at < FX_LATE {
                    // (the root where it is now: only its Y moves)
                    v.impact_at.loc[1] = s.uy;
                    if let Some(vfx) = host.vfx() {
                        vfx.spawn_level(&v.d.impact, &v.impact_at, true, &[], 0, false);
                    }
                }
            }
            if s.touch_at != v.touch_at {
                v.touch_at = s.touch_at;
                if s.touch_at >= 0 && tick - s.touch_at < FX_LATE {
                    v.touch_place.loc = [0.0, -s.touch_x, s.touch_y];
                    if let Some(vfx) = host.vfx() {
                        vfx.spawn_level(&v.d.touch, &v.touch_place, true, &[], 0, false);
                    }
                }
            }
        }
    }

    pub(super) fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        for v in &mut self.walls {
            let (dy, dz) = v.blend.at(alpha);
            v.place(host, dy, dz);
        }
    }
}

/// What one wall's tick copies of the sim's M010Wall.
struct Seen {
    glow: bool,
    on: bool,
    damage: f64,
    dissolve: f64,
    hit_at: i32,
    touch_at: i32,
    respawn_at: i32,
    uy: f64,
    touch_x: f64,
    touch_y: f64,
    /// The WallCollision solid's offset (UE Y, Z) and its run's move (sim x, y).
    floor: Option<(f64, f64, (f64, f64))>,
}

impl Seen {
    fn of(m: &M010Wall, floor: Option<(f64, f64, (f64, f64))>) -> Seen {
        Seen {
            glow: m.glow,
            on: m.on,
            damage: f64::from(m.damage),
            dissolve: f64::from(m.dissolve),
            hit_at: m.hit_at,
            touch_at: m.touch_at,
            respawn_at: m.respawn_at,
            uy: f64::from(m.uy),
            touch_x: f64::from(m.touch_x),
            touch_y: f64::from(m.touch_y),
            floor,
        }
    }
}

impl Wall {
    /// The barrier at its offset (UE Y, Z) from the level place, where it is not already.
    fn place(&mut self, host: &mut SceneryHost, dy: f64, dz: f64) {
        if (dy, dz) == self.drawn {
            return;
        }
        self.drawn = (dy, dz);
        let t = transform([0.0, dy, dz], [0.0; 3], [1.0; 3]);
        for (id, base) in self.barrier.iter().zip(&self.base) {
            host.place(*id, Some(mul(base, &t)));
        }
    }

    /// Its material instance's Damage / Dissolve, written when they change (`force`: at the begin).
    fn write(&mut self, host: &mut SceneryHost, damage: f64, dissolve: f64, force: bool) {
        if !force && damage == self.damage && dissolve == self.dissolve {
            return;
        }
        self.damage = damage;
        self.dissolve = dissolve;
        for id in &self.barrier {
            for &slot in &self.slots {
                host.vfx_material_scalars(*id, slot, &[("Damage", damage as f32), ("Dissolve", dissolve as f32)]);
            }
        }
    }
}
