//! Tree Fort's stage actors: the `treefortActors` block. The destructible
//! bounce trees (TreehouseSidePillar_C / _Child_C) drawn from their sim state, the upper platform's plank wiggle
//! (TreehouseUpperPlatform_C, _1v1_C) and the small platforms' hit animation (TreehouseSmallPlatform_C and its kin).
//!
//! The tree (offsets of ExecuteUbergraph_TreehouseSidePillar):
//! - its mesh by damageStage: stage 0 the placed SkeletalMesh, 1 and 2 DamageMeshStates[stage] with
//!   DamageMaterialStates[stage] on slot 1 (UpdateDamageMesh: SetSkeletalMesh @7320 with reinitialize-pose false,
//!   SetMaterial(1) @7425). The damage meshes are made at make (SceneryHost::skinned) and posed with the placed
//!   mesh's pose: one AnimBP instance whatever mesh it draws.
//! - Side_Tree_AnimBp_C: DefaultSlot over Side_Tree_Idle (rate 0.35, looping), ApplyAdditive of the "Additive" slot
//!   over that, then the five ModifyBone nodes. The slots share DefaultGroup (L_Tree_Base_Skeleton SlotGroups), so a
//!   montage played stops the other (Montage_Play's bStopAllMontages, blend 0 both): the death montage (@4605) is the
//!   one left on the tree's last hit, which plays the hit montage first. SideTree_Hit_AnimMontage on "Additive":
//!   Side_Tree_Hit, AAT_LocalSpaceBase over Side_Tree_Death_Left's frame 0 (its RefPoseSeq), applied over the slot's
//!   base pose. Side_Tree_Death_Left / _Right_Montage on DefaultSlot: RateScale 0.75 over a segment at rate 0.5, cut
//!   by K2_DestroyActor 100 frames in (`dead`: nothing is drawn from then on). A montage's position: the ticks since
//!   the tick it was played (hit_at, dying_at) plus that tick's own, x 1/60 x its RateScale (the anim update follows
//!   the actor's tick in its frame).
//! - The ModifyBone nodes: Bones[1..4] and [7] replace their bone's translation in its own bone space (BCS_BoneSpace,
//!   BMM_Replace): the bone moves by the vector turned by its own rotation and scale, its children with it. The sim
//!   keeps Bones (Pillar::bones).
//! - DeathVisuals (@3679..4605): Ball, Bucket and Flag hidden; CreateDynamicMaterialInstance(0 and 1,
//!   DamageMaterialStates[2]) with Tint 0.5 on the mesh of that moment (an instance of the tree's own).
//! - The notifies of the two montages: AnimNotify_PlayVFX on the socket Root (attached, scale 1): PS_Treefort_Hit and
//!   PS_Treefort_Destroyed, by the rule of trophys_edge.rs: a notify at a time in (last position, position].
//! - TreeFortPillarDamageVFX (damage_at, @7075): PS_Treefortbounce at TreePillarCollision, scale 1.5;
//!   TreeFortTrampolineBounceVisual (bounce_at, @1485): PS_Treefortbounce at bounce_fx, its pitch, scale 1. Both
//!   SpawnEmitterAtLocation: spawned when the tick changes, first ticked the next frame. A tick the presentation sees
//!   late (several ticks at once online) still spawns within 6 ticks; two bounces in one tick keep the sim's last one
//!   (bounce_fx holds one).
//! - AllowHazards off: BeginPlay's K2_DestroyActor (@5888): the sim's actor is dead from its start: nothing drawn.
//!
//! The upper platform: Box1..12 (BoxExtent the UBoxComponent default 32, at the box's scale; under the SkeletalMesh)
//! begin an overlap with a fighter: SetWiggle(fighter, N) (@2258..2577) adds a record (N, sign -1 when the fighter's
//! actor Z >= the platform's, else +1, the time) unless a record holds N. The platform's Tick (@15..2081): a record
//! past 0.5 s sets Bones[N] to zero and goes; else Bones[N] = (0, 0, sign x 2.5 x TreePlankWiggleCurve(age)). The
//! AnimBP (Treehouse_TopPlatformSkelMesh_Skeleton_AnimBlueprint_C / Treehouse_TopPlatform_AnimPB_C): its idle sequence
//! (or the reference pose) and the twelve ModifyBone nodes, as the tree's. A record made in a tick is first read by
//! the Tick of the next (the platform ticks ahead of the fighters' movement).
//!
//! The small platforms (TreehouseSmallPlatform_C, _Child_C, M002_TreehouseSmallPlatform_Night_01_C, one graph):
//! BeginPlay binds TreeFortPlatformHit to OnActorHit (@68..91), which a blocking hit of the platform's Box dispatches
//! (the mover's side first, then the platform's: AActor::DispatchBlockingHit). With a BaseFighter as the other actor
//! (@111..176): TreeFortSmallPlatformSkelMesh.PlayAnimation(TreeFortSmallPlatformHitAnim, false) (@186),
//! PF_Post_Event_At_Location with a null event (@278: Lib_Audio's IsValid(AkEvent) fails, nothing is posted),
//! Delay(0.8) (@356; a call while it is pending is ignored), resuming at @15:
//! PlayAnimation(TreeFortSmallPlatformIdleAnim, true). Until the first hit the component's own animation plays.

use std::collections::BTreeMap;
use std::sync::Arc;

use mvs_sim::World;
use mvs_sim::fighter::body::{PlatformRef, actor_y, cap_radius, capsule_half};
use mvs_sim::stages::tree_fort::Pillar;
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_vfx::{Attachment, SystemId};
use mvs_view::anim::{Pose, Rig, apply_additive, sample_clip};
use serde::Deserialize;
use serde_json::Value;

use super::super::host::{SceneryHost, SkinRef, SkinnedId};
use super::super::sky_arena::attachment_of;
use super::super::{CurveKey, rich_curve};
use crate::Result;
use crate::stage::bad;

const STEP: f64 = 1.0 / 60.0;
const DEG: f64 = std::f64::consts::PI / 180.0;
/// Ticks: a bounce or damage tick seen this late still spawns its effect.
const FX_LATE: i32 = 6;
/// The small platform's Delay(0.8) (@356).
const SMALL_DELAY: f64 = 0.8;

// ---- the block's data (the importer's scenery part tree_fort.rs, `treefortActors`)

#[derive(Debug, Deserialize)]
struct Segment {
    anim: String,
    pos: f64,
    start: f64,
    end: f64,
    rate: f64,
}

#[derive(Debug, Deserialize)]
struct Notify {
    at: f64,
    end: Option<f64>,
    system: String,
    socket: Option<String>,
    #[serde(default)]
    kill: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Montage {
    montage: String,
    length: f64,
    rate_scale: f64,
    segments: Vec<Segment>,
    notifies: Vec<Notify>,
}

#[derive(Debug, Deserialize)]
struct ModifyBone {
    slot: usize,
    bone: String,
}

#[derive(Debug, Deserialize)]
struct Modify {
    slots: usize,
    modify: Vec<ModifyBone>,
}

#[derive(Debug, Deserialize)]
struct Damage {
    mesh: String,
    materials: Vec<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct DeathMaterial {
    slots: Vec<usize>,
    material: String,
    scalars: BTreeMap<String, f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PillarData {
    actor: String,
    mesh: String,
    ball: Option<String>,
    bucket: Option<String>,
    flag: Option<String>,
    damage: Vec<Damage>,
    hit: Montage,
    death: Montage,
    bones: Modify,
    death_material: DeathMaterial,
    bounce: String,
    collision: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct WiggleBox {
    n: usize,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct Wiggle {
    #[serde(flatten)]
    modify: Modify,
    boxes: Vec<WiggleBox>,
    extent: f64,
    z: f64,
    curve: Vec<CurveKey>,
    scale: f64,
    life: f64,
}

#[derive(Debug, Deserialize)]
struct PlatformData {
    actor: String,
    kind: String,
    skeletal: String,
    hit: Option<String>,
    idle: Option<String>,
    wiggle: Option<Wiggle>,
}

#[derive(Debug, Deserialize)]
struct ActorsBlock {
    pillars: Vec<PillarData>,
    platforms: Vec<PlatformData>,
}

// ---- the AnimBPs' shared parts

/// A skeletal mesh's ModifyBone nodes: each node's bone and its Bones slot.
#[derive(Debug)]
struct ModifyIndex {
    index: Vec<usize>,
    slots: Vec<usize>,
}

fn modify_index(names: &[String], m: &Modify, what: &str) -> Result<ModifyIndex> {
    let mut index = Vec::new();
    for x in &m.modify {
        let i = names.iter().position(|n| *n == x.bone).ok_or_else(|| bad(what, format!("no bone {}", x.bone)))?;
        index.push(i);
    }
    Ok(ModifyIndex { index, slots: m.modify.iter().map(|x| x.slot).collect() })
}

/// Bones[slot] (UE, bone space) added to each modified bone's local translation: the vector in the pose's axes
/// (x, z, y) x 0.01, through the bone's local scale and rotation.
// Deviation: the bone's parent is taken at uniform scale (the local rotation and scale turn the vector).
fn modify_bones(pose: &mut Pose, m: &ModifyIndex, v: &[f32]) {
    for (&b, &slot) in m.index.iter().zip(&m.slots) {
        let o = slot * 3;
        let at = |k: usize| v.get(k).map_or(0.0, |x| f64::from(*x));
        let (x, y, z) = (at(o), at(o + 1), at(o + 2));
        // (a NaN counts as none, as `!x` does)
        let none = |a: f64| a == 0.0 || a.is_nan();
        if none(x) && none(y) && none(z) {
            continue;
        }
        let s = |k: usize| f64::from(pose.s[b * 3 + k]);
        let (vx, vy, vz) = (x * 0.01 * s(0), z * 0.01 * s(1), y * 0.01 * s(2));
        let [qx, qy, qz, qw] = [0, 1, 2, 3].map(|k| f64::from(pose.r[b * 4 + k]));
        // q v q* (v + 2w (q x v) + 2 q x (q x v))
        let (cx, cy, cz) = (qy * vz - qz * vy, qz * vx - qx * vz, qx * vy - qy * vx);
        let t = &mut pose.t;
        t[b * 3] = (f64::from(t[b * 3]) + vx + 2.0 * (qw * cx + qy * cz - qz * cy)) as f32;
        t[b * 3 + 1] = (f64::from(t[b * 3 + 1]) + vy + 2.0 * (qw * cy + qz * cx - qx * cz)) as f32;
        t[b * 3 + 2] = (f64::from(t[b * 3 + 2]) + vz + 2.0 * (qw * cz + qx * cy - qy * cx)) as f32;
    }
}

/// A montage's segment at a position: the clip and its time.
fn segment_at(m: &Montage, pos: f64) -> (&Segment, f64) {
    let mut s = &m.segments[0];
    for x in &m.segments {
        if pos >= x.pos {
            s = x;
        }
    }
    (s, s.end.min(s.start + (pos - s.pos) * s.rate))
}

/// The scenery's clip `path`, or the make's error.
fn clip_of(host: &SceneryHost, path: &str, what: &str) -> Result<usize> {
    host.state.clips.get(path).ok_or_else(|| bad(what, format!("the animation {path} is not loaded")))
}

/// A skeletal mesh posed by its block: its index, its bones bound to the scenery's clips, its reference pose and the
/// pose the block makes.
#[derive(Debug)]
struct Driven {
    name: String,
    rig: Rig,
    rest: Pose,
    pose: Pose,
}

impl Driven {
    fn make(host: &mut SceneryHost, name: &str) -> Result<Driven> {
        let k = host.skeletal_index(name).ok_or_else(|| bad(name, "not a skeletal mesh of the file"))?;
        let info = host.state.skins[k].info.clone();
        let rig = Rig::bind(&host.state.clips, info.bones().to_vec());
        host.drive(name);
        Ok(Driven { name: name.to_owned(), rig, rest: info.rest.clone(), pose: info.rest.clone() })
    }

    fn names(&self, host: &SceneryHost) -> Vec<String> {
        let k = host.skeletal_index(&self.name).expect("a driven skeletal mesh");
        host.state.skins[k].info.bones().to_vec()
    }

    fn sample(&mut self, host: &mut SceneryHost, c: usize, t: f64) {
        sample_clip(&mut host.state.clips, c, &self.rig, &self.rest, t, &mut self.pose, -1);
    }

    /// The pose written to the skeletal mesh.
    fn write(&self, host: &mut SceneryHost) {
        if let Some(p) = host.pose(&self.name) {
            p.copy(&self.pose);
        }
    }

    /// Its animation's time on one tick (the AnimBP's idle player): by the component's play rate and the clip's
    /// RateScale; the new time.
    fn advance(&self, host: &mut SceneryHost, dt: f64, rate_scale: f64) -> f64 {
        let a = host.skeletal(&self.name).expect("a driven skeletal mesh");
        a.time += dt * a.rate * rate_scale;
        a.time
    }
}

// ---- the trees

/// Ball, Bucket or Flag: shown while the tree is drawn, not dying and not hidden from the start.
#[derive(Debug)]
struct Part {
    name: String,
    hidden: bool,
    on: bool,
}

#[derive(Debug)]
struct Tree {
    d: PillarData,
    skel: Driven,
    /// The idle sequence (the skeletal mesh's own clip).
    idle: usize,
    /// The skeletal mesh of each damage stage past 0 (index 0: the placed one, None).
    sets: Vec<Option<SkinnedId>>,
    parts: Vec<Part>,
    modify: ModifyIndex,
    /// Bones as the sim left them this tick (float32, as the AnimBP's).
    bones: Vec<f32>,
    ta: Pose,
    // drawn
    /// -1: none drawn (the actor is gone).
    stage: i32,
    dying: bool,
    // this tick's montages (-1: none) and what their notifies saw
    hit_pos: f64,
    death_pos: f64,
    hit_start: i32,
    hit_seen: f64,
    death_start: i32,
    death_seen: f64,
    fx: Vec<Option<SystemId>>,
    seen_bounce: i32,
    seen_damage: i32,
    synced: bool,
}

impl Tree {
    fn make(d: PillarData, host: &mut SceneryHost) -> Result<Tree> {
        let what = format!("scenery treefortActors: {}", d.actor);
        let skel = Driven::make(host, &d.mesh)?;
        let names = skel.names(host);
        if !names.iter().any(|n| n == "Root") {
            return Err(bad(&what, format!("{} has no bone Root", d.mesh)));
        }
        let clip = host.skeletal(&d.mesh).and_then(|a| a.clip.clone());
        let idle = clip.ok_or_else(|| bad(&what, format!("{} has no idle sequence", d.mesh)))?;
        let idle = clip_of(host, &idle, &what)?;
        for s in d.hit.segments.iter().chain(&d.death.segments) {
            clip_of(host, &s.anim, &what)?;
        }
        // (R_Tree_Damage_01 / _02's third section, ST_TopRightPlatform_01_M, names no material in the mesh and none
        // in the component's two overrides: the data names the engine's default surface material, WorldGridMaterial,
        // and it is drawn with it, as the data does)
        let mut sets = vec![None];
        for dm in d.damage.iter().skip(1) {
            let mats: Vec<Option<&str>> = dm.materials.iter().map(Option::as_deref).collect();
            sets.push(Some(host.skinned(&d.mesh, &dm.mesh, &mats)?));
        }
        let mut parts = Vec::new();
        for n in [&d.ball, &d.bucket, &d.flag].into_iter().flatten() {
            let k = host.skeletal_index(n).ok_or_else(|| bad(&what, format!("{n} is not a skeletal mesh")))?;
            let hidden = host.skeletals()[k].hidden;
            parts.push(Part { name: n.clone(), hidden, on: !hidden });
        }
        let modify = modify_index(&names, &d.bones, &what)?;
        let n = skel.rest.n;
        Ok(Tree {
            bones: vec![0.0; d.bones.slots * 3],
            fx: vec![None; d.hit.notifies.len() + d.death.notifies.len()],
            d,
            skel,
            idle,
            sets,
            parts,
            modify,
            ta: Pose::new(n),
            stage: 0,
            dying: false,
            hit_pos: -1.0,
            death_pos: -1.0,
            hit_start: -1,
            hit_seen: 0.0,
            death_start: -1,
            death_seen: 0.0,
            seen_bounce: -1,
            seen_damage: -1,
            synced: false,
        })
    }

    fn kill_fx(&mut self, host: &mut SceneryHost) {
        for slot in &mut self.fx {
            if let (Some(id), Some(v)) = (slot.take(), host.vfx()) {
                v.kill(id);
            }
        }
    }

    /// The notifies of the hit (`death` false) or death montage in (from, to].
    // Unverified: a system still attached when the actor is destroyed is killed with it (the component taken as the
    // actor's: SpawnEmitterAttached's outer, from 4.26 source memory).
    fn notifies(&mut self, host: &mut SceneryHost, death: bool, from: f64, to: f64) {
        let (m, first) = if death { (&self.d.death, self.d.hit.notifies.len()) } else { (&self.d.hit, 0) };
        for (k, nt) in m.notifies.iter().enumerate() {
            if nt.at > from && nt.at <= to {
                if nt.socket.as_deref() != Some("Root") {
                    panic!("scenery: {} notify socket {:?}", m.montage, nt.socket);
                }
                let a = attachment_of(&host.bone_world(&self.d.mesh, "Root").expect("the tree's bone Root"));
                if let Some(v) = host.vfx() {
                    if let Some(id) = self.fx[first + k].take() {
                        v.deactivate(id);
                    }
                    self.fx[first + k] = v.spawn_level(&nt.system, &a, false, &[], 0, false);
                }
            }
            if let Some(end) = nt.end
                && nt.kill
                && end > from
                && end <= to
                && let Some(id) = self.fx[first + k].take()
                && let Some(v) = host.vfx()
            {
                v.deactivate(id);
            }
        }
    }

    fn show_stage(&self, host: &mut SceneryHost, stage: i32, on: bool) {
        match stage {
            s if s < 0 => {}
            0 => host.show_skeletal(&self.d.mesh, on),
            s => {
                if let Some(Some(id)) = self.sets.get(s as usize) {
                    host.show_skinned(*id, on);
                }
            }
        }
    }

    /// What is drawn of a tree: its stage's mesh (-1 none), Ball / Bucket / Flag, the death material.
    fn draw(&mut self, host: &mut SceneryHost, stage: i32, dying: bool) {
        if stage != self.stage {
            self.show_stage(host, self.stage, false);
            self.show_stage(host, stage, true);
            self.stage = stage;
        }
        if dying != self.dying {
            self.dying = dying;
            let dm = &self.d.death_material;
            let scalars: Vec<(&str, f32)> = dm.scalars.iter().map(|(k, v)| (k.as_str(), *v as f32)).collect();
            for (s, set) in self.sets.iter().enumerate() {
                let mesh = match set {
                    Some(id) => SkinRef::Own(*id),
                    None => SkinRef::File(&self.d.mesh),
                };
                for &slot in &dm.slots {
                    // (a section the damage mesh does not draw has no material to swap)
                    if s > 0 && self.d.damage[s].materials.get(slot).is_none_or(Option::is_none) {
                        continue;
                    }
                    host.skeletal_material(mesh, slot, dying.then_some(dm.material.as_str()));
                    if dying {
                        host.skeletal_scalars(mesh, slot, &scalars);
                    }
                }
            }
        }
        for part in &mut self.parts {
            let on = stage >= 0 && !dying && !part.hidden;
            if on == part.on {
                continue;
            }
            part.on = on;
            host.show_skeletal(&part.name, on);
        }
    }

    /// SpawnEmitterAtLocation (first ticked the next frame) at `loc` (UE), MakeRotator(pitch, yaw, 0), `scale`.
    fn spawn_at(&self, host: &mut SceneryHost, loc: [f64; 3], pitch: f64, yaw: f64, scale: f64) {
        let r = mvs_vfx::math::rotation_rows(pitch * DEG, yaw * DEG, 0.0);
        let a = Attachment {
            loc,
            rot: [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]],
            scale: Some([scale; 3]),
        };
        if let Some(v) = host.vfx() {
            v.spawn_level(&self.d.bounce, &a, true, &[], 0, false);
        }
    }

    /// The tree's World state this tick.
    fn tick(&mut self, host: &mut SceneryHost, w: &World) {
        let tick = w.flow.tick;
        let placed = w.data.stage.placed.as_deref().unwrap_or(&[]);
        let mut found = None;
        for a in w.stage_run.as_ref().map_or(&[][..], |r| &r.actors[..]) {
            if placed.get(a.index as usize).is_some_and(|p| p.name == self.d.actor) {
                found = Some(a);
            }
        }
        let a: Option<&Pillar> = found.filter(|a| !a.dead).map(|a| match &a.own {
            StageActorOwn::Pillar(p) => p,
            o => panic!("scenery: the stage actor {} is no TreePillar: {o:?}", self.d.actor),
        });
        let Some(a) = a else {
            // destroyed (its death Delay, or BeginPlay with AllowHazards off)
            self.hit_pos = -1.0;
            self.death_pos = -1.0;
            self.kill_fx(host);
            self.draw(host, -1, false);
            self.synced = false;
            return;
        };
        if !self.synced {
            self.seen_bounce = a.bounce_at;
            self.seen_damage = a.damage_at;
            self.hit_start = a.hit_at;
            self.hit_seen = f64::MAX;
            self.death_start = a.dying_at;
            self.death_seen = f64::MAX;
            self.synced = true;
        }
        for (i, b) in self.bones.iter_mut().enumerate() {
            *b = a.bones.get(i).map_or(0.0, |x| *x);
        }
        let dying = a.dying_at >= 0;
        // the death montage, else the hit montage while it plays
        // Unverified: the "+1": the montage taken as advanced in the tick it was played. Montage_Play comes from
        // ReceiveAnyDamage inside the attacker's hit (the attacker's tick), and whether the tree's mesh then updates
        // its anim in that same frame (after the attacker) or first in the next is the tick-group order, not read.
        let (hit, death) = (&self.d.hit, &self.d.death);
        self.death_pos =
            if dying { death.length.min(f64::from(tick - a.dying_at + 1) * STEP * death.rate_scale) } else { -1.0 };
        let hp = if !dying && a.hit_at >= 0 { f64::from(tick - a.hit_at + 1) * STEP * hit.rate_scale } else { -1.0 };
        self.hit_pos = if hp >= 0.0 && hp < hit.length { hp } else { -1.0 };
        // a montage played later than any seen: its notifies from the start; a start put back to an earlier tick or
        // to one already seen (a rollback, a resimulated hit) fires none of the positions already passed
        if a.hit_at > self.hit_start {
            self.hit_start = a.hit_at;
            self.hit_seen = 0.0;
        } else if a.hit_at != self.hit_start {
            self.hit_seen = if self.hit_pos >= 0.0 { self.hit_pos } else { f64::MAX };
        }
        if self.hit_pos >= 0.0 && self.hit_pos > self.hit_seen {
            self.notifies(host, false, self.hit_seen, self.hit_pos);
        }
        if self.hit_pos >= 0.0 {
            self.hit_seen = self.hit_pos;
        }
        if a.dying_at > self.death_start {
            self.death_start = a.dying_at;
            self.death_seen = 0.0;
        } else if a.dying_at != self.death_start {
            self.death_seen = if self.death_pos >= 0.0 { self.death_pos } else { f64::MAX };
        }
        if self.death_pos >= 0.0 && self.death_pos > self.death_seen {
            self.notifies(host, true, self.death_seen, self.death_pos);
        }
        if self.death_pos >= 0.0 {
            self.death_seen = self.death_pos;
        }
        self.draw(host, a.damage_stage, dying);
        // the damage and bounce effects (sim place: x = -UE Y, y = UE Z, z = UE X)
        if a.damage_at != self.seen_damage {
            self.seen_damage = a.damage_at;
            if a.damage_at >= 0 && tick - a.damage_at < FX_LATE {
                self.spawn_at(host, self.d.collision, 0.0, 90.0, 1.5);
            }
        }
        if a.bounce_at != self.seen_bounce {
            self.seen_bounce = a.bounce_at;
            if let Some(fx) = &a.bounce_fx
                && a.bounce_at >= 0
                && tick - a.bounce_at < FX_LATE
            {
                self.spawn_at(
                    host,
                    [f64::from(fx.z), f64::from(-fx.x), f64::from(fx.y)],
                    f64::from(fx.pitch),
                    90.0,
                    1.0,
                );
            }
        }
    }

    /// Side_Tree_AnimBp's pose this tick (the skeletal mesh's drive, after the block's tick): DefaultSlot
    /// (the death montage, else the idle sequence), the "Additive" slot over it, the ModifyBone nodes; the shown
    /// damage mesh takes the same pose. Then the attached systems follow the socket Root (VfxPlayer's follow, before
    /// its tick).
    // Unverified: the "Additive" slot's own source (Side_Tree_AdditiveNull) is taken as no change (as the data module).
    fn pose(&mut self, host: &mut SceneryHost, dt: f64) {
        let (idle_len, idle_rate) = {
            let c = &host.state.clips.clips[self.idle].anim;
            (c.length, c.rate_scale)
        };
        let time = self.skel.advance(host, dt, idle_rate);
        if self.death_pos >= 0.0 {
            let (s, at) = segment_at(&self.d.death, self.death_pos);
            let c = host.state.clips.get(&s.anim).expect("a tree montage's animation");
            self.skel.sample(host, c, at);
        } else {
            self.skel.sample(host, self.idle, time % idle_len);
        }
        if self.hit_pos >= 0.0 {
            let (s, at) = segment_at(&self.d.hit, self.hit_pos);
            let c = host.state.clips.get(&s.anim).expect("a tree montage's animation");
            if !host.state.clips.clips[c].additive {
                panic!("scenery: {} is not additive", self.d.hit.montage);
            }
            let sk = &mut self.skel;
            apply_additive(&mut host.state.clips, c, &sk.rig, &sk.rest, at, &mut sk.pose, &mut self.ta);
        }
        modify_bones(&mut self.skel.pose, &self.modify, &self.bones);
        self.skel.write(host);
        if self.stage > 0
            && let Some(Some(id)) = self.sets.get(self.stage as usize)
        {
            host.pose_skinned(*id).copy(&self.skel.pose);
        }
        if self.fx.iter().any(Option::is_some) {
            let a = attachment_of(&host.bone_world(&self.d.mesh, "Root").expect("the tree's bone Root"));
            if let Some(v) = host.vfx() {
                for slot in &mut self.fx {
                    match *slot {
                        Some(id) if v.alive(id) => v.attach(id, &a),
                        _ => *slot = None,
                    }
                }
            }
        }
    }

    fn begin(&mut self, host: &mut SceneryHost) {
        self.kill_fx(host);
        self.hit_pos = -1.0;
        self.death_pos = -1.0;
        self.synced = false;
        self.draw(host, 0, false);
    }
}

// ---- the upper platforms' planks

#[derive(Debug)]
struct WiggleRecord {
    n: usize,
    sign: f64,
    tick: i32,
}

/// A box (UE, the platform actor's space): its centre, its axes' rows, its half extents, the reach of its corners.
#[derive(Debug)]
struct BoxShape {
    c: [f64; 3],
    ax: [f64; 9],
    h: [f64; 3],
    reach: f64,
}

impl BoxShape {
    /// The distance squared from a point (UE) to the box.
    fn dist2(&self, x: f64, y: f64, z: f64) -> f64 {
        let (dx, dy, dz) = (x - self.c[0], y - self.c[1], z - self.c[2]);
        let mut s = 0.0;
        for i in 0..3 {
            let p = (dx * self.ax[3 * i] + dy * self.ax[3 * i + 1] + dz * self.ax[3 * i + 2]).abs() - self.h[i];
            if p > 0.0 {
                s += p * p;
            }
        }
        s
    }

    /// A fighter's root capsule (upright on the play plane, UE X 0) against the box.
    fn overlaps(&self, y: f64, z: f64, half: f64, r: f64) -> bool {
        let seg = (half - r).max(0.0);
        let (ey, ez) = (y - self.c[1], z - self.c[2]);
        if (self.c[0] * self.c[0] + ey * ey + ez * ez).sqrt() > self.reach + seg + r {
            return false;
        }
        // the distance along the segment is convex: a ternary search
        let (mut lo, mut hi) = (z - seg, z + seg);
        for _ in 0..24 {
            if hi - lo <= 0.01 {
                break;
            }
            let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if self.dist2(0.0, y, m1) <= self.dist2(0.0, y, m2) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        self.dist2(0.0, y, (lo + hi) / 2.0) <= r * r
    }
}

// Unverified: the overlap is the fighter's root capsule against the box (the boxes' native collision profile is
// unsaved; BaseFighter's other components are not tested); begun overlaps of one tick are taken in box order; the
// time is the presentation's tick count (a rollback does not redo a wiggle).
#[derive(Debug)]
struct Plank {
    d: Wiggle,
    skel: Driven,
    /// Its idle sequence; None: the reference pose.
    idle: Option<usize>,
    modify: ModifyIndex,
    bones: Vec<f32>,
    boxes: Vec<BoxShape>,
    /// WiggleInfo, in the map's order.
    records: Vec<WiggleRecord>,
    /// Per fighter, the boxes it overlapped (bits) last tick.
    prev: Vec<u32>,
}

impl Plank {
    fn make(p: PlatformData, host: &mut SceneryHost) -> Result<Plank> {
        let what = format!("scenery treefortActors: {}", p.actor);
        let d = p.wiggle.ok_or_else(|| bad(&what, "without its wiggle"))?;
        let skel = Driven::make(host, &p.skeletal)?;
        let idle = match host.skeletal(&p.skeletal).and_then(|a| a.clip.clone()) {
            Some(c) => Some(clip_of(host, &c, &what)?),
            None => None,
        };
        let boxes = d
            .boxes
            .iter()
            .map(|b| {
                let ax = mvs_vfx::math::rotation_rows(b.rot[0] * DEG, b.rot[1] * DEG, b.rot[2] * DEG);
                let h = [0, 1, 2].map(|i| d.extent * b.scale[i].abs());
                BoxShape { c: b.loc, ax, h, reach: (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt() }
            })
            .collect::<Vec<_>>();
        if boxes.len() > 32 {
            return Err(bad(&what, "more than 32 wiggle boxes"));
        }
        let modify = modify_index(&skel.names(host), &d.modify, &what)?;
        Ok(Plank {
            bones: vec![0.0; d.modify.slots * 3],
            d,
            skel,
            idle,
            modify,
            boxes,
            records: Vec::new(),
            prev: Vec::new(),
        })
    }

    fn tick(&mut self, w: &World) {
        let tick = w.flow.tick;
        let d = &self.d;
        // Tick: the records made before this tick
        let bones = &mut self.bones;
        self.records.retain(|r| {
            let age = f64::from(tick - r.tick) * STEP;
            let o = r.n * 3 + 2;
            if age > d.life || age < 0.0 {
                if o < bones.len() {
                    bones[o] = 0.0;
                }
                return false;
            }
            if o < bones.len() {
                bones[o] = (rich_curve(&d.curve, age) * r.sign * d.scale) as f32;
            }
            true
        });
        // the boxes' begun overlaps: SetWiggle
        let n = w.fighters.len();
        if self.prev.len() < n {
            self.prev.resize(n, 0);
        }
        for (i, f) in w.fighters.ids().enumerate() {
            let y = -w.fighter(f).body.x;
            let z = actor_y(w, f);
            let (half, r) = (capsule_half(w, f), cap_radius(w, f));
            let mut now = 0u32;
            for (k, b) in self.boxes.iter().enumerate() {
                if b.overlaps(f64::from(y), f64::from(z), f64::from(half), f64::from(r)) {
                    now |= 1 << k;
                }
            }
            let begun = now & !self.prev[i];
            self.prev[i] = now;
            for k in 0..self.boxes.len() {
                if begun & (1 << k) == 0 {
                    continue;
                }
                let n = d.boxes[k].n;
                if self.records.iter().any(|r| r.n == n) {
                    continue;
                }
                self.records.push(WiggleRecord {
                    n,
                    sign: if z >= mvs_sim::math::narrow(d.z) { -1.0 } else { 1.0 },
                    tick,
                });
            }
        }
    }

    /// The AnimBP's pose: its idle sequence (or the reference pose), then the ModifyBone nodes.
    fn pose(&mut self, host: &mut SceneryHost, dt: f64) {
        match self.idle {
            Some(c) => {
                let (len, rate) = {
                    let a = &host.state.clips.clips[c].anim;
                    (a.length, a.rate_scale)
                };
                let time = self.skel.advance(host, dt, rate);
                self.skel.sample(host, c, time % len);
            }
            None => self.skel.pose.copy(&self.skel.rest),
        }
        modify_bones(&mut self.skel.pose, &self.modify, &self.bones);
        self.skel.write(host);
    }

    fn begin(&mut self) {
        self.records.clear();
        self.prev.clear();
        self.bones.fill(0.0);
    }
}

// ---- the small platforms

// Unverified: the fighters' blocking hit on the Box is taken as a landing on the platform (the tick his base becomes
// it): the Box blocks the capsule only while he does not ignore it (not rising,
// his feet above its top), which leaves the landing sweep; a hitstun bounce off it (BFBounce) is not taken.
// Unverified: PlayAnimation's single-node instance taken to start at time 0 and play rate 1, its first advance the
// tick of the hit (as the trees' montages); the Delay counted from that tick on (the platform's latent entry and tick
// not read).
#[derive(Debug)]
struct Small {
    actor: String,
    skeletal: String,
    /// The component's own animation (clip, looping, rate).
    own: (Option<Arc<str>>, bool, f64),
    hit: Arc<str>,
    idle: Arc<str>,
    /// Its place in StageData.platforms (None before the first World), what a fighter's base names.
    index: Option<Option<usize>>,
    /// Per fighter, standing on it last tick.
    on: Vec<bool>,
    /// The Delay(0.8) left, < 0 none pending.
    wait: f64,
}

impl Small {
    fn make(p: PlatformData, host: &mut SceneryHost) -> Result<Small> {
        let what = format!("scenery treefortActors: {}", p.actor);
        let (Some(hit), Some(idle)) = (p.hit, p.idle) else {
            return Err(bad(&what, "without its hit and idle animations"));
        };
        clip_of(host, &hit, &what)?;
        clip_of(host, &idle, &what)?;
        let a =
            host.skeletal(&p.skeletal).ok_or_else(|| bad(&what, format!("{} is not a skeletal mesh", p.skeletal)))?;
        Ok(Small {
            actor: p.actor,
            skeletal: p.skeletal,
            own: (a.clip.clone(), a.looping, a.rate),
            hit: Arc::from(hit),
            idle: Arc::from(idle),
            index: None,
            on: Vec::new(),
            wait: -1.0,
        })
    }

    /// PlayAnimation(anim, looping) on its single-node animation.
    fn play(&self, host: &mut SceneryHost, clip: Arc<str>, looping: bool) {
        if let Some(a) = host.skeletal(&self.skeletal) {
            a.clip = Some(clip);
            a.looping = looping;
            a.time = 0.0;
            a.rate = 1.0;
        }
    }

    fn tick(&mut self, host: &mut SceneryHost, w: &World, dt: f64) {
        // (a miss is looked up again each tick, while its index is -1)
        if !matches!(self.index, Some(Some(_))) {
            self.index =
                Some(w.data.stage.platforms.as_deref().and_then(|ps| ps.iter().position(|p| p.name == self.actor)));
        }
        let index = self.index.flatten();
        let mut hit = false;
        let n = w.fighters.len();
        if let Some(index) = index {
            for (i, f) in w.fighters.ids().enumerate() {
                let on = match w.fighter(f).platforms.base {
                    Some(PlatformRef::Stage(s)) => {
                        w.stage.stage_platforms.get(s as usize).is_some_and(|p| p.stage_index == index as i32)
                    }
                    _ => false,
                };
                if i >= self.on.len() {
                    self.on.push(false);
                }
                if on && !self.on[i] {
                    hit = true;
                }
                self.on[i] = on;
            }
        }
        self.on.resize(n, false);
        if hit {
            // TreeFortPlatformHit @186 .. @356
            self.play(host, self.hit.clone(), false);
            if self.wait < 0.0 {
                self.wait = SMALL_DELAY;
            }
        }
        // the Delay's resume (@15), counted from the tick of the hit (the fighters' move comes before the latent pass)
        if self.wait >= 0.0 {
            self.wait -= dt;
            if self.wait <= 0.0 {
                self.wait = -1.0;
                self.play(host, self.idle.clone(), true);
            }
        }
    }

    fn begin(&mut self, host: &mut SceneryHost) {
        if let Some(a) = host.skeletal(&self.skeletal) {
            a.clip = self.own.0.clone();
            a.looping = self.own.1;
            a.rate = self.own.2;
        }
        self.on.clear();
        self.wait = -1.0;
    }
}

/// The `treefortActors` block.
#[derive(Debug)]
pub struct TreefortActors {
    trees: Vec<Tree>,
    planks: Vec<Plank>,
    smalls: Vec<Small>,
}

impl TreefortActors {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<TreefortActors> {
        let what = format!("scenery: {key}");
        let block: ActorsBlock = serde_json::from_value(data.clone()).map_err(|e| bad(&what, e.to_string()))?;
        let mut trees = Vec::new();
        for d in block.pillars {
            trees.push(Tree::make(d, host)?);
        }
        let (mut planks, mut smalls) = (Vec::new(), Vec::new());
        for p in block.platforms {
            match p.kind.as_str() {
                "upper" => planks.push(Plank::make(p, host)?),
                "small" => smalls.push(Small::make(p, host)?),
                k => return Err(bad(&what, format!("{}: the platform kind {k}", p.actor))),
            }
        }
        Ok(TreefortActors { trees, planks, smalls })
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        for t in &mut self.trees {
            t.begin(host);
        }
        for pl in &mut self.planks {
            pl.begin();
        }
        for sm in &mut self.smalls {
            sm.begin(host);
        }
    }

    /// One world tick: the trees, planks and small platforms from the World (none outside a match of the stage),
    /// then the AnimBPs' poses (the drives, in the skeletal meshes' tick after the blocks').
    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        if let Some(w) = host.world() {
            for t in &mut self.trees {
                t.tick(host, w);
            }
            for pl in &mut self.planks {
                pl.tick(w);
            }
            for sm in &mut self.smalls {
                sm.tick(host, w, dt);
            }
        }
        for t in &mut self.trees {
            t.pose(host, dt);
        }
        for pl in &mut self.planks {
            pl.pose(host, dt);
        }
    }

    pub fn frame(&mut self, _host: &mut SceneryHost, _alpha: f64) {}

    pub fn end(&mut self, _host: &mut SceneryHost) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Tree Fort entry's `treefortActors` reads as the block's data: its trees' damage stages, montages and
    /// ModifyBone nodes, the upper platforms' wiggles and the small platforms' animations.
    #[test]
    fn treefort_actors_data_reads() {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: nothing to check", dir.display());
            return;
        };
        let Ok(pack) = store.kind("scenery") else { return };
        let mut seen = 0;
        for path in pack.paths().filter(|p| p.to_ascii_lowercase().contains("treehouse")) {
            let bytes = pack.bytes(path).unwrap();
            let bin = mvs_material::jsonbin::JsonBin::split(b"MVSC", &bytes, path).unwrap();
            let h: super::super::super::SceneryHeader = bin.parse(path).unwrap();
            let Some(data) = h.scenery.rest.get("treefortActors") else { continue };
            let b: ActorsBlock = serde_json::from_value(data.clone()).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(!b.pillars.is_empty(), "{path}");
            for p in &b.pillars {
                assert_eq!(p.damage.len(), 3, "{path}: {}", p.actor);
                assert!(p.hit.notifies.iter().chain(&p.death.notifies).all(|n| n.socket.as_deref() == Some("Root")));
            }
            for p in &b.platforms {
                match p.kind.as_str() {
                    "upper" => assert!(p.wiggle.as_ref().is_some_and(|w| w.boxes.len() == 12), "{path}: {}", p.actor),
                    "small" => assert!(p.hit.is_some() && p.idle.is_some(), "{path}: {}", p.actor),
                    k => panic!("{path}: {k}"),
                }
            }
            seen += 1;
        }
        eprintln!("{seen} Tree Fort entries with treefortActors");
    }

    /// The block over the stage-treehouse match: the right tree's damage meshes shown by stage, the planks' wiggle
    /// records and the small platforms' hit animation from the fighters; then (no match kills a tree) a tree set
    /// dying by hand takes the death material and hides its parts, a rollback puts both back, and a dead one draws
    /// nothing.
    #[test]
    fn treefort_actors_follow_the_world() {
        use crate::stage::stage_key;
        use crate::stagedraw::StageDraw;

        let Some(mut h) = mvs_sim_trace::harness::at("stage-treehouse", 0) else { return };
        let store = mvs_store::Store::open(mvs_store::dev_store_dir()).unwrap();
        let entry = StageDraw::load(&store, &stage_key(&h.w.data.stage_key)).unwrap().scenery.entry;
        let bytes = store.kind("scenery").unwrap().bytes(&entry).unwrap();
        let bin = mvs_material::jsonbin::JsonBin::split(b"MVSC", &bytes, &entry).unwrap();
        let s = bin.parse::<super::super::super::SceneryHeader>(&entry).unwrap().scenery;
        let meshes =
            s.skeletal.iter().map(|k| Arc::new(mvs_view::mesh::MeshInfo::load(&store, &k.mesh).unwrap())).collect();
        let mut run =
            super::super::super::SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest)
                .unwrap();
        let b =
            run.blocks.iter().position(|b| matches!(b, super::super::super::SceneryBlock::TreefortActors(_))).unwrap();
        let mut sound = super::super::super::NullSound;
        let step =
            |run: &mut super::super::super::SceneryRun, w: &World, sound: &mut super::super::super::NullSound| {
                let mut host = SceneryHost { state: &mut run.state, vfx: None, sound, world: Some(w) };
                run.blocks[b].tick(&mut host, STEP);
                run.state.tick_skeletal(STEP);
            };
        {
            let mut host = SceneryHost { state: &mut run.state, vfx: None, sound: &mut sound, world: None };
            run.blocks[b].begin(&mut host);
        }
        let (mut wiggled, mut stage2) = (false, false);
        for _ in 0..2440 {
            h.step_recorded();
            step(&mut run, &h.w, &mut sound);
            let super::super::super::SceneryBlock::TreefortActors(t) = &run.blocks[b] else { unreachable!() };
            wiggled |= t.planks.iter().any(|p| !p.records.is_empty());
            let tree = &t.trees[1];
            if tree.stage == 2 {
                let Some(id) = tree.sets[2] else { unreachable!() };
                assert!(run.state.own[id.0 as usize].shown, "the stage 2 mesh shown");
                assert!(
                    !run.state.skeletal_shown[run.state.skeletal.iter().position(|x| x.name == tree.d.mesh).unwrap()]
                );
                stage2 = true;
            }
        }
        assert!(wiggled, "a fighter's capsule began an overlap with a plank box");

        // (no fighter of the match lands on a small platform: one is put on the first by hand) TreeFortPlatformHit,
        // then the idle animation 0.8 s on
        let (small, hit, idle) = {
            let super::super::super::SceneryBlock::TreefortActors(t) = &run.blocks[b] else { unreachable!() };
            (t.smalls[0].skeletal.clone(), t.smalls[0].hit.clone(), t.smalls[0].idle.clone())
        };
        let j = h.w.stage.stage_platforms.iter().position(|p| p.stage_index == 1).unwrap();
        let f = h.w.fighters.ids().next().unwrap();
        h.w.fighter_mut(f).platforms.base = Some(PlatformRef::Stage(j as u32));
        let ks = run.state.skeletal.iter().position(|x| x.name == small).unwrap();
        step(&mut run, &h.w, &mut sound);
        assert_eq!(run.state.anims[ks].clip.as_deref(), Some(&*hit));
        assert!(!run.state.anims[ks].looping);
        for _ in 0..48 {
            step(&mut run, &h.w, &mut sound);
        }
        assert_eq!(run.state.anims[ks].clip.as_deref(), Some(&*idle), "the idle animation after the Delay");
        assert!(run.state.anims[ks].looping);
        assert!(stage2, "the right tree reached damage stage 2");

        // DeathVisuals: the tree set dying now
        let set_pillar = |w: &mut World, actor: &str, f: &dyn Fn(&mut mvs_sim::world::stage_actors::StageActor)| {
            let placed = w.data.stage.placed.clone().unwrap();
            for a in &mut w.stage_run.as_mut().unwrap().actors {
                if placed[a.index as usize].name == actor {
                    f(a);
                }
            }
        };
        let (actor, mesh, parts) = {
            let super::super::super::SceneryBlock::TreefortActors(t) = &run.blocks[b] else { unreachable!() };
            let tree = &t.trees[0];
            (tree.d.actor.clone(), tree.d.mesh.clone(), tree.parts.iter().map(|p| p.name.clone()).collect::<Vec<_>>())
        };
        let tick = h.w.flow.tick;
        // (the match's fighters may have destroyed this tree by now: it is put back alive first)
        set_pillar(&mut h.w, &actor, &|a| {
            a.dead = false;
            if let StageActorOwn::Pillar(p) = &mut a.own {
                p.dying_at = tick;
            }
        });
        step(&mut run, &h.w, &mut sound);
        let k = run.state.skeletal.iter().position(|x| x.name == mesh).unwrap();
        let slots = format!("{:?}", run.state.skel_slots[k]);
        assert!(slots.contains("swap: Some(") && slots.contains("Tint"), "the death material on the tree: {slots}");
        for p in &parts {
            let j = run.state.skeletal.iter().position(|x| x.name == *p).unwrap();
            assert!(!run.state.skeletal_shown[j], "{p} hidden while dying");
        }
        {
            let super::super::super::SceneryBlock::TreefortActors(t) = &run.blocks[b] else { unreachable!() };
            assert!(t.trees[0].death_pos > 0.0);
        }
        // a rollback to alive: its own materials again
        set_pillar(&mut h.w, &actor, &|a| {
            if let StageActorOwn::Pillar(p) = &mut a.own {
                p.dying_at = -1;
            }
        });
        step(&mut run, &h.w, &mut sound);
        assert!(!format!("{:?}", run.state.skel_slots[k]).contains("swap: Some("), "the tree's own materials back");
        // destroyed: nothing drawn
        set_pillar(&mut h.w, &actor, &|a| a.dead = true);
        step(&mut run, &h.w, &mut sound);
        let super::super::super::SceneryBlock::TreefortActors(t) = &run.blocks[b] else { unreachable!() };
        assert_eq!(t.trees[0].stage, -1);
        assert!(!run.state.skeletal_shown[k]);
    }
}
