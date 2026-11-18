//! Cromulons' speakers (M009_DestructiblePlatformBP_C / _Child_C) drawn from their sim state
//! (mvs-sim stages/cromulons/speaker.rs and world/wall_bp.rs: the Speaker
//! in World.stage_run.actors, read each tick through SceneryHost::world). The data: the block `cromulons`'s
//! `speakerArt` (the importer's scenery part cromulons.rs); the graphs: M009_DestructiblePlatformBP,
//! SpeakerSkelMesh_Skeleton_AnimBlueprint, WallBP.
//!
//! - The mesh by `mesh` (UpdateMesh @4332: SetSkeletalMesh(NewMesh, reinit pose)): 0 the placed SpeakerSkelMesh, 1
//!   SpeakerSkelMesh_DamageState1, 2 _DamageState3, made at make (SceneryHost::skinned) and posed with the placed
//!   mesh's pose. Slot 0 the NewMaterial (speaker_platform_LightDamage_Mat / HeavyDamage_Mat), slots 1..3 the three
//!   dynamic instances of SpeakerAudioVisulizer_MAT BeginPlay made (@2829..3184, on the placed mesh's slots 0..2):
//!   Break 1 from stage 1 (DynamicMATs[0], @679), Break2 1 with the second UpdateMesh (DynamicMATs[2], @1013). A
//!   dynamic instance shared by two meshes is set on both.
//! - The music: the Tick's Scroll of each instance (@2149..2356: FMax(NormalizeToRange(FrequencyAmount[i], -30, 0),
//!   0.05)) and the AnimBP's ModifyBone inputs (TheBeat 45 x u, SpeakerScale 0.75 + 0.5 x u; SmallBeat 25 x u',
//!   SmallSpeakerScale 0.75 + 0.65 x u'; u = NormalizeToRange(mus_freq_analyzer_01, -48, 0), u' the same of _04) read
//!   the Wwise RTPCs mus_freq_analyzer_01..04 (01..03 the speaker's Tick @3717..4081 by the music's playing id, 01 the
//!   AnimBP's @1253 by it, 04 @1579 by playing id 0), which AkMeterFX on the music's aux buses writes: read through
//!   SceneryHost::rtpc each tick, as ./motion.rs reads them for the visualizer (a parameter nothing has set taken as
//!   the meters' floor -48, which the port's music, with no aux sends, leaves them at: Scroll 0.05, TheBeat 0,
//!   SpeakerScale 0.75, SmallBeat 0, SmallSpeakerScale 0.75).
//! - SpeakerSkelMesh_Skeleton_AnimBlueprint_C (the data's node links): the state machine DamageStates (NoDamage
//!   looping, DamageState1 and DamageState2 once; DamageState = CurrentStage, > 0 and > 1, crossfades 0) under a
//!   LayeredBoneBlend that takes Engine and Engine1 and their children from Speaker_Engine_Shake (looping, local space,
//!   weight 1), the five ModifyBone nodes (translation and scale replaced in bone space: the bone's local transform
//!   composed under (beat, 0, 0) at the scale), ApplyAdditive of the slot DefaultSlot over Speaker_Idle_Additive (the
//!   hit montages, additive: accumulated onto the idle's delta), then the slot Override (the destroy montage). So a
//!   hit that changes the stage shows no hit montage (DamageVisuals' Montage_Play comes before UpdateMesh in the
//!   damage) and no PS_SpeakerDamage_Hit; the state machine starts in NoDamage on that tick with its first update's
//!   transitions skipped (bSkipFirstUpdateTransition) and enters the stage's state on the next, its sequence from 0.
//! - The montages' position: the ticks since the one they were played in plus that tick's own, x 1/60 (the tree
//!   fort's pillars' "+1" rule and its Unverified note). The hit montages (hit_at / hit_montage:
//!   Speaker_Hit_Anim_Montage and _DamageState1 / _2_Montage) have a PlayVFX notify state, PS_SpeakerDamage_Hit
//!   attached at Root (the trees' notify rule). Speaker_Destroyed_Montage from off_at; its OnCompleted (WallBP @1391 ->
//!   @791, Destroyed_0) hides the mesh.
//! - UpdateMesh's PS_SpeakerDamage1 at the root socket's location (@1164: rotation 0, scale 1) and with the second
//!   SpeakerDamageState2Multicast's PS_SpeakerDamage2 attached at Root (@4932): spawned when mesh_at changes (a tick
//!   seen late still spawns within 6), first ticked the next frame.
//! - PS_SchwiftyDamageSparks (its template PS_SpeakerDamageIdle, under CollisionBox): `sparks`.
//! - The thrusters (spawned by ../cromulons.rs) hidden when `thrusters` goes false (DestroyDestructible @3263 /
//!   @3297).
//! - The sparks' OnParticleSpawn (SFX @4467): the event named Lead posts sfx_map_cromulon_electric_lead_a, Lead1 _b,
//!   Lead4 _c when the particle's Z is in [80, 150] (InRange, both ends in, @4727), through the BP's PostEvent
//!   (@0..215: the Audio_Emit_Sparks_A / B / C child's AkEvent set, then its ManualStart: the event on that child's
//!   AkComponent, where the stage data places it; ParticleLocation is not used for the place).
// Unverified: ModifyBone's scale-then-translation order and the bone space conversions are 4.26 source memory, not
// read in the exe.
// Unverified: SetSkeletalMesh with reinit pose taken as InitializeAnimation (4.26 source memory): the montages
// stopped and the anim graph started over.
// Unverified: the analyzers' value by playing id taken as the global one (./motion.rs).
// Deviation: PS_SchwiftyDamageSparks is spawned when it is shown and killed when hidden; the build's component is
// active and hidden from BeginPlay (its particles already running when UpdateMesh shows it).
// Unverified: the sparks' OnParticleSpawn sounds are posted at the scenery's next tick, from the particle player's
// spawn events of its tick: one tick after the particle tick.

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_vfx::{Attachment, SystemId};
use mvs_view::anim::{Pose, accumulate_held};
use serde::Deserialize;

use super::super::host::{SceneryHost, SkinRef, SkinnedId};
use super::super::sky_arena::attachment_of;
use crate::Result;
use crate::stage::{bad, transform};

/// SFX @4467's events (Lead, Lead1, Lead4).
pub(in crate::scenery) const SPARK_SOUNDS: [&str; 3] =
    ["sfx_map_cromulon_electric_lead_a", "sfx_map_cromulon_electric_lead_b", "sfx_map_cromulon_electric_lead_c"];

const STEP: f64 = 1.0 / 60.0;
const FX_LATE: i32 = 6;

#[derive(Clone, Debug, Deserialize)]
struct Segment {
    anim: String,
    pos: f64,
    start: f64,
    end: f64,
    rate: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct Notify {
    at: f64,
    #[serde(default)]
    end: Option<f64>,
    system: String,
    socket: String,
    kill: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Montage {
    montage: String,
    slot: String,
    length: f64,
    rate_scale: f64,
    segments: Vec<Segment>,
    notifies: Vec<Notify>,
}

#[derive(Clone, Debug, Deserialize)]
struct Player {
    anim: String,
    #[serde(rename = "loop")]
    looping: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct Shake {
    anim: String,
    bones: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Modify {
    bone: String,
    large: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct AnimBp {
    states: Vec<Player>,
    shake: Shake,
    idle: Player,
    modify: Vec<Modify>,
    hit: IndexMap<String, Montage>,
    destroy: Montage,
}

#[derive(Clone, Debug, Deserialize)]
struct DamageMesh {
    mesh: String,
    materials: Vec<Option<String>>,
}

/// The block's `speakerArt`: the damage drawing's assets, once for both speakers.
#[derive(Clone, Debug, Deserialize)]
pub(super) struct SpeakerArt {
    visualizer: String,
    base: Vec<usize>,
    damaged: Vec<usize>,
    damage: Vec<DamageMesh>,
    stage1: String,
    stage2: String,
    #[serde(rename = "animBP")]
    anim_bp: AnimBp,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct Sparks {
    system: String,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

/// A speaker of the block's `speakers`: its skeletal mesh (the actor's name) and its sparks' place.
#[derive(Clone, Debug, Deserialize)]
pub(super) struct SpeakerData {
    pub(super) skeletal: String,
    sparks: Sparks,
}

/// A clip of the scenery: its path, length and whether it is additive.
#[derive(Clone, Debug)]
struct Clip {
    path: String,
    length: f64,
}

/// The speaker's sim state as this tick reads it.
#[derive(Clone, Debug)]
struct Seen<'w> {
    tick: i32,
    stage: i32,
    mesh: i32,
    mesh_at: i32,
    hit_at: i32,
    hit_montage: &'w str,
    off_at: i32,
    sparks: bool,
    thrusters: bool,
    /// The Audio_Emit_Sparks_A / B / C children's game objects and places, read when the sparks are to be spawned.
    spark_keys: Option<[(String, [f64; 3]); 3]>,
}

/// One speaker as it is drawn.
#[derive(Debug)]
struct Sp {
    d: SpeakerData,
    /// The damage meshes (mesh 1, 2).
    sets: Vec<SkinnedId>,
    /// The shake layer's bones, the ModifyBone nodes' (bone, large), every bone.
    layer: Vec<usize>,
    modify: Vec<(usize, bool)>,
    all: Vec<usize>,
    poses: [Pose; 3],
    ta: Pose,
    /// The anim instance's own clock (the driven SceneryAnim's time).
    time: f64,
    /// Drawn: -1 hidden (the destroy montage ended).
    mesh: i32,
    /// Break, Break2 set (bits).
    brk: u32,
    /// This tick's anim graph (from the sim's fields): the tick the anim instance (re)started (-1: the match's start:
    /// its own clock).
    init_at: i32,
    state: usize,
    hit: Option<usize>,
    hit_pos: f64,
    destroy_pos: f64,
    hit_start: i32,
    hit_seen: f64,
    hit_fx: Option<SystemId>,
    seen_mesh: i32,
    stage_fx: Option<SystemId>,
    sparks: Option<SystemId>,
    spark_keys: Option<[(String, [f64; 3]); 3]>,
    sparks_at: Attachment,
    thrusters: bool,
    synced: bool,
    /// The systems that follow the Root socket (VfxPlayer's follow): moved before each particle tick.
    follow: Vec<SystemId>,
}

/// The speakers' drawing, for the block `cromulons` (../cromulons.rs).
#[derive(Debug)]
pub(super) struct Speakers {
    art: SpeakerArt,
    sps: Vec<Sp>,
    states: Vec<Clip>,
    shake: Clip,
    idle: Clip,
    /// The montages' clips by segment anim path.
    hit_keys: Vec<String>,
    /// The music as the last tick read it: u (analyzer 01) and u' (04) for the bones.
    u: f64,
    u4: f64,
    /// The world's tick as the last tick read it.
    w0tick: i32,
    /// Speaker i's thruster systems (../cromulons.rs spawns them at begin).
    pub(super) thruster_fx: Vec<Vec<Option<SystemId>>>,
}

fn clip(host: &SceneryHost, path: &str) -> Result<(Clip, bool)> {
    let clips = &host.state.clips;
    let c = clips.get(path).ok_or_else(|| bad("scenery: cromulons", format!("the clip {path} is not loaded")))?;
    let rc = &clips.clips[c];
    Ok((Clip { path: path.to_owned(), length: rc.anim.length }, rc.additive))
}

/// A montage's segment at a position: the clip and its time.
fn segment_at(m: &Montage, pos: f64) -> (&str, f64) {
    let mut s = &m.segments[0];
    for x in &m.segments {
        if pos >= x.pos {
            s = x;
        }
    }
    (&s.anim, s.end.min(s.start + (pos - s.pos) * s.rate))
}

impl Speakers {
    pub(super) fn make(art: SpeakerArt, list: &[SpeakerData], host: &mut SceneryHost) -> Result<Speakers> {
        let what = "scenery: cromulons";
        let a = &art.anim_bp;
        let states = a.states.iter().map(|s| clip(host, &s.anim).map(|c| c.0)).collect::<Result<Vec<_>>>()?;
        let shake = clip(host, &a.shake.anim)?.0;
        let (idle, additive) = clip(host, &a.idle.anim)?;
        if !additive {
            return Err(bad(what, format!("{} is not additive", a.idle.anim)));
        }
        let destroy_additive = a.destroy.segments.iter().map(|s| clip(host, &s.anim).map(|c| c.1));
        if a.destroy.slot != "Override" || destroy_additive.collect::<Result<Vec<_>>>()?.into_iter().any(|x| x) {
            return Err(bad(what, format!("{} slot", a.destroy.montage)));
        }
        for m in a.hit.values() {
            let adds = m.segments.iter().map(|s| clip(host, &s.anim).map(|c| c.1)).collect::<Result<Vec<_>>>()?;
            if m.slot != "DefaultSlot" || adds.iter().any(|x| !x) || m.segments.is_empty() {
                return Err(bad(what, format!("{} is not an additive DefaultSlot montage", m.montage)));
            }
        }
        if a.destroy.segments.is_empty() || art.base.len() != 3 || art.damaged.len() != 3 {
            return Err(bad(what, "speakerArt"));
        }
        let mut sps = Vec::new();
        for d in list {
            let name = d.skeletal.as_str();
            let k = host.skeletal_index(name).ok_or_else(|| bad(what, format!("{name} is not a skeletal mesh")))?;
            // BeginPlay's dynamic instances on the placed mesh's slots
            if host.skeletals()[k].materials.len() != 4 {
                return Err(bad(what, format!("{name} sections")));
            }
            let info = host.state.skins[k].info.clone();
            let names = info.bones();
            let parents = &info.skeleton.as_ref().ok_or_else(|| bad(name, "no skeleton"))?.parents;
            let idx = |b: &str| names.iter().position(|n| n == b).ok_or_else(|| bad(name, format!("no bone {b}")));
            idx("Root")?;
            // the bones the shake layer takes: the branch bones and their children (BlendDepth 0)
            let branch = a.shake.bones.iter().map(|b| idx(b)).collect::<Result<Vec<_>>>()?;
            let layer = (0..names.len())
                .filter(|&b| {
                    let mut o = b as i32;
                    while o >= 0 {
                        if branch.contains(&(o as usize)) {
                            return true;
                        }
                        o = parents[o as usize];
                    }
                    false
                })
                .collect();
            let modify = a.modify.iter().map(|m| idx(&m.bone).map(|b| (b, m.large))).collect::<Result<Vec<_>>>()?;
            let n = info.rest.n;
            for &slot in &art.base {
                host.skeletal_material(SkinRef::File(name), slot, Some(&art.visualizer));
            }
            let mut sets = Vec::new();
            for dm in &art.damage {
                let mats: Vec<Option<&str>> = dm.materials.iter().map(Option::as_deref).collect();
                let id = host.skinned(name, &dm.mesh, &mats)?;
                for &slot in &art.damaged {
                    host.skeletal_material(SkinRef::Own(id), slot, Some(&art.visualizer));
                }
                sets.push(id);
            }
            // the AnimBP poses the placed mesh from now on
            host.drive(name);
            let s = &d.sparks;
            sps.push(Sp {
                d: d.clone(),
                sets,
                layer,
                modify,
                all: (0..n).collect(),
                poses: [Pose::new(n), Pose::new(n), Pose::new(n)],
                ta: Pose::new(n),
                time: 0.0,
                mesh: 0,
                brk: 0,
                init_at: -1,
                state: 0,
                hit: None,
                hit_pos: -1.0,
                destroy_pos: -1.0,
                hit_start: -1,
                hit_seen: 0.0,
                hit_fx: None,
                seen_mesh: -1,
                stage_fx: None,
                sparks: None,
                spark_keys: None,
                sparks_at: attachment_of(&transform(s.loc, s.rot, s.scale)),
                thrusters: true,
                synced: false,
                follow: Vec::new(),
            });
        }
        let hit_keys = a.hit.keys().cloned().collect();
        let n = sps.len();
        Ok(Speakers {
            art,
            sps,
            states,
            shake,
            idle,
            hit_keys,
            u: 0.0,
            u4: 0.0,
            w0tick: 0,
            thruster_fx: vec![Vec::new(); n],
        })
    }

    pub(super) fn begin(&mut self, host: &mut SceneryHost) {
        // (../cromulons.rs spawns the thrusters again, shown)
        for i in 0..self.sps.len() {
            self.kill_fx(host, i);
            let sp = &mut self.sps[i];
            sp.hit = None;
            sp.hit_pos = -1.0;
            sp.destroy_pos = -1.0;
            sp.init_at = -1;
            sp.state = 0;
            sp.synced = false;
            sp.time = 0.0;
            sp.follow.clear();
            self.show(host, i, 0);
            self.sps[i].thrusters = true;
        }
    }

    fn kill_fx(&mut self, host: &mut SceneryHost, i: usize) {
        let sp = &mut self.sps[i];
        let ids = [sp.hit_fx.take(), sp.stage_fx.take(), sp.sparks.take()];
        if let Some(v) = host.vfx() {
            for id in ids.into_iter().flatten() {
                v.kill(id);
            }
        }
    }

    fn show(&mut self, host: &mut SceneryHost, i: usize, mesh: i32) {
        let sp = &mut self.sps[i];
        if mesh == sp.mesh {
            return;
        }
        let mut set = |m: i32, on: bool| match m {
            0 => host.show_skeletal(&sp.d.skeletal, on),
            m if m > 0 => host.show_skinned(sp.sets[m as usize - 1], on),
            _ => {}
        };
        set(sp.mesh, false);
        set(mesh, true);
        sp.mesh = mesh;
    }

    fn set_thrusters(&mut self, host: &mut SceneryHost, i: usize, on: bool) {
        if on == self.sps[i].thrusters {
            return;
        }
        self.sps[i].thrusters = on;
        if let Some(v) = host.vfx() {
            for id in self.thruster_fx[i].iter().flatten() {
                v.set_visible(*id, on);
            }
        }
    }

    /// A dynamic instance's scalars on every mesh that shows it: instance j is the placed mesh's slot base[j] and
    /// each damage mesh's damaged[j].
    fn dyn_scalars(&self, host: &mut SceneryHost, i: usize, j: usize, scalars: &[(&str, f32)]) {
        let sp = &self.sps[i];
        host.skeletal_scalars(SkinRef::File(&sp.d.skeletal), self.art.base[j], scalars);
        for &id in &sp.sets {
            host.skeletal_scalars(SkinRef::Own(id), self.art.damaged[j], scalars);
        }
    }

    /// The socket Root's world transform (UE).
    fn root_world(host: &SceneryHost, sp: &Sp) -> Attachment {
        attachment_of(&host.bone_world(&sp.d.skeletal, "Root").expect("the speaker's bone Root"))
    }

    /// The speaker's sim state (the Speaker whose placed actor is the speaker's skeletal mesh), None with no sim actor.
    fn read<'w>(w: &'w World, sp: &Sp) -> Option<Seen<'w>> {
        let placed = w.data.stage.placed.as_deref().unwrap_or(&[]);
        let actors = w.stage_run.as_ref().map_or(&[][..], |r| r.actors.as_slice());
        let a = actors.iter().rfind(|a| placed.get(a.index as usize).is_some_and(|p| p.name == sp.d.skeletal))?;
        let StageActorOwn::Speaker(s) = &a.own else {
            panic!("scenery: cromulons: the stage actor {} of {} is no Speaker", a.index, sp.d.skeletal)
        };
        let tick = w.flow.tick;
        // SFX @4467 for one speaker: its Audio_Emit_Sparks children's game objects (the stage data's emitters of the
        // actor), read when the sparks are spawned
        let spark_keys = (s.sparks && sp.sparks.is_none()).then(|| {
            let emitters = w.data.stage.audio.as_ref().map_or(&[][..], |a| a.emitters.as_slice());
            let child = |c: &str| {
                let e = emitters
                    .iter()
                    .find(|m| m.actor.as_deref() == Some(&sp.d.skeletal) && m.component.as_deref() == Some(c))
                    .unwrap_or_else(|| panic!("scenery: cromulons speaker {} without {c}", sp.d.skeletal));
                (format!("{}.{c}", sp.d.skeletal), [e.at[0], e.at[1], e.at[2]])
            };
            [child("Audio_Emit_Sparks_A"), child("Audio_Emit_Sparks_B"), child("Audio_Emit_Sparks_C")]
        });
        Some(Seen {
            tick,
            stage: s.wall.stage,
            mesh: s.mesh,
            mesh_at: s.mesh_at,
            hit_at: s.wall.hit_at,
            hit_montage: &s.wall.hit_montage,
            off_at: s.wall.off_at,
            sparks: s.sparks,
            thrusters: s.thrusters,
            spark_keys,
        })
    }

    /// One tick: the music, each speaker's state from the World (with one), then the AnimBP's pose; then what follows
    /// the speakers' bones is moved.
    pub(super) fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        self.spark_sounds(host);
        if host.world().is_some() {
            // NormalizeToRange(F, -48, 0) for the bones, FMax(NormalizeToRange(F, -30, 0), 0.05) for Scroll
            let f = |host: &SceneryHost, n: usize| {
                host.rtpc(super::motion::ANALYZERS[n]).unwrap_or(super::motion::METER_MIN)
            };
            self.u = (f(host, 0) + 48.0) / 48.0;
            self.u4 = (f(host, 3) + 48.0) / 48.0;
            let scroll: [f64; 3] = std::array::from_fn(|n| ((f(host, n) + 30.0) / 30.0).max(0.05));
            for i in 0..self.sps.len() {
                for (n, v) in scroll.iter().enumerate() {
                    self.dyn_scalars(host, i, n, &[("Scroll", *v as f32)]);
                }
            }
            let w = host.world().expect("a World");
            self.w0tick = w.flow.tick;
            for i in 0..self.sps.len() {
                let s = Self::read(w, &self.sps[i]);
                self.speaker_tick(host, i, s);
            }
        }
        for i in 0..self.sps.len() {
            self.drive(host, i, dt);
        }
        for sp in &mut self.sps {
            let at = Self::root_world(host, sp);
            let Some(v) = host.vfx() else { continue };
            sp.follow.retain(|&id| v.alive(id));
            for &id in &sp.follow {
                v.attach(id, &at);
            }
        }
    }

    /// The sparks' OnParticleSpawn of the particle player's last tick (SFX @4467).
    fn spark_sounds(&mut self, host: &mut SceneryHost) {
        let mut posts: Vec<(&str, String, [f64; 3])> = Vec::new();
        for sp in &self.sps {
            let (Some(id), Some(keys)) = (sp.sparks, &sp.spark_keys) else { continue };
            let Some(v) = host.vfx() else { continue };
            for (name, loc) in v.spawned(id) {
                let n = name.to_lowercase();
                if n == "lead" {
                    posts.push((SPARK_SOUNDS[0], keys[0].0.clone(), keys[0].1));
                } else if n == "lead1" {
                    posts.push((SPARK_SOUNDS[1], keys[1].0.clone(), keys[1].1));
                } else if n == "lead4" && loc[2] >= 80.0 && loc[2] <= 150.0 {
                    posts.push((SPARK_SOUNDS[2], keys[2].0.clone(), keys[2].1));
                }
            }
        }
        for (event, key, at) in posts {
            host.sound_on(event, &key, at);
        }
    }

    fn speaker_tick(&mut self, host: &mut SceneryHost, i: usize, s: Option<Seen<'_>>) {
        let a = &self.art.anim_bp;
        let sp = &mut self.sps[i];
        let Some(s) = s else {
            // no sim actor (a World with no stage actors): the start's drawing
            sp.init_at = -1;
            sp.state = 0;
            sp.hit = None;
            sp.hit_pos = -1.0;
            sp.destroy_pos = -1.0;
            return;
        };
        if !sp.synced {
            sp.seen_mesh = s.mesh_at;
            sp.hit_start = s.hit_at;
            sp.hit_seen = f64::MAX;
            sp.synced = true;
        }
        let tick = s.tick;
        // the anim instance: started over by the last UpdateMesh
        sp.init_at = s.mesh_at;
        sp.state = if s.mesh_at >= 0 && tick > s.mesh_at {
            usize::try_from(s.stage).unwrap_or(0).min(a.states.len() - 1)
        } else {
            0
        };
        // the destroy montage, and its end (OnCompleted: hidden)
        let destroy = &a.destroy;
        let dp = if s.off_at >= 0 { f64::from(tick - s.off_at + 1) * STEP * destroy.rate_scale } else { -1.0 };
        let gone = dp >= destroy.length;
        sp.destroy_pos = if dp >= 0.0 && !gone { dp } else { -1.0 };
        // the hit montage while it plays, unless an UpdateMesh at or after its start stopped it
        let playing = s.hit_at >= 0 && s.hit_at > s.mesh_at;
        let m = if playing {
            let k = self.hit_keys.iter().position(|k| *k == s.hit_montage);
            Some(k.unwrap_or_else(|| panic!("scenery: speaker montage {}", s.hit_montage)))
        } else {
            None
        };
        let hm = m.map(|k| &a.hit[k]);
        let hp = hm.map_or(-1.0, |m| f64::from(tick - s.hit_at + 1) * STEP * m.rate_scale);
        sp.hit = m;
        sp.hit_pos = match hm {
            Some(m) if hp < m.length => hp,
            _ => -1.0,
        };
        // its notify (PlayVFX: PS_SpeakerDamage_Hit attached at Root) in (last position, position]; a start put back
        // (a rollback) fires none of the positions already passed
        let start = if m.is_some() { s.hit_at } else { -1 };
        if start > sp.hit_start {
            sp.hit_start = start;
            sp.hit_seen = 0.0;
        } else if start != sp.hit_start {
            sp.hit_start = start;
            sp.hit_seen = if sp.hit_pos >= 0.0 { sp.hit_pos } else { f64::MAX };
        }
        if let Some(m) = hm
            && sp.hit_pos >= 0.0
            && sp.hit_pos > sp.hit_seen
        {
            for nt in &m.notifies {
                if nt.at > sp.hit_seen && nt.at <= sp.hit_pos {
                    if nt.socket != "Root" {
                        panic!("scenery: {} notify socket {}", m.montage, nt.socket);
                    }
                    let at = Self::root_world(host, sp);
                    if let Some(v) = host.vfx() {
                        if let Some(id) = sp.hit_fx {
                            v.deactivate(id);
                        }
                        sp.hit_fx = v.spawn_level(&nt.system, &at, false, &[], 0, false);
                        sp.follow.extend(sp.hit_fx);
                    }
                }
                if let Some(end) = nt.end
                    && nt.kill
                    && end > sp.hit_seen
                    && end <= sp.hit_pos
                {
                    if let (Some(id), Some(v)) = (sp.hit_fx, host.vfx()) {
                        v.deactivate(id);
                    }
                    sp.hit_fx = None;
                }
            }
        }
        if sp.hit_pos >= 0.0 {
            sp.hit_seen = sp.hit_pos;
        }
        // the mesh and its materials' Break / Break2
        self.show(host, i, if gone { -1 } else { s.mesh });
        let brk = u32::from(s.mesh >= 1) | if s.mesh >= 2 { 2 } else { 0 };
        if brk != self.sps[i].brk {
            self.sps[i].brk = brk;
            self.dyn_scalars(host, i, 0, &[("Break", if brk & 1 != 0 { 1.0 } else { 0.0 })]);
            self.dyn_scalars(host, i, 2, &[("Break2", if brk & 2 != 0 { 1.0 } else { 0.0 })]);
        }
        // UpdateMesh's effects
        let sp = &mut self.sps[i];
        if s.mesh_at != sp.seen_mesh {
            sp.seen_mesh = s.mesh_at;
            if s.mesh_at >= 0 && tick - s.mesh_at < FX_LATE {
                let r = Self::root_world(host, sp);
                let fx_at = Attachment {
                    loc: r.loc,
                    rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    scale: Some([1.0; 3]),
                };
                if let Some(v) = host.vfx() {
                    v.spawn_level(&self.art.stage1, &fx_at, true, &[], 0, false);
                    if s.mesh == 2 {
                        if let Some(id) = sp.stage_fx {
                            v.deactivate(id);
                        }
                        sp.stage_fx = v.spawn_level(&self.art.stage2, &r, true, &[], 0, false);
                        sp.follow.extend(sp.stage_fx);
                    }
                }
            }
        }
        // the sparks and the thrusters
        if s.sparks != sp.sparks.is_some() {
            if s.sparks {
                sp.spark_keys = s.spark_keys;
                sp.sparks =
                    host.vfx().and_then(|v| v.spawn_level(&sp.d.sparks.system, &sp.sparks_at, false, &[], 0, true));
            } else if let Some(id) = sp.sparks.take()
                && let Some(v) = host.vfx()
            {
                v.kill(id);
            }
        }
        self.set_thrusters(host, i, s.thrusters);
    }

    /// The AnimBP's pose of speaker i this tick (as the drive writes it), on the placed mesh and the shown
    /// damage mesh.
    fn drive(&mut self, host: &mut SceneryHost, i: usize, dt: f64) {
        let a = &self.art.anim_bp;
        let (u, u4, w0tick) = (self.u, self.u4, self.w0tick);
        let sp = &mut self.sps[i];
        sp.time += dt;
        let name = sp.d.skeletal.as_str();
        let [b, add, l] = &mut sp.poses;
        if sp.destroy_pos >= 0.0 {
            // the slot Override: the destroy montage (blend 0) is the whole pose
            let (anim, t) = segment_at(&a.destroy, sp.destroy_pos);
            host.sample(name, anim, t, b);
        } else {
            // the time since the anim instance started (the match's start, or the last UpdateMesh)
            let since = if sp.init_at < 0 { sp.time } else { f64::from(w0tick - sp.init_at + 1) * STEP };
            // DamageStates: the state's sequence, from its entry (the tick after the restart)
            let st = &self.states[sp.state];
            let st_time = if sp.state == 0 { since } else { since - STEP };
            let t = if a.states[sp.state].looping { st_time % st.length } else { st_time.min(st.length) };
            host.sample(name, &st.path, t, b);
            // the shake layer on Engine / Engine1
            host.sample(name, &self.shake.path, since % self.shake.length, l);
            for &k in &sp.layer {
                b.copy_bone(l, k);
            }
            // the ModifyBone nodes: scale replaced, then translation (beat, 0, 0) in the bone's space (glTF axes x, z,
            // y x 0.01)
            for &(k, large) in &sp.modify {
                let beat = if large { 45.0 * u } else { 25.0 * u4 };
                let scale = if large { 0.75 + 0.5 * u } else { 0.75 + 0.65 * u4 };
                for c in 0..3 {
                    b.s[k * 3 + c] = (f64::from(b.s[k * 3 + c]) * scale) as f32;
                }
                let vx = beat * 0.01 * f64::from(b.s[k * 3]);
                let [qx, qy, qz, qw] = b.quat(k);
                // q v q* with v = (vx, 0, 0)
                let (cy, cz) = (qz * vx, -qy * vx);
                let add_t = |t: &mut f32, v: f64| *t = (f64::from(*t) + v) as f32;
                add_t(&mut b.t[k * 3], vx + 2.0 * (qy * cz - qz * cy));
                add_t(&mut b.t[k * 3 + 1], 2.0 * (qw * cy - qx * cz));
                add_t(&mut b.t[k * 3 + 2], 2.0 * (qw * cz + qx * cy));
            }
            // the additive: Speaker_Idle_Additive (once) with the hit montage accumulated onto it, applied over the
            // base. (Every bone: a bone the clip does not track holds the identity delta, which leaves it as it is.)
            host.sample(name, &self.idle.path, since.min(self.idle.length), add);
            if let Some(h) = sp.hit
                && sp.hit_pos >= 0.0
            {
                let (anim, t) = segment_at(&a.hit[h], sp.hit_pos);
                host.sample(name, anim, t, &mut sp.ta);
                accumulate_held(&sp.ta, &sp.all, 1.0, true, add);
            }
            accumulate_held(add, &sp.all, 1.0, true, b);
        }
        host.pose(name).expect("the speaker's skeletal mesh").copy(b);
        // the shown damage mesh takes the same pose
        if sp.mesh > 0 {
            host.pose_skinned(sp.sets[sp.mesh as usize - 1]).copy(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mvs_material::jsonbin::JsonBin;
    use mvs_view::mesh::MeshInfo;

    use super::super::super::SceneryHeader;
    use super::super::super::blocks::SceneryBlock;
    use super::super::super::host::{NullSound, SceneryHost};
    use super::super::super::run::SceneryRun;
    use crate::stage::stage_key;
    use crate::stagedraw::StageDraw;

    /// The stage-m009 match drives the speakers (main .scratch/r6/index.md): speaker 0's stage 1 (mesh 1, the hit
    /// montage, its notify's effect, the sparks), stage 2 (mesh 2, PS_SpeakerDamage2), then its destruction
    /// (thrusters hidden, the destroy montage, the mesh hidden once it ends); speaker 1's stage 1.
    #[test]
    fn speakers_follow_the_world() {
        let Some(mut h) = mvs_sim_trace::harness::at("stage-m009", 0) else { return };
        let store = mvs_store::Store::open(mvs_store::dev_store_dir()).unwrap();
        let entry = StageDraw::load(&store, &stage_key(&h.w.data.stage_key)).unwrap().scenery.entry;
        let bytes = store.kind("scenery").unwrap().bytes(&entry).unwrap();
        let s = JsonBin::split(b"MVSC", &bytes, &entry).unwrap().parse::<SceneryHeader>(&entry).unwrap().scenery;
        let meshes = s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(&store, &k.mesh).unwrap())).collect();
        let mut run = SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
        let mut vfx = mvs_vfx::Player::new(&store, 1).unwrap();
        let mut sound = NullSound;
        let k = run.blocks.iter().position(|b| matches!(b, SceneryBlock::Cromulons(_))).expect("the cromulons block");
        run.state.begin();
        {
            let mut host = SceneryHost { state: &mut run.state, vfx: Some(&mut vfx), sound: &mut sound, world: None };
            for b in &mut run.blocks {
                b.begin(&mut host);
            }
        }
        let speakers = |run: &SceneryRun| {
            let SceneryBlock::Cromulons(c) = &run.blocks[k] else { unreachable!() };
            let s = c.speakers.as_ref().expect("speakers");
            s.sps
                .iter()
                .map(|sp| {
                    (sp.mesh, sp.brk, sp.hit_fx.is_some(), sp.stage_fx.is_some(), sp.sparks.is_some(), sp.thrusters)
                })
                .collect::<Vec<_>>()
        };
        let mut seen = Vec::new();
        let rest = run.state.skins
            [run.state.skeletal.iter().position(|x| x.name == "Schwifty_DestructiblePlatformBP").unwrap()]
        .pose
        .clone();
        let mut posed = false;
        let mut hit_fx = false;
        for frame in 1..=13900 {
            h.step_recorded();
            {
                let mut host =
                    SceneryHost { state: &mut run.state, vfx: Some(&mut vfx), sound: &mut sound, world: Some(&h.w) };
                for b in &mut run.blocks {
                    b.tick(&mut host, 1.0 / 60.0);
                }
            }
            run.state.tick_skeletal(1.0 / 60.0);
            vfx.tick(1.0 / 60.0);
            let now = speakers(&run);
            hit_fx |= now[0].2;
            if frame == 30 {
                let k0 = run.state.skeletal.iter().position(|x| x.name == "Schwifty_DestructiblePlatformBP").unwrap();
                posed = run.state.skins[k0].pose != rest;
            }
            if [1590, 2580, 4590, 13720, 13900].contains(&frame) {
                eprintln!("frame {frame}: {now:?}");
                seen.push(now);
            }
        }
        assert!(posed, "the AnimBP poses the speaker");
        assert!(hit_fx, "a hit montage's PS_SpeakerDamage_Hit");
        // (mesh, brk, hit fx, stage fx, sparks, thrusters)
        assert_eq!((seen[0][0].0, seen[0][0].1, seen[0][0].4), (1, 1, true), "speaker 0 at stage 1");
        assert_eq!(seen[1][1].0, 1, "speaker 1 at stage 1");
        // (the match's fighters break the speakers earlier than they did: speaker 0 is at stage 2 by the second frame)
        assert_eq!((seen[1][0].0, seen[1][0].1, seen[1][0].3), (2, 3, true), "speaker 0 at stage 2");
        assert_eq!(seen[2][1].0, 2, "speaker 1 at stage 2");
        assert!(!seen[3][0].5, "speaker 0's thrusters off");
        assert_eq!(seen[4][0].0, -1, "speaker 0 hidden once its destroy montage ends");
    }
}
