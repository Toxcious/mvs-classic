//! Scooby's Haunted Mansion (Map_ScoobyDoo, Map_ScoobyDoo_NoRoof): the blocks its scenery entry has of its own
//! Blueprint actors and level Blueprints: `scooby` (the elevators, the switches, the walls, the webs) and
//! `scoobyMotion` (the lightning, the ghost walks, the statues' LookAt, the elevators' dust).
//!
//! The switches, trapdoors, walls and chains are drawn from their sim state (the ScoobyElevatorSwitch_C,
//! ScoobyElevator_C and ScoobyWall_C of the World's stage actors, read each tick through `SceneryHost::world`); the
//! graphs are ScoobyElevatorSwitch, ScoobyElevator, ScoobyWall (offsets: each class's ubergraph's).
//!
//! - Every animation is SkeletalMeshComponent.PlayAnimation(asset, false) on a component with no AnimClass: the single
//!   node instance: a montage by montage rules (its one slot track), a sequence (the switch's end poses) from 0, held
//!   at its end.
//! - A montage's position: the ticks since the one it was played in plus that tick's own, x 1/60 x RateScale
//!   (Unverified: the "+1", ledger "Scooby actor plays: position"). A play
//!   replaces the component's last (the switch's end pose at doneAt cuts its Release / Retract montage, which is
//!   longer than the 1.65 s move).
//! - The montages' AnimNotify_PlayVFX states (the switches' PS_MansionGearSparks, the trapdoors' two
//!   PS_MansionGearSparks_Big): a non-fighter owner's SpawnVFX path (@2233 -> @3113: SpawnEmitterAttached(ParticleTeam0,
//!   the mesh, SocketName Root, Location, Rotation, VFX Scale 1, KeepRelativeOffset)), deactivated at the state's end
//!   (KillOnNotifyEnd, @523..1409).
//! - The switch (ScoobyElevatorSwitch_C): the Release (Up false) / Retract montage at moveAt (MoveScoobyElevatorMulticast
//!   @788), the Down / Up end pose at doneAt (the move's link @48); hidden when dead (BeginPlay's K2_DestroyActor with
//!   AllowHazards off, @501). The trap chains' montages (PlayTrapChainAnims @2413 / @2521) and the wall chain's
//!   (PlayWallChainVisuals @2231) at moveAt; the wall chain's WallChainParticle0 at moveAt (@2151) and WallChainParticle1
//!   at chain1At (the Delay(0.35)'s link @327), SpawnEmitterAtLocation at their transforms.
//! - The trapdoor (ScoobyElevator_C.TrapDoorSkelMesh): its MoveDownAnim / MoveUpAnim at the elevator's moveAt (the
//!   switch's @979); moved with the root: the holder carries the BoxCollision solid's offset, blended between the last
//!   two ticks as the pass-through platforms are (platform_blend.rs). The gear slot's dynamic instance of its
//!   GearGlowSourceMaterial takes `emissive` as Emissive (1 while moving: @1011 / the link's @130).
//! - The wall (ScoobyWall_C): ScoobyWallImpactVFX and ScoobyWallImpactAnim at hitAt (PlayImpactVisualsMulticast @10:
//!   @56, @117), the gears' Release (Up false) / Retract at gearAt (PlayScoobyWallGearAnim @571 / @657),
//!   ScoobyWallDestroyVFX at brokenAt (@334), the wall and its gears hidden from then (K2_DestroyActor).
//! - A SpawnEmitterAtLocation is spawned when its tick changes to one less than 6 ticks back (a tick seen late, a
//!   rollback that played it again), first ticked the next frame (Unverified: ledger "Batcave / Scooby drawing:
//!   rollback re-fire"); a notify state's start put back (a rollback) fires
//!   none of the positions already passed.
//!
//! The webs (BP_ScoobyWeb_C): each the construction script's dynamic instance of SCOOB_Webs_A_01_M with its
//! Emissive_Mult and WPO (`web` on the instance).
//!
//! What moves on its own is `scoobyMotion`. Each wait is a world Delay counted down by the tick's time and done at or
//! below 0 (the leftover latent pass: what it spawns at a place first ticks the frame after, `spawn_level`'s `next`).
//! The random waits are the presentation's (`SceneryHost::rand`), as the build's RandomFloatInRange is no part of a
//! rule.
//! - BP_Mansion_LightningStrike_C: BeginPlay @327 -> @225 RandomFloatInRange(30, 40) s, Delay @263 -> @322 -> @15
//!   PF_Post_Global_Event(amb_map_scoobydoo_emit_lightning), Delay(SFX Delay 0.2) @70 -> @129
//!   SpawnEmitterAttached(PS_MansionLightningStrike) on the actor's component at (0, 0, 0), rot 0, scale 1, auto
//!   destroy (the component's place: a static actor), then @225 again.
//! - Map_ScoobyDoo_C / Map_ScoobyDoo_NoRoof_C: HandleWalkingGhost @409 -> @127 RandomFloatInRange(30, 60) s, Delay
//!   @165 -> @224 PS_ScoobyMansion_GhostWalk at (-2985, 2380, 990) yaw -90, RandomFloatInRange(30, 60) s, Delay @335 ->
//!   @54 PS_ScoobyMansion_GhostWalk_L at (-2985, -2380, 990) yaw 90, @127 again (SpawnEmitterAtLocation, scale 1, auto
//!   destroy). Nothing posts sfx_map_scoobydoo_ghost_appear_l / _r: no sound.
//! - ScoobyMansion_Statue_AnimBP_C / ScoobyMansion_StatueB_AnimBP_C: BeginPlay -> Delay(0.5) (A @677 -> @619, B @73 ->
//!   @15) -> GetMyPlayerData: the fighter found (A: the one whose MatchPlayerData is the local player's; B: the local
//!   player's teammate's (GetTeamMateByAccountID @382), else the first of GetAllPlayers that is not the local player
//!   (@1007..1641; alone, the loop ends on the last index: the local player's own); then the
//!   GetAllActorsOfClass(BaseFighter_C) loop @128..928 keeps the actor whose MatchPlayerData is that one, else the
//!   last actor it tried), or Delay(0.5) again while none is valid; BlueprintUpdateAnimation then writes the fighter's
//!   K2_GetActorLocation into LookAtPoint, which the property access copies into the LookAt node's LookAtLocation
//!   (LookAtPoint starts at the world origin).
//!   The graph: Root <- ComponentToLocal <- LookAt(statueHead) on an unlinked pose (the reference pose), alpha 1, no
//!   clamp, no interpolation (checked at make). FAnimNode_LookAt / AnimationCore::SolveAim are native: LookAtLocation
//!   with no target bone is a world location, taken into component space; the bone's look-at axis (0, -1, 0) and
//!   look-up axis (0, 0, 1), both in the bone's space, are turned by its component rotation; with bUseLookUpAxis both
//!   the axis and the direction to the target are projected on the plane normal to the look-up axis and normalised,
//!   and FindBetweenNormals of the two is put before the bone's component rotation (a turn about the look-up axis
//!   only).
//! - ScoobyElevator_C's PS_Mansion_Dustfall (ScoobyElevator @42..92: at DefaultSceneRoot's location, rot 0, scale 1,
//!   auto destroy): from the sim's dustAt / dustX / dustY, once per new dustAt.
// Unverified: GetTeamMateByAccountID (native) is taken as the first other player of GetAllPlayers on the local
// player's team; GetAllActorsOfClass's order as the fighters'.
// Unverified: FAnimNode_LookAt / AnimationCore::SolveAim: 4.26 source from memory, not read in the exe.
// Unverified: the pose after a montage's end taken as its last frame held (the single node's montage end is not read in
// the exe); its blend in / out not drawn (the single node's source pose is the same asset's: 4.26 source memory, not
// read).
// Unverified: a notify state still open when its montage ends or is replaced ends then.
// Unverified: the lightning component drawn active from BeginPlay by the scenery's emitters (the auto activation at
// BeginPlay of a component whose tick starts off, 4.26 from memory, not read in the exe); the global game object's
// place and orientation (those of a pooled component at the origin with no rotation, which is what
// `SceneryHost::sound` posts on).
// Deviation: a Lab reset or fighter change replaces the pawns (Spawn All Players), which leaves the build's Fighter
// invalid and the statue still (the search runs from BlueprintBeginPlay alone); here it follows the new pawn at that
// index.
// Deviation: the particle systems that follow a bone (the notifies') take the bone as the tick before the block's
// own left it: the skeletal meshes' animation runs after the blocks' ticks here, and the follow is evaluated in
// the particle player's tick after it.

use std::sync::Arc;

use mvs_sim::World;
use mvs_sim::fighter::body::actor_y;
use mvs_sim::stages::scoobys_haunted_mansion::{ScoobyElevator, ScoobySwitch, ScoobyWall};
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_vfx::math::{Rows, compose_rows, rotation_rows};
use mvs_vfx::{Attachment, SystemId};
use mvs_view::M4;
use mvs_view::math::{self, Mat4};
use serde::Deserialize;
use serde_json::Value;

use super::host::{InstanceId, SceneryHost, SkinRef, stage_actor};
use super::platform_blend::PlatformBlend;
use super::sky_arena::attachment_of;
use crate::Result;
use crate::stage::{bad, transform};

const STEP: f64 = 1.0 / 60.0;
const DEG: f64 = std::f64::consts::PI / 180.0;
const FX_LATE: i32 = 6;

fn rows_of(r: &[f64; 3]) -> Rows {
    rotation_rows(r[0] * DEG, r[1] * DEG, r[2] * DEG)
}

fn rows_flat(a: &[[f64; 3]; 3]) -> Rows {
    [a[0][0], a[0][1], a[0][2], a[1][0], a[1][1], a[1][2], a[2][0], a[2][1], a[2][2]]
}

fn rows_nested(r: &Rows) -> [[f64; 3]; 3] {
    [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]]
}

// ---- the data (`scoobyMotion`)

#[derive(Debug, Deserialize)]
struct SegmentData {
    anim: String,
    pos: f64,
    start: f64,
    end: f64,
    rate: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct Notify {
    at: f64,
    end: Option<f64>,
    system: String,
    loc: [f64; 3],
    rot: [f64; 3],
    kill: bool,
}

#[derive(Debug, Deserialize)]
struct PlayData {
    length: f64,
    rate: f64,
    segments: Vec<SegmentData>,
    notifies: Vec<Notify>,
}

#[derive(Debug, Deserialize)]
struct EmitterData {
    system: String,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct GearData {
    slot: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElevatorData {
    actor: String,
    skeletal: String,
    gear: GearData,
    solid: String,
    move_down: PlayData,
    move_up: PlayData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitchData {
    actor: String,
    skeletal: String,
    release: PlayData,
    retract: PlayData,
    down_pose: PlayData,
    up_pose: PlayData,
    trap_chains: Vec<String>,
    trap_release: PlayData,
    trap_retract: PlayData,
    wall_chain: String,
    wall_release: PlayData,
    wall_retract: PlayData,
    particle0: EmitterData,
    particle1: EmitterData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WallData {
    actor: String,
    wall: String,
    gears: Vec<String>,
    impact: EmitterData,
    destroy: EmitterData,
    impact_anim: PlayData,
    gear_release: PlayData,
    gear_retract: PlayData,
}

#[derive(Debug, Deserialize)]
struct ScoobyBlock {
    webs: usize,
    #[serde(default)]
    elevators: Vec<ElevatorData>,
    #[serde(default)]
    switches: Vec<SwitchData>,
    #[serde(default)]
    walls: Vec<WallData>,
}

// ---- one component's animation as the single node plays it

/// A segment of a play's slot track: its animation (by build object path) and the montage position it starts at.
#[derive(Debug)]
struct Seg {
    anim: Arc<str>,
    pos: f64,
    start: f64,
    end: f64,
    rate: f64,
}

/// A play (a montage or a sequence): its length and RateScale, its track's segments and its notify states.
#[derive(Debug)]
struct Play {
    length: f64,
    rate: f64,
    segments: Vec<Seg>,
    notifies: Vec<Notify>,
}

fn play_of(d: PlayData) -> Play {
    Play {
        length: d.length,
        rate: d.rate,
        segments: d
            .segments
            .into_iter()
            .map(|s| Seg { anim: Arc::from(s.anim.as_str()), pos: s.pos, start: s.start, end: s.end, rate: s.rate })
            .collect(),
        notifies: d.notifies,
    }
}

/// One component's animation: the asset and the tick it was played in, its notify states' last tested position and
/// their systems (by notify).
#[derive(Debug)]
struct Comp {
    name: String,
    /// Its plays have notifies (a socket Root to attach them to).
    notifies: bool,
    play: Option<usize>,
    start: i32,
    pos: f64,
    seen_start: i32,
    seen: f64,
    fx: Vec<Option<SystemId>>,
}

/// A notify's system deactivated and still alive: it keeps following its bone until its particles are gone.
#[derive(Debug)]
struct Dying {
    id: SystemId,
    comp: usize,
    play: usize,
    notify: usize,
}

/// A SpawnEmitterAtLocation keyed by the tick it was made in.
#[derive(Debug)]
struct Shot {
    seen: i32,
    fx: Option<SystemId>,
}

impl Shot {
    fn new() -> Shot {
        Shot { seen: -1, fx: None }
    }

    fn tick(&mut self, host: &mut SceneryHost, at: i32, tick: i32, system: &str, place: &Attachment) {
        if at == self.seen {
            return;
        }
        self.seen = at;
        if at >= 0 && tick - at < FX_LATE {
            self.fx = host.vfx().and_then(|v| v.spawn_level(system, place, true, &[], 0, false));
        }
    }

    fn reset(&mut self, host: &mut SceneryHost) {
        if let (Some(id), Some(v)) = (self.fx.take(), host.vfx()) {
            v.kill(id);
        }
        self.seen = -1;
    }
}

/// A notify's system attached at the socket Root with its Location / Rotation (KeepRelativeOffset): the Root's
/// world transform `root` (UE) with the notify's offset in its frame.
fn notify_follow(root: &Attachment, nt: &Notify) -> Attachment {
    let rel = rows_of(&nt.rot);
    let s = root.scale.expect("a bone's attachment has its scale");
    let r = rows_flat(&root.rot);
    let l = nt.loc;
    let loc =
        std::array::from_fn(|i| root.loc[i] + s[0] * l[0] * r[i] + s[1] * l[1] * r[3 + i] + s[2] * l[2] * r[6 + i]);
    Attachment { loc, rot: rows_nested(&compose_rows(&rel, &r)), scale: Some(s) }
}

fn place_of(e: &EmitterData) -> Attachment {
    Attachment { loc: e.loc, rot: rows_nested(&rows_of(&e.rot)), scale: Some(e.scale) }
}

/// The components' plays, their state and their notify systems.
#[derive(Debug, Default)]
struct Core {
    plays: Vec<Play>,
    comps: Vec<Comp>,
    dying: Vec<Dying>,
}

impl Core {
    fn add_play(&mut self, d: PlayData) -> usize {
        self.plays.push(play_of(d));
        self.plays.len() - 1
    }

    /// A component of the entry that plays (`notifies`: it has a Root to attach a notify's system to).
    fn add_comp(&mut self, host: &mut SceneryHost, name: &str, notifies: bool, plays: &[usize]) -> Result<usize> {
        let what = format!("scenery: scooby {name}");
        if !notifies && plays.iter().any(|&p| !self.plays[p].notifies.is_empty()) {
            return Err(bad(&what, "notifies"));
        }
        let Some(anim) = host.skeletal(name) else {
            return Err(bad(&what, "not a skeletal mesh of the file"));
        };
        if anim.clip.is_some() {
            return Err(bad(&what, "already plays"));
        }
        if notifies && host.bone_world(name, "Root").is_none() {
            return Err(bad(&what, "has no bone Root"));
        }
        self.comps.push(Comp {
            name: name.to_owned(),
            notifies,
            play: None,
            start: -1,
            pos: -1.0,
            seen_start: -1,
            seen: 0.0,
            fx: Vec::new(),
        });
        Ok(self.comps.len() - 1)
    }

    /// The notify's attachment now: the component's Root as it is posed with the notify's offset.
    fn follow(&self, host: &SceneryHost, comp: usize, play: usize, notify: usize) -> Attachment {
        let name = &self.comps[comp].name;
        let m = host.bone_world(name, "Root").unwrap_or_else(|| panic!("scenery: scooby {name}: no bone Root"));
        notify_follow(&attachment_of(&m), &self.plays[play].notifies[notify])
    }

    fn deactivate(&mut self, host: &mut SceneryHost, comp: usize, play: usize, notify: usize, id: SystemId) {
        if let Some(v) = host.vfx() {
            v.deactivate(id);
            self.dying.push(Dying { id, comp, play, notify });
        }
    }

    /// The states of the component's play ended (a new play, a reset).
    fn end_fx(&mut self, host: &mut SceneryHost, ci: usize) {
        let play = self.comps[ci].play;
        for k in 0..self.comps[ci].fx.len() {
            if let Some(id) = self.comps[ci].fx[k].take() {
                // (the play that spawned it: the one the component had)
                self.deactivate(host, ci, play.unwrap_or(0), k, id);
            }
        }
    }

    /// The component's play this tick (None: none yet), and its notify states.
    fn play_tick(&mut self, host: &mut SceneryHost, ci: usize, tick: i32, play: Option<usize>, start: i32) {
        let rate = play.map(|p| self.plays[p].rate);
        {
            let c = &mut self.comps[ci];
            c.start = if play.is_some() { start } else { -1 };
            c.pos = rate.map_or(-1.0, |r| f64::from(tick - start + 1) * STEP * r);
        }
        if self.comps[ci].start != self.comps[ci].seen_start || play != self.comps[ci].play {
            // a new play ends the last one's states; one played again (a rollback) or seen late fires no passed notify
            // Deviation: a different montage at the same start (a rollback) is a new play, where a key on the start
            // alone would follow the live systems with the new play's notify list
            self.end_fx(host, ci);
            let c = &mut self.comps[ci];
            c.seen = if c.start >= c.seen_start && tick - c.start < FX_LATE { f64::NEG_INFINITY } else { c.pos };
            c.seen_start = c.start;
        }
        self.comps[ci].play = play;
        let Some(p) = play else { return };
        let n = self.plays[p].notifies.len();
        if n == 0 {
            return;
        }
        let length = self.plays[p].length;
        let pos = self.comps[ci].pos;
        let done = pos >= length;
        let pos = pos.min(length);
        if self.comps[ci].fx.len() < n {
            self.comps[ci].fx.resize(n, None);
        }
        for k in 0..n {
            let (at, end, kill) = {
                let nt = &self.plays[p].notifies[k];
                (nt.at, nt.end, nt.kill)
            };
            if at > self.comps[ci].seen && at <= pos {
                if let Some(id) = self.comps[ci].fx[k].take() {
                    self.deactivate(host, ci, p, k, id);
                }
                let at = self.follow(host, ci, p, k);
                let system = &self.plays[p].notifies[k].system;
                self.comps[ci].fx[k] = host.vfx().and_then(|v| v.spawn_level(system, &at, false, &[], 0, false));
            }
            if self.comps[ci].fx[k].is_some() && (done || (end.is_some_and(|e| kill && e <= pos))) {
                let id = self.comps[ci].fx[k].take().expect("checked");
                self.deactivate(host, ci, p, k, id);
            }
        }
        self.comps[ci].seen = pos;
    }

    fn reset_comp(&mut self, host: &mut SceneryHost, ci: usize) {
        self.end_fx(host, ci);
        let c = &mut self.comps[ci];
        c.play = None;
        c.start = -1;
        c.seen_start = -1;
        c.pos = -1.0;
        if let Some(a) = host.skeletal(&c.name) {
            a.clip = None;
        }
    }

    /// The notify systems follow their bones: each live one and the deactivated ones until their particles are gone.
    fn follow_all(&mut self, host: &mut SceneryHost) {
        for ci in 0..self.comps.len() {
            let Some(p) = self.comps[ci].play else { continue };
            if !self.comps[ci].notifies {
                continue;
            }
            for k in 0..self.comps[ci].fx.len() {
                let Some(id) = self.comps[ci].fx[k] else { continue };
                let alive = host.vfx().is_some_and(|v| v.alive(id));
                if !alive {
                    self.comps[ci].fx[k] = None;
                    continue;
                }
                let at = self.follow(host, ci, p, k);
                if let Some(v) = host.vfx() {
                    v.attach(id, &at);
                }
            }
        }
        let mut dying = std::mem::take(&mut self.dying);
        dying.retain(|d| {
            if !host.vfx().is_some_and(|v| v.alive(d.id)) {
                return false;
            }
            let at = self.follow(host, d.comp, d.play, d.notify);
            if let Some(v) = host.vfx() {
                v.attach(d.id, &at);
            }
            true
        });
        // (what end_fx pushed meanwhile: none)
        dying.append(&mut self.dying);
        self.dying = dying;
    }

    /// The single node's pose of each component: its play's segment at the play's position (a held end costs
    /// nothing: the pose is sampled again only when the asset or the time changed), the reference pose with no play.
    fn pose_all(&self, host: &mut SceneryHost) {
        for c in &self.comps {
            let Some(a) = host.skeletal(&c.name) else { continue };
            a.looping = false;
            a.rate = 0.0;
            match c.play {
                None => a.clip = None,
                Some(p) => {
                    let p = &self.plays[p];
                    let pos = c.pos.min(p.length);
                    let mut s = &p.segments[0];
                    for x in &p.segments {
                        if pos >= x.pos {
                            s = x;
                        }
                    }
                    a.clip = Some(s.anim.clone());
                    a.time = s.end.min(s.start + (pos - s.pos) * s.rate);
                }
            }
        }
    }
}

// ---- the sim actors' state as one tick left it

#[derive(Debug)]
struct MoveSnap {
    dx: f64,
    dy: f64,
    mx: f64,
    my: f64,
}

#[derive(Debug)]
struct ElevSnap {
    solid: Option<MoveSnap>,
    /// moveAt, moveUp, emissive.
    a: Option<(i32, bool, f64)>,
}

#[derive(Debug)]
struct SwitchSnap {
    shown: bool,
    /// moveAt, moveUp, doneAt, chain1At.
    a: Option<(i32, bool, i32, i32)>,
}

#[derive(Debug)]
struct WallSnap {
    shown: bool,
    /// hitAt, gearAt, gearUp, brokenAt.
    a: Option<(i32, i32, bool, i32)>,
}

#[derive(Debug, Default)]
struct Snap {
    tick: i32,
    elevators: Vec<ElevSnap>,
    switches: Vec<SwitchSnap>,
    walls: Vec<WallSnap>,
}

/// The sim actors' StageData.placed indices and the elevators' solids, matched once per match.
#[derive(Debug)]
struct Idx {
    elevators: Vec<i32>,
    switches: Vec<i32>,
    walls: Vec<i32>,
    solids: Vec<usize>,
}

#[derive(Debug)]
struct Elev {
    actor: String,
    skeletal: String,
    solid: String,
    slot: usize,
    comp: usize,
    down: usize,
    up: usize,
    blend: PlatformBlend,
    dy: f64,
    dz: f64,
    base: M4,
    emissive: f64,
}

#[derive(Debug)]
struct Switch {
    actor: String,
    skeletal: String,
    comp: usize,
    release: usize,
    retract: usize,
    down_pose: usize,
    up_pose: usize,
    trap: Vec<usize>,
    trap_release: usize,
    trap_retract: usize,
    wall: usize,
    wall_release: usize,
    wall_retract: usize,
    p0: Shot,
    p1: Shot,
    system0: String,
    system1: String,
    at0: Attachment,
    at1: Attachment,
    shown: bool,
}

#[derive(Debug)]
struct Wall {
    actor: String,
    wall: String,
    gears: Vec<String>,
    comp: usize,
    gear_comps: Vec<usize>,
    impact_anim: usize,
    gear_release: usize,
    gear_retract: usize,
    impact: Shot,
    destroy: Shot,
    impact_system: String,
    destroy_system: String,
    at_impact: Attachment,
    at_destroy: Attachment,
    shown: bool,
}

/// The `scooby` block.
#[derive(Debug)]
pub struct Scooby {
    core: Core,
    elevators: Vec<Elev>,
    switches: Vec<Switch>,
    walls: Vec<Wall>,
    idx: Option<Idx>,
    /// The last tick's snapshot, kept for its buffers.
    snap: Snap,
}

impl Scooby {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<Scooby> {
        let what = format!("scenery: scooby {key}");
        let d: ScoobyBlock = serde_json::from_value(data.clone()).map_err(|e| bad(&what, e.to_string()))?;
        // the webs: each instance with its pair of scalars (a dynamic instance of one material per pair of values)
        let mut web_key: Option<&str> = None;
        let mut pairs: Vec<(f64, f64)> = Vec::new();
        let mut webs: Vec<(InstanceId, f64, f64)> = Vec::new();
        for (k, inst) in host.instances().iter().enumerate() {
            let Some(w) = inst.fields.get("web") else { continue };
            let num = |n: &str| {
                w.get(n)
                    .and_then(Value::as_f64)
                    .ok_or_else(|| bad(&format!("scenery: scooby web {}", inst.name), format!("no {n}")))
            };
            let (e, p) = (num("Emissive_Mult")?, num("WPO")?);
            let m = match inst.materials.as_slice() {
                [Some(m)] if web_key.is_none_or(|w| w == m) => m,
                _ => return Err(bad(&format!("scenery: scooby web {}", inst.name), "its material")),
            };
            web_key = Some(m);
            if !pairs.contains(&(e, p)) {
                pairs.push((e, p));
            }
            webs.push((InstanceId(u32::try_from(k).expect("an instance index")), e, p));
        }
        if pairs.len() != d.webs {
            return Err(bad(&what, format!("webs {} of {}", pairs.len(), d.webs)));
        }
        for (id, e, p) in webs {
            // (the WPO scalar is written and has no effect: the scenery runs no world position offset)
            host.vfx_material_scalars(id, 0, &[("Emissive_Mult", e as f32), ("WPO", p as f32)]);
        }

        let mut core = Core::default();
        let mut elevators = Vec::new();
        for e in d.elevators {
            let (down, up) = (core.add_play(e.move_down), core.add_play(e.move_up));
            let comp = core.add_comp(host, &e.skeletal, true, &[down, up])?;
            let k = host.skeletal_index(&e.skeletal).expect("added: a skeletal mesh of the file");
            let sk = &host.skeletals()[k];
            let base = transform(sk.loc, sk.rot, sk.scale);
            elevators.push(Elev {
                actor: e.actor,
                skeletal: e.skeletal,
                solid: e.solid,
                slot: e.gear.slot,
                comp,
                down,
                up,
                blend: PlatformBlend::default(),
                dy: 0.0,
                dz: 0.0,
                base,
                emissive: 0.0,
            });
        }
        let mut switches = Vec::new();
        for s in d.switches {
            let release = core.add_play(s.release);
            let retract = core.add_play(s.retract);
            let down_pose = core.add_play(s.down_pose);
            let up_pose = core.add_play(s.up_pose);
            let comp = core.add_comp(host, &s.skeletal, true, &[release, retract, down_pose, up_pose])?;
            let trap_release = core.add_play(s.trap_release);
            let trap_retract = core.add_play(s.trap_retract);
            let mut trap = Vec::new();
            for n in &s.trap_chains {
                trap.push(core.add_comp(host, n, false, &[trap_release, trap_retract])?);
            }
            let wall_release = core.add_play(s.wall_release);
            let wall_retract = core.add_play(s.wall_retract);
            let wall = core.add_comp(host, &s.wall_chain, false, &[wall_release, wall_retract])?;
            switches.push(Switch {
                actor: s.actor,
                skeletal: s.skeletal,
                comp,
                release,
                retract,
                down_pose,
                up_pose,
                trap,
                trap_release,
                trap_retract,
                wall,
                wall_release,
                wall_retract,
                p0: Shot::new(),
                p1: Shot::new(),
                at0: place_of(&s.particle0),
                at1: place_of(&s.particle1),
                system0: s.particle0.system,
                system1: s.particle1.system,
                shown: true,
            });
        }
        let mut walls = Vec::new();
        for w in d.walls {
            let impact_anim = core.add_play(w.impact_anim);
            let comp = core.add_comp(host, &w.wall, false, &[impact_anim])?;
            let gear_release = core.add_play(w.gear_release);
            let gear_retract = core.add_play(w.gear_retract);
            let mut gear_comps = Vec::new();
            for n in &w.gears {
                gear_comps.push(core.add_comp(host, n, false, &[gear_release, gear_retract])?);
            }
            walls.push(Wall {
                actor: w.actor,
                wall: w.wall,
                gears: w.gears,
                comp,
                gear_comps,
                impact_anim,
                gear_release,
                gear_retract,
                impact: Shot::new(),
                destroy: Shot::new(),
                at_impact: place_of(&w.impact),
                at_destroy: place_of(&w.destroy),
                impact_system: w.impact.system,
                destroy_system: w.destroy.system,
                shown: true,
            });
        }
        Ok(Scooby { core, elevators, switches, walls, idx: None, snap: Snap::default() })
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        for ci in 0..self.core.comps.len() {
            self.core.reset_comp(host, ci);
        }
        self.idx = None;
        for e in &mut self.elevators {
            e.blend = PlatformBlend::default();
            e.dy = 0.0;
            e.dz = 0.0;
            host.place_skeletal(&e.skeletal, None);
            e.emissive = 0.0;
            // (BeginPlay's GearMaterial: a dynamic instance on the gear slot)
            host.skeletal_scalars(SkinRef::File(&e.skeletal), e.slot, &[("Emissive", 0.0)]);
        }
        for s in &mut self.switches {
            s.p0.reset(host);
            s.p1.reset(host);
            if !s.shown {
                host.show_skeletal(&s.skeletal, true);
            }
            s.shown = true;
        }
        for x in &mut self.walls {
            x.impact.reset(host);
            x.destroy.reset(host);
            if !x.shown {
                host.show_skeletal(&x.wall, true);
                for g in &x.gears {
                    host.show_skeletal(g, true);
                }
            }
            x.shown = true;
        }
    }

    /// The sim actors' state as the World's latest tick left it (the actors by StageData.placed index, matched once
    /// per match); None with no stage run.
    fn snapshot(&mut self, w: &World, out: &mut Snap) -> bool {
        let Some(run) = w.stage_run.as_ref() else { return false };
        if self.idx.is_none() {
            let placed = w.data.stage.placed.as_deref().unwrap_or(&[]);
            let by =
                |n: &str| placed.iter().position(|p| p.name == n).map_or(-1, |i| i32::try_from(i).expect("an index"));
            let all = w.data.stage.solids.as_deref().unwrap_or(&[]);
            let solids = self
                .elevators
                .iter()
                .map(|e| {
                    all.iter()
                        .position(|s| s.name == e.solid)
                        .unwrap_or_else(|| panic!("scenery: scooby {} is not a solid of the stage data", e.solid))
                })
                .collect();
            self.idx = Some(Idx {
                elevators: self.elevators.iter().map(|e| by(&e.actor)).collect(),
                switches: self.switches.iter().map(|s| by(&s.actor)).collect(),
                walls: self.walls.iter().map(|x| by(&x.actor)).collect(),
                solids,
            });
        }
        let idx = self.idx.as_ref().expect("matched");
        let actor = |i: i32| u32::try_from(i).ok().and_then(|i| stage_actor(w, i));
        out.elevators.clear();
        out.elevators.extend(idx.elevators.iter().zip(&idx.solids).map(|(&i, &si)| ElevSnap {
            solid: run.solids.get(si).map(|s| MoveSnap {
                dx: f64::from(s.m.dx),
                dy: f64::from(s.m.dy),
                mx: f64::from(s.m.mx),
                my: f64::from(s.m.my),
            }),
            a: actor(i).and_then(|a| match &a.own {
                StageActorOwn::ScoobyElevator(ScoobyElevator { move_at, move_up, emissive, .. }) => {
                    Some((*move_at, *move_up, f64::from(*emissive)))
                }
                _ => None,
            }),
        }));
        out.switches.clear();
        out.switches.extend(idx.switches.iter().map(|&i| {
            let st = actor(i);
            SwitchSnap {
                shown: st.is_some_and(|a| !a.dead),
                a: st.and_then(|a| match &a.own {
                    StageActorOwn::ScoobySwitch(ScoobySwitch { move_at, move_up, done_at, chain1_at, .. }) => {
                        Some((*move_at, *move_up, *done_at, *chain1_at))
                    }
                    _ => None,
                }),
            }
        }));
        out.walls.clear();
        out.walls.extend(idx.walls.iter().map(|&i| {
            let st = actor(i);
            WallSnap {
                shown: st.is_none_or(|a| !a.dead),
                a: st.and_then(|a| match &a.own {
                    StageActorOwn::ScoobyWall(ScoobyWall { hit_at, gear_at, gear_up, broken_at, .. }) => {
                        Some((*hit_at, *gear_at, *gear_up, *broken_at))
                    }
                    _ => None,
                }),
            }
        }));
        out.tick = w.flow.tick;
        true
    }

    pub fn tick(&mut self, host: &mut SceneryHost, _dt: f64) {
        let mut snap = std::mem::take(&mut self.snap);
        let found = match host.world() {
            Some(w) => self.snapshot(w, &mut snap),
            None => false,
        };
        if !found {
            self.snap = snap;
            return;
        }
        let tick = snap.tick;
        for (e, s) in self.elevators.iter_mut().zip(&snap.elevators) {
            // the root's offset (UE Y = -sim x, Z = sim y) and its pass's move
            match &s.solid {
                Some(m) => e.blend.tick(-m.dx, m.dy, Some((m.mx, m.my))),
                None => e.blend.tick(0.0, 0.0, None),
            }
            let Some((move_at, move_up, emissive)) = s.a else {
                self.core.play_tick(host, e.comp, tick, None, -1);
                continue;
            };
            let play = (move_at >= 0).then_some(if move_up { e.up } else { e.down });
            self.core.play_tick(host, e.comp, tick, play, move_at);
            if emissive != e.emissive {
                e.emissive = emissive;
                host.skeletal_scalars(SkinRef::File(&e.skeletal), e.slot, &[("Emissive", emissive as f32)]);
            }
        }
        for (s, a) in self.switches.iter_mut().zip(&snap.switches) {
            if a.shown != s.shown {
                s.shown = a.shown;
                host.show_skeletal(&s.skeletal, a.shown);
            }
            let Some((move_at, move_up, done_at, chain1_at)) = a.a else { continue };
            let moved = move_at >= 0;
            // the end pose from the move's end, else the Release / Retract montage from the move
            if done_at >= 0 && done_at >= move_at {
                self.core.play_tick(host, s.comp, tick, Some(if move_up { s.up_pose } else { s.down_pose }), done_at);
            } else {
                let play = moved.then_some(if move_up { s.retract } else { s.release });
                self.core.play_tick(host, s.comp, tick, play, move_at);
            }
            let trap = moved.then_some(if move_up { s.trap_retract } else { s.trap_release });
            for &t in &s.trap {
                self.core.play_tick(host, t, tick, trap, move_at);
            }
            let wall = moved.then_some(if move_up { s.wall_retract } else { s.wall_release });
            self.core.play_tick(host, s.wall, tick, wall, move_at);
            s.p0.tick(host, move_at, tick, &s.system0, &s.at0);
            s.p1.tick(host, chain1_at, tick, &s.system1, &s.at1);
        }
        for (x, a) in self.walls.iter_mut().zip(&snap.walls) {
            if a.shown != x.shown {
                x.shown = a.shown;
                host.show_skeletal(&x.wall, a.shown);
                for g in &x.gears {
                    host.show_skeletal(g, a.shown);
                }
            }
            let Some((hit_at, gear_at, gear_up, broken_at)) = a.a else { continue };
            self.core.play_tick(host, x.comp, tick, (hit_at >= 0).then_some(x.impact_anim), hit_at);
            let gear = (gear_at >= 0).then_some(if gear_up { x.gear_retract } else { x.gear_release });
            for &g in &x.gear_comps {
                self.core.play_tick(host, g, tick, gear, gear_at);
            }
            x.impact.tick(host, hit_at, tick, &x.impact_system, &x.at_impact);
            x.destroy.tick(host, broken_at, tick, &x.destroy_system, &x.at_destroy);
        }
        self.snap = snap;
        self.core.pose_all(host);
        self.core.follow_all(host);
    }

    pub fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        for e in &mut self.elevators {
            let (dy, dz) = e.blend.at(alpha);
            if dy == e.dy && dz == e.dz {
                continue;
            }
            e.dy = dy;
            e.dz = dz;
            // (a translation in UE space: (0, dY, dZ))
            let mut m = e.base;
            m[3][1] += dy;
            m[3][2] += dz;
            host.place_skeletal(&e.skeletal, Some(m));
        }
    }

    pub fn end(&mut self, host: &mut SceneryHost) {
        let _ = host;
    }
}

// ---- scoobyMotion

#[derive(Debug, Deserialize)]
struct PlaceData {
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LightningData {
    system: String,
    delay: [f64; 2],
    sfx_delay: f64,
    event: String,
    at: PlaceData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LookAtData {
    bone: String,
    axis: [f64; 3],
    axis_local: bool,
    up: Option<[f64; 3]>,
    clamp: f64,
    interp: f64,
}

#[derive(Debug, Deserialize)]
struct StatueData {
    skeletal: String,
    #[serde(rename = "animBP")]
    anim_bp: String,
    #[serde(rename = "lookAt")]
    look_at: LookAtData,
    at: PlaceData,
}

#[derive(Debug, Deserialize)]
struct WalkData {
    system: String,
    loc: [f64; 3],
    rot: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct GhostsData {
    delay: [f64; 2],
    walks: Vec<WalkData>,
}

#[derive(Debug, Deserialize)]
struct DustfallData {
    system: String,
}

#[derive(Debug, Deserialize)]
struct MotionData {
    lightning: LightningData,
    statues: Vec<StatueData>,
    ghosts: GhostsData,
    dustfall: DustfallData,
}

const STATUE_A: &str = "ScoobyMansion_Statue_AnimBP_C";
const STATUE_B: &str = "ScoobyMansion_StatueB_AnimBP_C";
const SEARCH_DELAY: f64 = 0.5;
/// A dustAt this many ticks old or older when first seen is not spawned (a rollback's replay, a Lab reset, the
/// scenery joining a world already under way).
const DUST_LATE: i32 = 10;

type V3 = [f64; 3];

/// UE axes -> the content axes of a skeletal holder (UE_TO_THREE without its 0.01: a direction).
fn ue_dir(v: V3) -> V3 {
    [v[0], v[2], v[1]]
}

/// The UE to three.js axes: three's (x, y, z) = UE's (X, Z, Y) x 0.01.
const UE_TO_THREE: Mat4 = [0.01, 0.0, 0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];

// (the three.js Vector3 / Quaternion rules)
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn normalize(v: V3) -> V3 {
    let l = dot(v, v).sqrt();
    let k = 1.0 / if l == 0.0 { 1.0 } else { l };
    [v[0] * k, v[1] * k, v[2] * k]
}

/// Vector3.applyMatrix4.
fn apply_matrix(v: V3, e: &Mat4) -> V3 {
    let [x, y, z] = v;
    let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    [
        (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
        (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
        (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
    ]
}

/// Vector3.applyQuaternion.
fn apply_quat(v: V3, q: [f64; 4]) -> V3 {
    let [vx, vy, vz] = v;
    let [qx, qy, qz, qw] = q;
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    [vx + qw * tx + qy * tz - qz * ty, vy + qw * ty + qz * tx - qx * tz, vz + qw * tz + qx * ty - qy * tx]
}

/// Vector3.projectOnPlane.
fn project_on_plane(v: V3, n: V3) -> V3 {
    let denominator = dot(n, n);
    let p = if denominator == 0.0 {
        [0.0; 3]
    } else {
        let scalar = dot(n, v) / denominator;
        [n[0] * scalar, n[1] * scalar, n[2] * scalar]
    };
    sub(v, p)
}

fn quat_normalize(q: [f64; 4]) -> [f64; 4] {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l == 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let l = 1.0 / l;
    [q[0] * l, q[1] * l, q[2] * l, q[3] * l]
}

/// Quaternion.setFromUnitVectors.
fn quat_from_unit_vectors(from: V3, to: V3) -> [f64; 4] {
    let mut r = dot(from, to) + 1.0;
    let q = if r < 1e-8 {
        r = 0.0;
        if from[0].abs() > from[2].abs() { [-from[1], from[0], 0.0, r] } else { [0.0, -from[2], from[1], r] }
    } else {
        [from[1] * to[2] - from[2] * to[1], from[2] * to[0] - from[0] * to[2], from[0] * to[1] - from[1] * to[0], r]
    };
    quat_normalize(q)
}

/// A statue: its LookAt node's settings and the place its AnimBP finds a fighter to look at from.
#[derive(Debug)]
struct Statue {
    skeletal: String,
    anim_bp_a: bool,
    /// The bones from the root to the head's parent, the head.
    chain: Vec<usize>,
    head: usize,
    /// UE world -> the holder's content space: UE_TO_THREE x the component's UE transform^-1.
    to_content: Mat4,
    /// The look-at and look-up axes in the bone's space (content axes).
    axis: V3,
    up: Option<V3>,
    rest: mvs_view::anim::Pose,
    /// The fighter found (a player index), -1 none.
    fighter: i32,
    /// The search's Delay(0.5): time left.
    wait: f64,
    /// LookAtPoint (UE world).
    point: V3,
}

impl Statue {
    /// FAnimNode_LookAt on the reference pose (see the head comment): the head bone's local rotation, None where
    /// the pose stays the reference one.
    fn look_at(&self) -> Option<[f64; 4]> {
        let r = &self.rest;
        let v3 = |a: &[f32], i: usize| [f64::from(a[i * 3]), f64::from(a[i * 3 + 1]), f64::from(a[i * 3 + 2])];
        let mut parent_m = math::IDENTITY;
        let mut parent_q = [0.0, 0.0, 0.0, 1.0];
        for &i in &self.chain {
            let m = math::compose(v3(&r.t, i), r.quat(i), v3(&r.s, i));
            parent_m = math::mul(&parent_m, &m);
            parent_q = math::qmul(parent_q, r.quat(i));
        }
        let h = self.head;
        let head_pos = apply_matrix(v3(&r.t, h), &parent_m);
        let mut head_q = math::qmul(parent_q, r.quat(h));
        let mut to_target = sub(apply_matrix(self.point, &self.to_content), head_pos);
        if dot(to_target, to_target) < 1e-12 {
            return None;
        }
        to_target = normalize(to_target);
        let mut aim = apply_quat(self.axis, head_q);
        if let Some(up) = self.up {
            let up = apply_quat(up, head_q);
            to_target = project_on_plane(to_target, up);
            aim = project_on_plane(aim, up);
            if dot(to_target, to_target) < 1e-12 || dot(aim, aim) < 1e-12 {
                return None;
            }
            to_target = normalize(to_target);
            aim = normalize(aim);
        }
        let delta = quat_from_unit_vectors(aim, to_target);
        // the new component rotation, back into the bone's space under its parent
        head_q = math::qmul(delta, head_q);
        let inv = [-parent_q[0], -parent_q[1], -parent_q[2], parent_q[3]];
        Some(math::qmul(inv, head_q))
    }

    /// The pose: the reference pose, the head turned.
    fn pose(&self, host: &mut SceneryHost) {
        let q = self.look_at();
        let Some(p) = host.pose(&self.skeletal) else { return };
        p.copy(&self.rest);
        if let Some(q) = q {
            for (k, v) in q.iter().enumerate() {
                p.r[self.head * 4 + k] = *v as f32;
            }
        }
    }
}

/// One fighter as the statues and the dust read the World: its place (sim x, the actor's y) and team.
#[derive(Debug)]
struct FighterSnap {
    x: f64,
    actor_y: f64,
    team: i32,
}

#[derive(Debug)]
struct DustSnap {
    index: u32,
    at: i32,
    x: f64,
    y: f64,
    loc_x: f64,
}

#[derive(Debug, Default)]
struct MotionSnap {
    tick: i32,
    /// The world has a stage run.
    staged: bool,
    fighters: Vec<FighterSnap>,
    dust: Vec<DustSnap>,
}

/// The `scoobyMotion` block.
#[derive(Debug)]
pub struct ScoobyMotion {
    d: MotionData,
    statues: Vec<Statue>,
    /// The random wait's time left.
    lightning: f64,
    /// The SFX Delay's time left, -1 while not waiting on it.
    sfx: f64,
    ghost: f64,
    /// The walk the next wait spawns.
    next: usize,
    /// The elevators' dust: the dustAt last drawn, by StageData.placed index.
    dust_seen: Vec<(u32, i32)>,
    dust_synced: bool,
    /// The last tick's snapshot, kept for its buffers.
    snap: MotionSnap,
    /// The match's players, kept for its buffer.
    players: Vec<usize>,
}

/// SpawnEmitterAtLocation of a level actor: first ticked the frame after.
fn spawn(host: &mut SceneryHost, system: &str, loc: V3, rot: V3, scale: V3) {
    let at = Attachment { loc, rot: rows_nested(&rows_of(&rot)), scale: Some(scale) };
    if let Some(v) = host.vfx() {
        v.spawn_level(system, &at, true, &[], 0, false);
    }
}

impl ScoobyMotion {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<ScoobyMotion> {
        let what = format!("scenery: scooby {key}");
        let d: MotionData = serde_json::from_value(data.clone()).map_err(|e| bad(&what, e.to_string()))?;
        let mut statues = Vec::new();
        for sd in &d.statues {
            let name = &sd.skeletal;
            let l = &sd.look_at;
            let statue = format!("scenery: scooby statue {name}");
            let (Some(k), Some(anim)) = (host.skeletal_index(name), host.skeletal(name)) else {
                return Err(bad(&statue, "not a skeletal mesh of the file"));
            };
            if anim.clip.is_some()
                || (sd.anim_bp != STATUE_A && sd.anim_bp != STATUE_B)
                || l.clamp != 0.0
                || l.interp != 0.0
                || !l.axis_local
            {
                return Err(bad(&statue, "not a statue AnimBP's LookAt as decoded"));
            }
            let (loc, rot, scale) = (sd.at.loc, sd.at.rot, sd.at.scale);
            let rest = host.rest(name).cloned().ok_or_else(|| bad(&statue, "no reference pose"))?;
            let bones = host.state.skins[k].info.bones().to_vec();
            let parents =
                host.state.skins[k].info.skeleton.as_ref().expect("a skeletal mesh's skeleton").parents.clone();
            let head =
                bones.iter().position(|b| *b == l.bone).ok_or_else(|| bad(&statue, format!("has no {}", l.bone)))?;
            let mut chain = Vec::new();
            let mut p = parents[head];
            while p >= 0 {
                chain.insert(0, usize::try_from(p).expect("a bone"));
                p = parents[usize::try_from(p).expect("a bone")];
            }
            let [rx, ry, rz] = rot;
            let x = math::compose(loc, math::rotator_quat(rx, ry, rz), scale);
            host.drive(name);
            statues.push(Statue {
                skeletal: name.clone(),
                anim_bp_a: sd.anim_bp == STATUE_A,
                chain,
                head,
                to_content: math::mul(&UE_TO_THREE, &math::invert(&x)),
                axis: normalize(ue_dir(l.axis)),
                up: l.up.map(|u| normalize(ue_dir(u))),
                rest,
                fighter: -1,
                wait: SEARCH_DELAY,
                point: [0.0; 3],
            });
        }
        Ok(ScoobyMotion {
            d,
            statues,
            lightning: 0.0,
            sfx: -1.0,
            ghost: 0.0,
            next: 0,
            dust_seen: Vec::new(),
            dust_synced: false,
            snap: MotionSnap::default(),
            players: Vec::new(),
        })
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        for s in &mut self.statues {
            s.fighter = -1;
            s.wait = SEARCH_DELAY;
            s.point = [0.0; 3];
        }
        self.lightning = host.rand(self.d.lightning.delay);
        self.sfx = -1.0;
        self.ghost = host.rand(self.d.ghosts.delay);
        self.next = 0;
        self.dust_seen.clear();
        self.dust_synced = false;
    }

    fn snapshot(w: &World, out: &mut MotionSnap) {
        out.fighters.clear();
        out.fighters.extend(w.fighters.iter().map(|(id, f)| FighterSnap {
            x: f64::from(f.body.x),
            actor_y: f64::from(actor_y(w, id)),
            team: f.team.team,
        }));
        let placed = w.data.stage.placed.as_deref().unwrap_or(&[]);
        let dust = &mut out.dust;
        dust.clear();
        for a in w.stage_run.iter().flat_map(|r| &r.actors) {
            let StageActorOwn::ScoobyElevator(e) = &a.own else { continue };
            let Some(p) = placed.get(a.index as usize) else { continue };
            dust.push(DustSnap {
                index: a.index,
                at: e.dust_at,
                x: f64::from(e.dust_x),
                y: f64::from(e.dust_y),
                loc_x: p.loc[0],
            });
        }
        out.tick = w.flow.tick;
        out.staged = w.stage_run.is_some();
    }

    /// GetMyPlayerData: the fighter each AnimBP looks for (no fighters: none).
    fn find(s: &Statue, fs: &[FighterSnap], m: Option<(usize, &[usize])>) -> i32 {
        if fs.is_empty() {
            return -1;
        }
        // (with no match, e.g. a picture without one: the local player is fighter 0, the players the fighters in order)
        let me = m.map_or(0, |m| m.0);
        // (the actor loop: the fighter of that player, else the last one tried)
        let actor_of =
            |player: usize| i32::try_from(if player < fs.len() { player } else { fs.len() - 1 }).expect("a fighter");
        if s.anim_bp_a {
            return actor_of(me);
        }
        let n = m.map_or(fs.len(), |m| m.1.len());
        if n == 0 {
            return -1;
        }
        let at = |k: usize| m.map_or(k, |m| m.1[k]);
        let mine = fs.get(me).map(|f| f.team);
        for k in 0..n {
            if at(k) != me && fs.get(at(k)).map(|f| f.team) == mine && mine.is_some() {
                return actor_of(at(k));
            }
        }
        let mut k = 0;
        while k < n - 1 && at(k) == me {
            k += 1;
        }
        actor_of(at(k))
    }

    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        let mut snap = std::mem::take(&mut self.snap);
        let have = match host.world() {
            Some(w) => {
                Self::snapshot(w, &mut snap);
                true
            }
            None => false,
        };
        // the lightning
        if self.sfx >= 0.0 {
            self.sfx -= dt;
            if self.sfx <= 0.0 {
                self.sfx = -1.0;
                let l = &self.d.lightning;
                spawn(host, &l.system, l.at.loc, l.at.rot, l.at.scale);
                self.lightning = host.rand(l.delay);
            }
        } else {
            self.lightning -= dt;
            if self.lightning <= 0.0 {
                host.sound(&self.d.lightning.event, [0.0; 3], None);
                self.sfx = self.d.lightning.sfx_delay;
            }
        }
        // the ghosts
        self.ghost -= dt;
        if self.ghost <= 0.0 {
            let g = &self.d.ghosts.walks[self.next];
            spawn(host, &g.system, g.loc, g.rot, [1.0; 3]);
            self.next = (self.next + 1) % self.d.ghosts.walks.len();
            self.ghost = host.rand(self.d.ghosts.delay);
        }
        // the statues: BlueprintUpdateAnimation (the mesh's tick: LookAtPoint from the fighter found), then the
        // search's Delay in the leftover latent pass (a fighter found there is read from the next tick); the pose is
        // the skeletal tick's, after this (it is written here: the same numbers)
        self.players.clear();
        let me = host.scenery_match().map(|m| {
            self.players.extend_from_slice(&m.players);
            m.me
        });
        let empty = [];
        let fs = if have { &snap.fighters[..] } else { &empty[..] };
        for s in &mut self.statues {
            if let Some(f) = usize::try_from(s.fighter).ok().and_then(|i| fs.get(i)) {
                s.point = [0.0, -f.x, f.actor_y];
            }
            if s.fighter < 0 {
                s.wait -= dt;
                if s.wait <= 0.0 {
                    s.fighter = Self::find(s, fs, me.map(|me| (me, &self.players[..])));
                    if s.fighter < 0 {
                        s.wait = SEARCH_DELAY;
                    }
                }
            }
            s.pose(host);
        }
        self.dust(host, have.then_some(&snap));
        self.snap = snap;
    }

    /// The elevators' PS_Mansion_Dustfall from the sim's dustAt, once per new dustAt.
    fn dust(&mut self, host: &mut SceneryHost, w: Option<&MotionSnap>) {
        let Some(w) = w.filter(|w| w.staged) else {
            self.dust_synced = false;
            return;
        };
        for d in &w.dust {
            let seen = self.dust_seen.iter().position(|(i, _)| *i == d.index);
            if seen.map_or(-1, |k| self.dust_seen[k].1) == d.at {
                continue;
            }
            match seen {
                Some(k) => self.dust_seen[k].1 = d.at,
                None => self.dust_seen.push((d.index, d.at)),
            }
            if !self.dust_synced || d.at < 0 || w.tick - d.at >= DUST_LATE || d.at > w.tick {
                continue;
            }
            let system = self.d.dustfall.system.clone();
            spawn(host, &system, [d.loc_x, -d.x, d.y], [0.0; 3], [1.0; 3]);
        }
        self.dust_synced = true;
    }

    pub fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        let _ = (host, alpha);
    }

    pub fn end(&mut self, host: &mut SceneryHost) {
        let _ = host;
    }
}
