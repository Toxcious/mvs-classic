//! FParticleEmitterInstance: spawning, the modules' spawn and update, ribbons and
//! the bounds. The particles live in a slot arena per emitter (a killed particle's slot takes the next one spawned,
//! as the build reuses its particle memory); `particles` holds the active slots in the build's index order.

use std::sync::Arc;

use mvs_math::hypot::{hypot2, hypot3};
use mvs_math::js::{fround, max, min};
use mvs_math::libm::{asin, atan2, cos, pow, sin};

use crate::distribution::{Params, sample, sample1};
use crate::math::{UNIT_X, UNIT_Z, V3, ZERO3, rows_to_euler, safe_normal};
use crate::random::RandomSource;
use crate::system::{Box3, SysState};
use crate::template::{
    BeamEnd, BeamMod, Completion, DynMethod, EmitterTemplate, EventType, Flip, Kind, Module, PF_COLLISION_HAS_OCCURRED,
    PF_COLLISION_IGNORE_CHECK, PF_DELAY_COLLISIONS, PF_FREEZE, PF_FREEZE_ROTATION, PF_FREEZE_TRANSLATION,
    PF_IGNORE_COLLISIONS, SubUvMethod, TrailSource,
};

/// DELTA_SECONDS (the engine's measured frame time, a float) x 60: the step of the RelativeTime sum per 1 / 60 s.
fn rel_time_step() -> f64 {
    fround(0.0166668) * 60.0
}

/// FRibbonTypeDataPayload's chain types.
pub const R_FORCE: u8 = 0;
pub const R_DEAD: u8 = 1;
pub const R_MIDDLE: u8 = 2;
pub const R_START: u8 = 4;
pub const R_END: u8 = 8;

/// FRibbonTypeDataPayload: the chain type with the prev (newer) and next (older) links as slots, TrailIndex,
/// SpawnTime (RunningTime units), SpawnDelta, TiledU, SpawnedTessellationPoints, TriangleCount and
/// RenderingInterpCount (set by the draw), bInterpolatedSpawn and bMovementSpawned, Tangent and Up. A trail's START
/// is its newest particle; next walks to older ones down to the END.
#[derive(Clone, Debug)]
pub struct RibbonPayload {
    pub ty: u8,
    pub prev: Option<usize>,
    pub next: Option<usize>,
    pub trail: usize,
    pub spawn_time: f64,
    pub spawn_delta: f64,
    pub tiled_u: f64,
    pub points: f64,
    pub triangles: f64,
    pub interp_count: f64,
    pub interp: bool,
    pub moved: bool,
    pub tangent: V3,
    pub up: V3,
}

/// FBeam2TypeDataPayload as the beam Tick leaves it: SourcePoint / SourceTangent / SourceStrength, TargetPoint /
/// TargetTangent / TargetStrength, the noise frequency (Lock_Max_NumNoisePoints bits 12..23), Direction, StepSize,
/// Steps and TriangleCount; the noise points TargetNoisePoints (frequency + 1 vectors, floats), InterpolatedPoints
/// (InterpolationPoints > 0 without noise: Steps points along the Hermite from the source to the target, the last on
/// the target; TypeData Update 142bfdfd0) and TaperValues (TaperMethod not None: written once by TypeData Spawn
/// 142bfb770), floats.
#[derive(Clone, Debug)]
pub struct BeamPayload {
    pub src: V3,
    pub src_tan: V3,
    pub src_str: f64,
    pub tgt: V3,
    pub tgt_tan: V3,
    pub tgt_str: f64,
    pub freq: f64,
    pub noise: Vec<V3>,
    /// The noise distance scale (1 unless FrequencyDistance with bApplyNoiseScale sets it).
    pub noise_scale: f64,
    pub dir: V3,
    pub step: f64,
    pub steps: f64,
    pub tris: f64,
    pub pts: Vec<V3>,
    pub taper: Vec<f64>,
}

/// One particle (FBaseParticle with the payloads the ported modules keep).
#[derive(Clone, Debug)]
pub struct Particle {
    pub loc: V3,
    pub old: V3,
    pub vel: V3,
    pub base_vel: V3,
    pub size: V3,
    pub base_size: V3,
    pub color: [f64; 4],
    pub base_color: [f64; 4],
    pub rot: f64,
    pub rot_rate: f64,
    pub base_rot_rate: f64,
    pub rel_time: f64,
    pub one_over_life: f64,
    pub sub_image: f64,
    /// FFullSubUVPayload RandomImageTime.
    pub sub_time: f64,
    pub dyn_param: [f64; 4],
    /// CameraOffset payload Offset (reset to BaseOffset each tick) and BaseOffset.
    pub cam_offset: f64,
    pub cam_base: f64,
    /// The Flags counter PostSpawn stamps (ParticleCounter & 0x1ffffff).
    pub order: u32,
    /// FOrbitChainModuleInstancePayload: BaseOffset, Offset, Rotation (turns), BaseRotationRate, RotationRate,
    /// PreviousOffset.
    pub orb_base: V3,
    pub orbit: V3,
    pub orb_rot: V3,
    pub orb_base_rate: V3,
    pub orb_rate: V3,
    pub orb_prev: V3,
    /// FMeshRotationPayloadData (degrees; X roll, Y pitch, Z yaw): InitialOrientation, Rotation, RotationRate,
    /// RotationRateBase; and the alignment Euler the mesh PostSpawn / Tick add (EmitterInstance.alignMesh).
    pub mesh_init: V3,
    pub mesh_rot: V3,
    pub mesh_rate: V3,
    pub mesh_rate_base: V3,
    pub mesh_align: V3,
    /// Acceleration payload (UsedAcceleration).
    pub accel: V3,
    /// LocationDirect payload: LocationOffset at the spawn.
    pub direct: V3,
    pub rb: Option<RibbonPayload>,
    pub bm: Option<Box<BeamPayload>>,
    /// STATE_Particle_JustSpawned.
    pub just_spawned: bool,
    /// The Flags bits the port keeps (template.rs PF_*).
    pub flags: u32,
    /// The Collision payload: UsedCollisions, UsedDampingFactor, UsedDampingFactorRotation, Delay.
    pub coll: f64,
    pub coll_damp: V3,
    pub coll_damp_rot: V3,
    pub coll_delay: f64,
    /// Which particle this is in its emitter (a count of spawns): a ribbon's particle source knows a reused slot by it.
    pub(crate) uid: u64,
}

impl Particle {
    /// A new record as PreSpawn leaves it before the location and velocity.
    fn fresh() -> Particle {
        Particle {
            loc: ZERO3,
            old: ZERO3,
            vel: ZERO3,
            base_vel: ZERO3,
            size: ZERO3,
            base_size: ZERO3,
            color: [1.0; 4],
            base_color: [1.0; 4],
            rot: 0.0,
            rot_rate: 0.0,
            base_rot_rate: 0.0,
            rel_time: 0.0,
            one_over_life: 0.0,
            sub_image: 0.0,
            sub_time: 0.0,
            dyn_param: [1.0; 4],
            cam_offset: 0.0,
            cam_base: 0.0,
            order: 0,
            orb_base: ZERO3,
            orbit: ZERO3,
            orb_rot: ZERO3,
            orb_base_rate: ZERO3,
            orb_rate: ZERO3,
            orb_prev: ZERO3,
            mesh_init: ZERO3,
            mesh_rot: ZERO3,
            mesh_rate: ZERO3,
            mesh_rate_base: ZERO3,
            mesh_align: ZERO3,
            accel: ZERO3,
            direct: ZERO3,
            rb: None,
            bm: None,
            just_spawned: true,
            flags: 0,
            coll: 0.0,
            coll_damp: ZERO3,
            coll_damp_rot: ZERO3,
            coll_delay: 0.0,
            uid: 0,
        }
    }

    fn rb(&self) -> &RibbonPayload {
        self.rb.as_ref().expect("a ribbon particle")
    }

    fn rb_mut(&mut self) -> &mut RibbonPayload {
        self.rb.as_mut().expect("a ribbon particle")
    }
}

/// A ribbon trail's source state (the instance's per-trail arrays, zeroed by InitParameters): SourceTime, the
/// SpawnPerUnit remainder, SourceDistanceTraveled, the Current and Last source position, tangent and up, the
/// trail's OneOverMaxLifetime and Size, and SourceTimes / LastSourceTimes (0 without a particle source).
#[derive(Clone, Debug, Default)]
struct Trail {
    time: f64,
    rem: f64,
    dist: f64,
    cur: V3,
    cur_tan: V3,
    cur_up: V3,
    last: V3,
    last_tan: V3,
    last_up: V3,
    ooml: f64,
    size: f64,
    src_time: f64,
    last_src_time: f64,
    /// A particle source's held particle (SourceIndices[t], None for -1).
    held: Option<Held>,
}

/// A particle source's held particle: its slot and uid in the source emitter, and its Location and RelativeTime as
/// last read. GetParticleDirect reads the slot whether or not the particle lives, so a killed one stays where it died
/// (a new particle in its slot is not followed: the copy keeps the dead one's).
#[derive(Clone, Copy, Debug)]
struct Held {
    slot: usize,
    uid: u64,
    loc: V3,
    rel_time: f64,
}

/// The other emitter instances of the system, around the one ticking (a module reads them by EmitterName).
#[derive(Clone, Copy)]
pub struct Others<'a> {
    pub before: &'a [EmitterInstance],
    pub after: &'a [EmitterInstance],
}

impl Others<'_> {
    pub const NONE: Others<'static> = Others { before: &[], after: &[] };
}

/// The first emitter of the system named `name`: None, the emitter itself, or another.
enum Source<'a> {
    None,
    This,
    Other(&'a EmitterInstance),
}

/// What one source particle hands a LocationEmitter spawn (read before the new particle is written).
struct SourceParticle {
    loc: V3,
    vel: V3,
    rot: f64,
    mesh_rot: V3,
    rel_time: f64,
}

fn add3(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// One running emitter of a system instance.
#[derive(Debug)]
pub struct EmitterInstance {
    pub t: Arc<EmitterTemplate>,
    /// SecondsSinceCreation.
    pub secs: f64,
    /// EmitterTime; within the tick it has this tick's emitter delay taken off (what the modules sample).
    pub time: f64,
    loop_count: f64,
    duration: f64,
    cur_delay: f64,
    /// bEmitterIsDone (CheckEmitterFinished).
    pub done: bool,
    /// Deleted by the component (bKillOnCompleted / bKillOnDeactivate).
    pub dead: bool,
    pub fired: Vec<bool>,
    pub(crate) slots: Vec<Particle>,
    free: Vec<usize>,
    /// The active particles' slots, in index order.
    pub(crate) particles: Vec<usize>,
    leftover: f64,
    spawn_time: f64,
    last_selected: f64,
    travelled: f64,
    running: f64,
    last_tick: f64,
    trails: Vec<Trail>,
    counter: u32,
    spawn_tracking: f64,
    death_tracking: f64,
    /// The EventGenerator payload's collision count (pre-incremented once per HandleParticleCollision call).
    collision_tracking: f64,
    /// The ribbon's LastSelectedParticleIndex (+0x318: -1 in the ctor 142c66530, kept by InitParameters).
    trail_selected: f64,
    /// The next particle's uid.
    next_uid: u64,
    /// ParticleBoundingBox, None while not IsValid.
    pub bbox: Option<Box3>,
}

impl EmitterInstance {
    pub fn new(t: Arc<EmitterTemplate>, rng: &mut dyn RandomSource) -> EmitterInstance {
        let trails = t.ribbon.as_ref().map_or(0, |r| r.trails);
        let mut e = EmitterInstance {
            fired: vec![false; t.bursts.len()],
            t,
            secs: 0.0,
            time: 0.0,
            loop_count: 0.0,
            duration: 0.0,
            cur_delay: 0.0,
            done: false,
            dead: false,
            slots: Vec::new(),
            free: Vec::new(),
            particles: Vec::new(),
            leftover: 0.0,
            spawn_time: 0.0,
            last_selected: 0.0,
            travelled: 0.0,
            running: 0.0,
            last_tick: 0.0,
            trails: vec![Trail::default(); trails],
            counter: 0,
            spawn_tracking: 0.0,
            death_tracking: 0.0,
            collision_tracking: 0.0,
            trail_selected: -1.0,
            next_uid: 0,
            bbox: None,
        };
        e.setup_duration(rng);
        e
    }

    /// The live particles in index order.
    pub fn particles(&self) -> impl ExactSizeIterator<Item = &Particle> + '_ {
        self.particles.iter().map(|&s| &self.slots[s])
    }

    pub fn particle_count(&self) -> usize {
        self.particles.len()
    }

    /// The live particle at index `i`.
    pub fn particle(&self, i: usize) -> &Particle {
        &self.slots[self.particles[i]]
    }

    /// The particle in `slot` (a ribbon link).
    pub fn at_slot(&self, slot: usize) -> &Particle {
        &self.slots[slot]
    }

    /// The live particles' slots in index order.
    pub fn slots(&self) -> &[usize] {
        &self.particles
    }

    fn alloc(&mut self, p: Particle) -> usize {
        if let Some(s) = self.free.pop() {
            self.slots[s] = p;
            s
        } else {
            self.slots.push(p);
            self.slots.len() - 1
        }
    }

    /// Swap-removes the particle at index `i` (the last active one takes its place).
    fn remove_at(&mut self, i: usize) {
        let s = self.particles.swap_remove(i);
        self.free.push(s);
    }

    /// The component deletes the instance: its particles vanish.
    pub fn kill(&mut self) {
        self.dead = true;
        self.clear_particles();
    }

    /// Reactivation: InitParticles (142bf0750) gives a deleted instance a new one (LoopCount and the module payloads
    /// zero); then InitParameters (142c15640: SetupEmitterDuration again) and Init (142c14ad0: SpawnFraction,
    /// SecondsSinceCreation, EmitterTime and ParticleCounter zero, ResetBurstList, bEmitterIsDone cleared). A live
    /// instance keeps LoopCount, its payloads and its particles. The ribbon's InitParameters (142c75660) zeroes the
    /// per-trail arrays.
    pub fn reinit(&mut self, rng: &mut dyn RandomSource) {
        if self.dead {
            self.dead = false;
            self.loop_count = 0.0;
            self.running = 0.0;
            self.last_tick = 0.0;
            self.travelled = 0.0;
            self.last_selected = 0.0;
            self.trail_selected = -1.0;
            self.spawn_tracking = 0.0;
            self.death_tracking = 0.0;
            self.bbox = None;
        }
        self.setup_duration(rng);
        self.secs = 0.0;
        self.time = 0.0;
        self.leftover = 0.0;
        self.counter = 0;
        self.fired.fill(false);
        self.done = false;
        if let Some(r) = self.t.ribbon.as_ref() {
            self.trails = vec![Trail::default(); r.trails];
        }
    }

    /// Every particle gone at once.
    pub fn clear_particles(&mut self) {
        for &s in &self.particles {
            self.free.push(s);
            if let Some(r) = self.slots[s].rb.as_mut() {
                r.prev = None;
                r.next = None;
            }
        }
        self.particles.clear();
    }

    /// SetupEmitterDuration (142c1ebe0): CurrentDelay = EmitterDelay (with the range, a random point from
    /// EmitterDelayLow); EmitterDuration = the duration (with the range, a random point from EmitterDurationLow) plus
    /// CurrentDelay, less CurrentDelay again from the first loop on with bDelayFirstLoopOnly and EmitterLoops not 1.
    fn setup_duration(&mut self, rng: &mut dyn RandomSource) {
        let t = &self.t;
        self.cur_delay = if t.delay_range { (t.delay - t.delay_low) * rng.next_f64() + t.delay_low } else { t.delay };
        let base =
            if t.duration_range { (t.duration - t.duration_low) * rng.next_f64() + t.duration_low } else { t.duration };
        let mut d = base + self.cur_delay;
        if self.loop_count == 1.0 && t.delay_first_loop_only && t.loops != 1.0 {
            d -= self.cur_delay;
        }
        self.duration = d;
    }

    /// FParticleEmitterInstance::HasCompleted (142c14620): every loop played (EmitterLoops 0 never ends) and no
    /// particles.
    pub fn has_completed(&self) -> bool {
        let t = &self.t;
        t.loops != 0.0 && !(self.secs < t.loops * self.duration) && self.particles.is_empty()
    }

    /// Tick_EmitterTimeSetup (142c26e00), with or without bUseLegacyEmitterTime; returns the delay taken off
    /// EmitterTime until the tick ends.
    fn time_setup(&mut self, dt: f64, rng: &mut dyn RandomSource) -> f64 {
        self.secs += dt;
        let mut looped = false;
        if !self.t.legacy_time {
            self.time += dt;
            looped = self.duration > 0.0 && self.time >= self.duration;
        } else {
            self.time = self.secs;
            if self.duration > 1e-4 {
                self.time = self.secs % self.duration;
                looped = self.duration <= self.secs - self.loop_count * self.duration;
            }
        }
        let mut delay = self.cur_delay;
        if looped {
            self.loop_count += 1.0;
            self.fired.fill(false);
            if !self.t.legacy_time {
                self.time -= self.duration;
            }
            if self.t.recalc_each_loop || (self.t.delay_first_loop_only && self.loop_count == 1.0) {
                self.setup_duration(rng);
            }
        }
        if self.t.delay_first_loop_only && self.loop_count > 0.0 {
            delay = 0.0;
        }
        self.time -= delay;
        delay
    }

    /// Tick (mesh 142c266a0): time setup, KillParticles, ResetParticleParameters, module updates,
    /// Tick_SpawnParticles, UpdateOrbitData, UpdateBoundingBox, FinalUpdate, CheckEmitterFinished.
    pub fn tick(&mut self, dt: f64, sys: &mut SysState, others: Others, rng: &mut dyn RandomSource) {
        if self.dead {
            return;
        }
        let t = self.t.clone();
        if t.ribbon.is_some() {
            return self.tick_ribbon(dt, sys, others, rng);
        }
        if t.beam.is_some() {
            return self.tick_beam(dt, sys, others, rng);
        }
        if t.mesh_align != 0 {
            for &s in &self.particles {
                align_mesh(t.mesh_align, &mut self.slots[s]);
            }
        }
        let first = self.secs <= 0.0;
        let delay = self.time_setup(dt, rng);
        // KillParticles (142c181b0): from the last active index down, a particle past RelativeTime 1 swaps with the
        // last active one; each counted and reported to an EPET_Death entry.
        let mut i = self.particles.len();
        while i > 0 {
            i -= 1;
            let p = &self.slots[self.particles[i]];
            if p.rel_time <= 1.0 {
                continue;
            }
            if !t.death_events.is_empty() {
                self.death_tracking += 1.0;
                let n = self.death_tracking;
                for ev in &t.death_events {
                    if ev.freq == 0.0 || n % ev.freq == 0.0 {
                        sys.push_event(&ev.name, EventType::Death, p.loc, p.vel, self.time);
                    }
                }
            }
            self.remove_at(i);
        }
        self.reset_params(dt);
        self.update_all(dt, sys, rng);
        let active = sys.active
            && self.time >= 0.0
            && (t.loops == 0.0 || self.loop_count < t.loops || self.secs < t.loops * self.duration || first);
        if active {
            self.spawn_rate_and_bursts(dt, sys, others, rng);
        }
        if t.orbit_scale.is_some() && !self.particles.is_empty() {
            self.update_orbit_data(dt);
        }
        // UpdateBoundingBox (sprite 142c2ad30, mesh 142c2b4c0): OldLocation = Location, then the integration, except
        // for a particle spawned this tick without legacy spawning; the mesh rotation payload turns every particle.
        // Freeze stops both, FreezeTranslation the move, FreezeRotation the turns (the Collision completion flags).
        for &s in &self.particles {
            let p = &mut self.slots[s];
            p.old = p.loc;
            let f = p.flags;
            if !p.just_spawned || t.legacy_spawn {
                if f & (PF_FREEZE | PF_FREEZE_TRANSLATION) == 0 {
                    p.loc[0] += dt * p.vel[0];
                    p.loc[1] += dt * p.vel[1];
                    p.loc[2] += dt * p.vel[2];
                }
                if f & (PF_FREEZE | PF_FREEZE_ROTATION) == 0 {
                    p.rot += dt * p.rot_rate;
                }
            }
            p.just_spawned = false;
            if f & (PF_FREEZE | PF_FREEZE_ROTATION) == 0 {
                for c in 0..3 {
                    p.mesh_rot[c] += dt * p.mesh_rate[c];
                }
            }
        }
        if !sys.fixed_box && !self.particles.is_empty() {
            self.update_box(sys);
        }
        if !t.final_mods.is_empty() {
            self.final_update(dt, sys, rng);
        }
        // CheckEmitterFinished (142c0ac00).
        if self.particles.is_empty() {
            let past = t.bursts.last().is_none_or(|b| b.time < self.time);
            if past && (self.has_completed() || (t.max_rate == 0.0 && t.duration == 0.0 && t.loops == 0.0)) {
                self.done = true;
            }
        }
        self.time += delay;
    }

    /// Tick_ModuleFinalUpdate: SourceMovement (142c0f270): Location += SourceMovementScale(RelativeTime) x
    /// (ComponentToWorld.Translation - OldPosition), for a particle older than about two frames.
    fn final_update(&mut self, dt: f64, sys: &SysState, rng: &mut dyn RandomSource) {
        let d = [sys.loc[0] - sys.old_pos[0], sys.loc[1] - sys.old_pos[1], sys.loc[2] - sys.old_pos[2]];
        let dt32 = fround(dt * rel_time_step());
        let t = self.t.clone();
        for &mi in &t.final_mods {
            let Module::SourceMovement { scale } = &t.mods[mi] else { continue };
            for &s in &self.particles {
                let p = &mut self.slots[s];
                if !source_movement_aged(p.rel_time, p.one_over_life, dt32) {
                    continue;
                }
                let k = sample(scale, p.rel_time, &sys.params, rng);
                for c in 0..3 {
                    p.loc[c] += k[c] * d[c];
                }
            }
        }
    }

    /// ResetParticleParameters (142c1adf0): Size = |BaseSize|, velocity, rotation rate, colour, the mesh rotation
    /// rate and the CameraOffset payload back to their bases; RelativeTime += DeltaTime x OneOverMaxLifetime, a
    /// float32 sum of float32 products whose step is DELTA_SECONDS per 1 / 60 s of dt. Orbit payload:
    /// PreviousOffset = Offset, Offset = BaseOffset, RotationRate = BaseRotationRate.
    fn reset_params(&mut self, dt: f64) {
        let orbit = self.t.orbit_scale.is_some();
        let dt32 = fround(dt * rel_time_step());
        for &s in &self.particles {
            let p = &mut self.slots[s];
            p.vel = p.base_vel;
            p.size = p.base_size.map(f64::abs);
            p.color = p.base_color;
            p.rot_rate = p.base_rot_rate;
            p.mesh_rate = p.mesh_rate_base;
            p.cam_offset = p.cam_base;
            p.rel_time = fround(p.rel_time + fround(dt32 * p.one_over_life));
            if orbit {
                p.orb_prev = p.orbit;
                p.orbit = p.orb_base;
                p.orb_rate = p.orb_base_rate;
            }
        }
    }

    // ---------- beams ----------

    /// FParticleBeam2EmitterInstance::Tick (142bfc2e0): Tick_EmitterTimeSetup, KillParticles (past RelativeTime 1,
    /// swapped with the last), the spawn, ResetParticleParameters, the module updates, Tick_ModulePostUpdate
    /// (142bfd460: the Source, Target and Noise modules' Update, then the TypeData's), UpdateBoundingBox; EmitterTime
    /// gets the delay back. No CheckEmitterFinished: bEmitterIsDone stays false.
    /// The spawn (not while suppressed or EmitterTime < 0; EmitterLoops 0, LoopCount < EmitterLoops or
    /// SecondsSinceCreation < EmitterLoops x EmitterDuration): SpawnRate = Rate(EmitterTime); with fewer live beams
    /// than BeamCount, a rate <= 0 becomes 1 / DeltaTime (one beam: how the Lasso emitter, Rate 0 and no burst, gets
    /// its beam) and bAlwaysOn (DeltaTime > 1e-4) adds a burst of BeamCount at BeamCount / DeltaTime; Number =
    /// floor(SpawnRate x DeltaTime + SpawnFraction), Count = max(Burst, Number), 1 when no beam lives and Count is 0,
    /// capped at BeamCount - live. So a beam that dies is replaced in the same tick.
    fn tick_beam(&mut self, dt: f64, sys: &mut SysState, others: Others, rng: &mut dyn RandomSource) {
        let t = self.t.clone();
        let b = t.beam.as_ref().unwrap();
        let delay = self.time_setup(dt, rng);
        let mut i = self.particles.len();
        while i > 0 {
            i -= 1;
            if self.slots[self.particles[i]].rel_time > 1.0 {
                self.remove_at(i);
            }
        }
        if sys.active
            && self.time >= 0.0
            && (t.loops == 0.0 || self.loop_count < t.loops || self.secs < t.loops * self.duration)
        {
            let mut rate = sample1(&t.rate, self.time, &sys.params, rng);
            let inv = if dt > 0.0 { 1.0 / dt } else { 0.0 };
            let mut burst = 0.0;
            let live = self.particles.len();
            if live < b.count {
                if rate <= 0.0 {
                    rate = inv;
                }
                if b.always_on && dt > 1e-4 {
                    burst = b.count as f64;
                    rate += b.count as f64 * inv;
                }
            }
            if rate > 0.0 {
                let nl = rate * dt + self.leftover;
                let n = nl.floor();
                let inc = 1.0 / rate;
                let start = inc * self.leftover + dt - inc;
                self.leftover = nl - n;
                let mut count = max(burst, n);
                if live == 0 && count == 0.0 {
                    count = 1.0;
                }
                count = min(count, b.count.saturating_sub(live) as f64);
                let mut k = 0.0;
                while k < count {
                    self.spawn_beam(sys, others, rng, start);
                    k += 1.0;
                }
            }
        }
        self.reset_params(dt);
        self.update_all(dt, sys, rng);
        for i in 0..self.particles.len() {
            self.update_beam(i, sys, rng, false);
        }
        // UpdateBoundingBox (142bff620): float32 in the build. Per live beam the particle's
        // OldLocation = Location, then (unless just spawned without legacy spawning) Location += dt x Velocity and
        // Rotation += dt x RotationRate, Rotation = Fmod(Rotation, 2 pi). Without a fixed relative box the box starts
        // at the component location and takes the particle's Location, SourcePoint and TargetPoint, each also shifted
        // by NoiseRange's out range min and max on all three axes (raw: no NoiseRangeScale, no transform); then it
        // grows on every axis by MaxSize = the largest |Size x Scale| of any axis, at least 1.
        // Unverified: Component.bWarmingUp is not kept by the player (taken as false); PositionOffsetThisTick is 0
        // (no origin shift).
        let (nmin, nmax) = b.noise.as_ref().map_or((0.0, 0.0), |n| n.out_range);
        let mut bx = Box3 { min: sys.loc, max: sys.loc };
        let mut max_size: f64 = 1.0;
        for &s in &self.particles {
            let p = &mut self.slots[s];
            p.old = p.loc;
            if !p.just_spawned || t.legacy_spawn {
                for c in 0..3 {
                    p.loc[c] += dt * p.vel[c];
                }
                p.rot += dt * p.rot_rate;
            }
            p.just_spawned = false;
            p.rot %= std::f64::consts::TAU;
            let bm = p.bm.as_ref().unwrap();
            for pt in [p.loc, bm.src, bm.tgt] {
                for d in [0.0, nmin, nmax] {
                    for c in 0..3 {
                        bx.min[c] = min(bx.min[c], pt[c] + d);
                        bx.max[c] = max(bx.max[c], pt[c] + d);
                    }
                }
            }
            for c in 0..3 {
                max_size = max(max_size, (p.size[c] * sys.scale[c]).abs());
            }
        }
        if !sys.fixed_box {
            self.bbox = Some(if self.particles.is_empty() {
                Box3 { min: sys.loc.map(|v| v - 1.0), max: sys.loc.map(|v| v + 1.0) }
            } else {
                Box3 { min: bx.min.map(|v| v - max_size), max: bx.max.map(|v| v + max_size) }
            });
        }
        self.time += delay;
    }

    /// One beam: the module Spawns, then PostSpawn (142bf3c40: the Source, Target and Noise modules' Spawn, the
    /// TypeData's, the base PostSpawn). Noise Spawn (142bfb1f0, with bLowFreq_Enabled and a Frequency): the frequency
    /// is Frequency, or trunc((Frequency - Frequency_LowRange) x rand + Frequency_LowRange) with a low range; that many
    /// + 1 noise points.
    fn spawn_beam(&mut self, sys: &mut SysState, others: Others, rng: &mut dyn RandomSource, spawn_time: f64) {
        let t = self.t.clone();
        let b = t.beam.as_ref().unwrap();
        let mut p = self.make_particle(sys, others, rng, spawn_time, None, 0.0, None);
        if p.rel_time > 1.0 {
            return;
        }
        let freq = match &b.noise {
            Some(n) if n.low > 0.0 => ((n.freq - n.low) * rng.next_f64() + n.low).trunc(),
            Some(n) => n.freq,
            None => 0.0,
        };
        // TaperValues count (TypeData Spawn 142bfb770): with an enabled low-frequency BeamNoise (frequency + 1) x
        // NoiseTessellation (at least 1), else 2 without interpolation points and InterpolationPoints + 1 with them.
        let tapers = match (&b.taper, &b.noise) {
            (None, _) => 0.0,
            (Some(_), Some(n)) => (freq + 1.0) * n.tess,
            (Some(_), None) if b.interp == 0 => 2.0,
            (Some(_), None) => b.interp as f64 + 1.0,
        };
        p.bm = Some(Box::new(BeamPayload {
            src: ZERO3,
            src_tan: ZERO3,
            src_str: 0.0,
            tgt: ZERO3,
            tgt_tan: ZERO3,
            tgt_str: 0.0,
            freq,
            noise: if freq > 0.0 { vec![ZERO3; freq as usize + 1] } else { Vec::new() },
            noise_scale: 1.0,
            dir: UNIT_X,
            step: 0.0,
            steps: 0.0,
            tris: 0.0,
            pts: if b.interp > 0 { vec![ZERO3; b.interp] } else { Vec::new() },
            taper: vec![0.0; tapers as usize],
        }));
        let s = self.alloc(p);
        self.particles.push(s);
        self.update_beam(self.particles.len() - 1, sys, rng, true);
    }

    /// The beam modules on beam `i`, at its spawn and in every tick.
    /// Source (ResolveSourceData 142bf5450). UserSet: the instance's UserSetSourceArray, entry [beam index], [0] past
    /// its end; Emitter: the component's location; Actor: the root component's place of the actor in the component's
    /// instance parameter named SourceName (142bf5340; looked up again at every call); an empty array, no actor and
    /// Default: the Source distribution at EmitterTime, through ComponentToWorld (with its scale) unless
    /// bSourceAbsolute. Tangent (Direct): ComponentToWorld.TransformVector(1, 0, 0); strength: SourceStrength at
    /// EmitterTime. No Source module (TypeData Spawn 142bfb770): the component's location, that tangent, strength 1.
    /// Target (ResolveTargetData 142bf5cd0): the same with TargetName / Target / bTargetAbsolute; no Target module and
    /// BeamMethod Distance: the source + the component's X axis x Distance.
    /// Noise Update (142bfd820) with 0 <= NoiseLockTime <= 1e-4: every tick point i = NoiseRange at i / (frequency +
    /// 1), so the curve runs along the beam. Spawn (142bfb1f0) fills the points at the same t = i x (1 / (frequency +
    /// 1)); the same call is used here. Unverified: a longer NoiseLockTime runs a timer that
    /// rewrites the points without bSmooth in a way not read; here they keep the Spawn's values (the
    /// grapple's 0.5 s is its beam's Lifetime: the beam is replaced as the timer would end).
    /// TypeData Update (142bfdfd0), Speed 0: the particle sits on the target and the beam is locked; Direction =
    /// target - source, normalised; with noise StepSize = distance / (frequency + 1), Steps = frequency,
    /// TriangleCount = (Steps + 1) x NoiseTessellation x 2; without, one step and two triangles, or with
    /// InterpolationPoints > 0 StepSize = distance / InterpolationPoints, Steps = InterpolationPoints and 2 x Steps
    /// triangles. With InterpolationPoints > 0 (noise or not), the InterpolatedPoints k = 1 .. InterpolationPoints at
    /// t = k / InterpolationPoints on FMath::CubicInterp(source, source tangent x source strength, target, target
    /// tangent x target strength), a tangent with every component within 1e-4 taken as
    /// ComponentToWorld.TransformVector(1, 0, 0).
    /// FrequencyDistance > 0 (with noise): the frequency is capped at trunc(distance / FrequencyDistance) and, with
    /// bApplyNoiseScale, the noise distance scale is NoiseScale(capped / frequency) (from
    /// 142bfdfd0). Unverified: the capped count taken for Steps, StepSize and TriangleCount alike, the ratio's
    /// denominator taken as the beam's own frequency (both the same for the one beam with FrequencyDistance, the
    /// grapple's), and the interpolated points with noise computed as without (142bfdfd0's noise branch not read: the
    /// interpolated-noise fill 142c3eb10 reads them, with InterpolationSteps taken as InterpolationPoints).
    /// Tangents (ResolveSourceData / ResolveTargetData, +0x8c): Direct and Emitter
    /// ComponentToWorld.TransformVector(1, 0, 0); UserSet the instance's UserSetSourceTangentArray (no graph calls
    /// SetBeamSourceTangent or its kin: always empty here), empty: the Tangent distribution at the particle's
    /// RelativeTime through TransformVector unless bSourceAbsolute; Distribution that sample as it is. Strength: the
    /// Strength distribution at RelativeTime (the UserSet strength array is empty as well).
    /// BeamModifier (TypeData Spawn 142bfb770 and Update 142bfdfd0, after the ends are resolved): the source slot's
    /// payload adds its Position to SourcePoint (multiplies with bScale) and its Strength to SourceStrength; the
    /// target slot's the same to the target. World space, untransformed; Source Spawn has put the particle on the
    /// source point before. Unverified: the module's own Spawn / Update (its vtable +0x270 / +0x278), which fill the
    /// payload, are not read: the payload is taken as the Position / Strength distributions at EmitterTime (every
    /// module here has constants).
    /// Taper (TypeData Spawn 142bfb770, TaperMethod not None, Full or Partial alike): TaperValues[i] = TaperFactor(t)
    /// x TaperScale(t), t = i / (count - 1); written at the spawn only.
    fn update_beam(&mut self, i: usize, sys: &SysState, rng: &mut dyn RandomSource, spawning: bool) {
        let t = self.t.clone();
        let b = t.beam.as_ref().unwrap();
        let et = self.time;
        let pp = &sys.params;
        let slot = self.particles[i];
        let p = &mut self.slots[slot];
        let rel = p.rel_time;
        let bm = p.bm.as_deref_mut().unwrap();
        let x = sys.transform_vector(UNIT_X);
        match &b.source {
            None => {
                if spawning {
                    bm.src = sys.loc;
                    bm.src_tan = x;
                    bm.src_str = 1.0;
                }
            }
            Some(e) if spawning || !e.locked => {
                let user = usize::try_from(b.index).ok().and_then(|k| sys.beam_sources.get(k));
                bm.src = beam_resolve(e, user, i, et, sys, rng);
                bm.src_tan = beam_tangent(e, rel, x, sys, rng);
                bm.src_str = sample1(&e.strength, rel, pp, rng);
            }
            Some(_) => {}
        }
        match &b.target {
            None => {
                if spawning {
                    let d = if b.by_distance { sample1(&b.distance, et, pp, rng) } else { 0.0 };
                    let xn = safe_normal(x);
                    for c in 0..3 {
                        bm.tgt[c] = bm.src[c] + xn[c] * d;
                    }
                    bm.tgt_tan = x;
                    bm.tgt_str = 1.0;
                }
            }
            Some(e) if spawning || !e.locked => {
                let user = usize::try_from(b.index).ok().and_then(|k| sys.beam_targets.get(k));
                bm.tgt = beam_resolve(e, user, i, et, sys, rng);
                bm.tgt_tan = beam_tangent(e, rel, x, sys, rng);
                bm.tgt_str = sample1(&e.strength, rel, pp, rng);
            }
            Some(_) => {}
        }
        if let Some(n) = &b.noise
            && !bm.noise.is_empty()
            && (spawning || (n.lock_time >= 0.0 && n.lock_time <= 1e-4))
        {
            let step = 1.0 / (bm.freq + 1.0);
            for (k, v) in bm.noise.iter_mut().enumerate() {
                *v = sample(&n.range, k as f64 * step, pp, rng).map(fround);
            }
        }
        if spawning {
            // Source Spawn (142bfb4a0) puts the new particle on the source point; TypeData Spawn then applies the
            // modifiers and writes the taper values.
            p.loc = bm.src;
            bm.src_str = beam_modify(b.src_mod.as_ref(), &mut bm.src, bm.src_str, et, pp, rng);
            bm.tgt_str = beam_modify(b.tgt_mod.as_ref(), &mut bm.tgt, bm.tgt_str, et, pp, rng);
            if let Some((factor, scale)) = &b.taper {
                let n = bm.taper.len() as f64;
                for (k, v) in bm.taper.iter_mut().enumerate() {
                    let a = k as f64 * (1.0 / (n - 1.0));
                    *v = fround(sample1(factor, a, pp, rng) * sample1(scale, a, pp, rng));
                }
            }
            return;
        }
        bm.src_str = beam_modify(b.src_mod.as_ref(), &mut bm.src, bm.src_str, et, pp, rng);
        bm.tgt_str = beam_modify(b.tgt_mod.as_ref(), &mut bm.tgt, bm.tgt_str, et, pp, rng);
        p.loc = bm.tgt;
        let d = [bm.tgt[0] - bm.src[0], bm.tgt[1] - bm.src[1], bm.tgt[2] - bm.src[2]];
        let len = max(hypot3(d[0], d[1], d[2]), 1e-8);
        bm.dir = [d[0] / len, d[1] / len, d[2] / len];
        if let (Some(n), false) = (&b.noise, bm.noise.is_empty()) {
            let mut steps = bm.freq;
            if n.freq_dist > 0.0 {
                steps = min(steps, (len / n.freq_dist).trunc());
                if n.apply_scale {
                    bm.noise_scale = fround(sample1(&n.scale, steps / bm.freq, pp, rng));
                }
            }
            bm.step = len / (steps + 1.0);
            bm.steps = steps;
            bm.tris = (steps + 1.0) * n.tess * 2.0;
            if !bm.pts.is_empty() {
                interp_points(bm, b.interp, x);
            }
        } else if !bm.pts.is_empty() {
            let n = b.interp as f64;
            bm.step = len / n;
            bm.steps = n;
            bm.tris = n * 2.0;
            interp_points(bm, b.interp, x);
        } else {
            bm.step = len;
            bm.steps = 1.0;
            bm.tris = 2.0;
        }
    }

    // ---------- ribbons ----------

    /// FParticleRibbonEmitterInstance::Tick (142c80fb0): time setup, UpdateSourceData, KillParticles, the ribbon
    /// Spawn, ResetParticleParameters, the module updates, UpdateBoundingBox (142c82160: the integration, then
    /// OldLocation = Location), Tick_RecalculateTangents; RunningTime += DeltaTime, LastTickTime = TimeSeconds. No
    /// CheckEmitterFinished.
    fn tick_ribbon(&mut self, dt: f64, sys: &mut SysState, others: Others, rng: &mut dyn RandomSource) {
        let t = self.t.clone();
        let first = self.secs <= 0.0;
        let delay = self.time_setup(dt, rng);
        self.update_source(sys, first, others, rng);
        self.kill_ribbon(sys);
        let active = sys.active
            && self.time >= 0.0
            && (t.loops == 0.0 || self.loop_count < t.loops || self.secs < t.loops * self.duration || first);
        if active {
            self.spawn_ribbon(dt, sys, others, rng);
        }
        self.reset_params(dt);
        self.update_all(dt, sys, rng);
        for &s in &self.particles {
            let p = &mut self.slots[s];
            if !p.just_spawned || t.legacy_spawn {
                p.loc[0] += dt * p.vel[0];
                p.loc[1] += dt * p.vel[1];
                p.loc[2] += dt * p.vel[2];
                p.rot += dt * p.rot_rate;
            }
            p.old = p.loc;
            p.just_spawned = false;
        }
        if !sys.fixed_box {
            self.update_box(sys);
        }
        if t.ribbon.as_ref().is_some_and(|r| r.every_frame) {
            self.recalc_tangents();
        }
        self.time += delay;
        self.running += dt;
        self.last_tick = sys.world_time;
    }

    /// UpdateSourceData (142c82bd0), per trail. The source (GetSourcePosition 142c7b580) is the component location
    /// without a particle TrailSource (particle_source with one), world in both spaces; Up =
    /// ComponentToWorld.TransformVector(0, 0, 1) on every path. With a particle source and no particle held after the
    /// call: Last = Current = the source and SourceTime = 0, so nothing spawns (the tangents there are the PSC's
    /// PartSysVelocity, not kept: no spawn reads them, as a hold that starts zeroes LastTangent and CurrentTangent is
    /// made each tick). Else, on the first tick or when a hold has just started: Last = the source with a zero tangent
    /// and SourceTime = RunningTime; then Current = the source, CurrentTangent = (Current - Last) / (RunningTime -
    /// SourceTime), or (1, 0, 0) when no time passed (a source particle's own Location - OldLocation is not used).
    fn update_source(&mut self, sys: &SysState, first: bool, others: Others, rng: &mut dyn RandomSource) {
        let up = sys.transform_vector(UNIT_Z);
        let ts = self.t.trail_source.is_some();
        for ti in 0..self.trails.len() {
            let before = self.trails[ti].held.is_some();
            let pos = if ts { self.particle_source(sys, ti, others, rng) } else { sys.loc };
            let s = &mut self.trails[ti];
            if ts && s.held.is_none() {
                s.last = pos;
                s.cur = pos;
                s.last_tan = ZERO3;
                s.cur_tan = ZERO3;
                s.last_up = up;
                s.cur_up = up;
                s.time = 0.0;
                continue;
            }
            if first || (ts && !before) {
                s.last = pos;
                s.last_tan = ZERO3;
                s.last_up = up;
                s.time = self.running;
            }
            s.cur = pos;
            let e = self.running - s.time;
            if e == 0.0 {
                s.cur_tan = UNIT_X;
            } else {
                for c in 0..3 {
                    s.cur_tan[c] = (s.cur[c] - s.last[c]) * (1.0 / e);
                }
            }
            s.cur_up = up;
        }
    }

    /// GetSourcePosition (142c7b580) for TrailSource PET2SRCM_Particle. The source emitter is the first instance
    /// whose EmitterName is SourceName (ResolveSource 142c7b200; the ribbon itself is not left out). With none, or no
    /// live particle in it, the no-source answer (the component location) and the held particle and SourceTimes stay
    /// as they are. Else a held particle is kept while its RelativeTime is at most 1, and without one a new one is
    /// picked: random (trunc(fraction x ActiveParticles), the other trails not checked) or sequential (LastSelected +
    /// 1, back to 0 at ActiveParticles, then on past each particle another trail holds; -1 once past the last). None:
    /// the component location and SourceTimes 0. Held: its Location + the source emitter's SimulationToWorld origin
    /// (the component location for a local emitter, zero for a world one), SourceTimes = its RelativeTime. No
    /// SourceOffset on this path.
    fn particle_source(&mut self, sys: &SysState, ti: usize, others: Others, rng: &mut dyn RandomSource) -> V3 {
        let t = self.t.clone();
        let ts = t.trail_source.as_ref().expect("a particle trail source");
        let mut trails = std::mem::take(&mut self.trails);
        let mut sel = self.trail_selected;
        let pos = match self.source(others, &ts.name) {
            Source::None => sys.loc,
            Source::This => held_source(self, ts, &mut trails, &mut sel, ti, sys, rng),
            Source::Other(e) => held_source(e, ts, &mut trails, &mut sel, ti, sys, rng),
        };
        self.trails = trails;
        self.trail_selected = sel;
        pos
    }

    /// The ribbon Spawn (142c7e2b0): Spawn_Source first (false skips the rest), then rate particles on trail 0.
    fn spawn_ribbon(&mut self, dt: f64, sys: &mut SysState, others: Others, rng: &mut dyn RandomSource) {
        if !self.spawn_source(dt, sys, others, rng) {
            return;
        }
        let t = self.t.clone();
        let rb = t.ribbon.as_ref().unwrap();
        let rate = sample1(&t.rate, self.time, &sys.params, rng);
        let frac = self.leftover;
        let nl = rate * dt + frac;
        let number = nl.floor();
        self.leftover = nl - number;
        let over = self.particles.len() as f64 - rb.max + number;
        if rb.max > 0.0 && over > 0.0 {
            self.kill_trail(0, over, sys);
        }
        if !(rate > 0.0 && number > 0.0) {
            return;
        }
        let mut start = self.trail_start(0);
        let inc = 1.0 / rate;
        let up = if rb.source_up { sys.transform_vector(UNIT_Z) } else { UNIT_Z };
        let mut i = 0.0;
        while i < number {
            let spawn_time = inc * frac + dt - inc - i * inc;
            let alpha = min(max((i + 1.0) / number, 0.0), 1.0);
            let mut p = self.make_particle(sys, others, rng, spawn_time, None, 1.0 - (i + 1.0) / number, None);
            self.trail_life_size(0, &mut p, start.is_none(), spawn_time, sys, rng);
            let tan = [-dt * p.vel[0], -dt * p.vel[1], -dt * p.vel[2]];
            p.rb = Some(new_payload(0, self.running + alpha * dt, i * inc, 1.0, tan, up, false, false));
            let ps = self.add_to_trail(0, p, start);
            let (spawn_time_r, loc) = (self.slots[ps].rb().spawn_time, self.slots[ps].loc);
            if self.slots[ps].rb().tangent.iter().all(|x| x.abs() <= 1e-4) {
                let n = self.skip_interp(self.slots[ps].rb().next);
                if let Some(n) = n {
                    let k = 1.0 / (spawn_time_r - self.slots[n].rb().spawn_time);
                    let nl = self.slots[n].loc;
                    let r = self.slots[ps].rb_mut();
                    for c in 0..3 {
                        r.tangent[c] = (loc[c] - nl[c]) * k;
                    }
                }
            }
            if rb.prev_tangent {
                let pv = self.skip_interp(self.slots[ps].rb().next);
                if let Some(pv) = pv {
                    let pp = self.skip_interp(self.slots[pv].rb().next);
                    let b = pp.unwrap_or(pv);
                    let dt2 = spawn_time_r - self.slots[b].rb().spawn_time;
                    let bl = self.slots[b].loc;
                    let r = self.slots[pv].rb_mut();
                    for c in 0..3 {
                        r.tangent[c] = (loc[c] - bl[c]) / dt2;
                    }
                }
            }
            self.trails[0].time = spawn_time_r;
            start = Some(ps);
            i += 1.0;
        }
    }

    /// The first particle from `from` on (older ones) not spawned by interpolation.
    fn skip_interp(&self, mut from: Option<usize>) -> Option<usize> {
        while let Some(n) = from {
            if !self.slots[n].rb().interp {
                break;
            }
            from = self.slots[n].rb().next;
        }
        from
    }

    /// Spawn_Source (142c7f5f0), per trail: Count from GetSpawnPerUnitAmount with a SpawnPerUnit module (else 0; its
    /// answer, true without one, is what Spawn_Source returns), less the overflow past MaxParticleInTrailCount; Count
    /// particles along the cubic Hermite from Last to Current with the tangents x Elapsed.
    fn spawn_source(&mut self, dt: f64, sys: &mut SysState, others: Others, rng: &mut dyn RandomSource) -> bool {
        let t = self.t.clone();
        let rb = t.ribbon.as_ref().unwrap();
        let mut ret = true;
        for ti in 0..self.trails.len() {
            let mut count = 0.0;
            if t.spu.is_some() {
                let (c, r) = self.spawn_per_unit_amount(ti, dt, sys, rng);
                count = c;
                ret = r;
            }
            if rb.max > 0.0 {
                let over = self.particles.len() as f64 - rb.max + count;
                if over > 0.0 {
                    self.kill_trail(ti, over, sys);
                }
                if self.particles.len() as f64 + count > rb.max {
                    count = max(rb.max - self.particles.len() as f64, 0.0);
                }
            }
            let mut start = self.trail_start(ti);
            // Source loss (bDeadTrailsOnSourceLoss and LastSourceTimes > SourceTimes; both stay 0 without a particle
            // source): the START becomes a DEADTRAIL, Last = Current; nothing spawns.
            if rb.dead_on_loss && self.trails[ti].last_src_time > self.trails[ti].src_time {
                if let Some(st) = start {
                    self.slots[st].rb_mut().ty = R_DEAD;
                }
                let s = &mut self.trails[ti];
                s.last = s.cur;
                s.last_tan = s.cur_tan;
                s.last_up = s.cur_up;
                s.last_src_time = s.src_time;
                s.held = None;
                continue;
            }
            if count <= 0.0 {
                continue;
            }
            if self.secs < self.trails[ti].time {
                let s = &mut self.trails[ti];
                for c in 0..3 {
                    s.last_tan[c] = (s.cur[c] - s.last[c]) * (1.0 / self.running);
                }
            }
            if rb.prev_tangent
                && let Some(st) = start
            {
                self.prev_tangent_source(ti, st);
            }
            let s = self.trails[ti].clone();
            let e = self.running - s.time;
            let inc = 1.0 / count;
            let mut t0 = [0.0; 3];
            let mut t1 = [0.0; 3];
            for c in 0..3 {
                t0[c] = s.last_tan[c] * e;
                t1[c] = s.cur_tan[c] * e;
            }
            let (p0, p1) = (s.last, s.cur);
            let mut i = 0.0;
            while i < count {
                let a = min(max((i + 1.0) * inc, 0.0), 1.0);
                let a2 = a * a;
                let a3 = a2 * a;
                let h00 = 2.0 * a3 - 3.0 * a2 + 1.0;
                let h10 = a3 - 2.0 * a2 + a;
                let h01 = 3.0 * a2 - 2.0 * a3;
                let h11 = a3 - a2;
                let mut pos = [0.0; 3];
                let mut tan = [0.0; 3];
                for c in 0..3 {
                    pos[c] = p0[c] * h00 + t0[c] * h10 + p1[c] * h01 + t1[c] * h11;
                }
                for c in 0..3 {
                    tan[c] = ((6.0 * p0[c] + 3.0 * t0[c] + 3.0 * t1[c] - 6.0 * p1[c]) * a2
                        + (-6.0 * p0[c] - 4.0 * t0[c] - 2.0 * t1[c] + 6.0 * p1[c]) * a
                        + t0[c])
                        * inc;
                }
                let mut up = UNIT_Z;
                if rb.source_up {
                    for c in 0..3 {
                        up[c] = s.last_up[c] + (s.cur_up[c] - s.last_up[c]) * a;
                    }
                }
                let spawn_time = dt - i * inc * dt;
                let mut p = self.make_particle(sys, others, rng, spawn_time, Some(pos), 0.0, None);
                self.trail_life_size(ti, &mut p, start.is_none(), spawn_time, sys, rng);
                p.rb =
                    Some(new_payload(ti, self.running - (1.0 - a) * e, a * e, count, tan, up, i != count - 1.0, true));
                start = Some(self.add_to_trail(ti, p, start));
                i += 1.0;
            }
            let s = &mut self.trails[ti];
            s.last = s.cur;
            s.last_tan = s.cur_tan;
            s.last_up = s.cur_up;
            s.time = self.running;
            s.last_src_time = s.src_time;
            // A particle source past its life (SourceTimes > 1): the new START becomes a DEADTRAIL and the hold ends.
            if t.trail_source.is_some() && self.trails[ti].src_time > 1.0 {
                if let Some(st) = start {
                    self.slots[st].rb_mut().ty = R_DEAD;
                }
                self.trails[ti].held = None;
            }
        }
        ret
    }

    /// Spawn_Source's previous tangent pass: Prev = the START or the first older particle not spawned by
    /// interpolation, PrevPrev the next such one; the particles Prev's spawn interpolated are laid again along the
    /// Hermite from PrevPrev to Prev with NewTangent, which becomes LastSourceTangent.
    fn prev_tangent_source(&mut self, ti: usize, start: usize) {
        let Some(pv) = self.skip_interp(Some(start)) else { return };
        let Some(pp) = self.skip_interp(self.slots[pv].rb().next) else { return };
        let dt = self.running - self.slots[pp].rb().spawn_time;
        let cur = self.trails[ti].cur;
        let mut nt = [0.0; 3];
        if 1e-8 < dt {
            for c in 0..3 {
                nt[c] = (cur[c] - self.slots[pp].loc[c]) / dt;
            }
        }
        let n = self.slots[pv].rb().points;
        if n > 0.0 {
            let diff = self.slots[pv].rb().spawn_time - self.slots[pp].rb().spawn_time;
            let inc = 1.0 / n;
            let p0 = self.slots[pp].loc;
            let t0 = self.slots[pp].rb().tangent;
            let t1 = [nt[0] * diff, nt[1] * diff, nt[2] * diff];
            let mut o = self.slots[pp].rb().prev;
            let mut i = 1.0;
            while i <= n {
                let Some(os) = o else { break };
                let a = i * inc;
                let a2 = a * a;
                let a3 = a2 * a;
                let h00 = 2.0 * a3 - 3.0 * a2 + 1.0;
                let h10 = a3 - 2.0 * a2 + a;
                let h01 = 3.0 * a2 - 2.0 * a3;
                let h11 = a3 - a2;
                // (axis by axis: an axis reads only that axis of Prev, which o may be)
                for c in 0..3 {
                    let p1 = self.slots[pv].loc[c];
                    let pos = p0[c] * h00 + t0[c] * h10 + p1 * h01 + t1[c] * h11;
                    let tan = ((6.0 * p0[c] + 3.0 * t0[c] + 3.0 * t1[c] - 6.0 * p1) * a2
                        + (-6.0 * p0[c] - 4.0 * t0[c] - 2.0 * t1[c] + 6.0 * p1) * a
                        + t0[c])
                        * inc;
                    let q = &mut self.slots[os];
                    q.old[c] = q.loc[c];
                    q.loc[c] = pos;
                    q.rb_mut().tangent[c] = tan;
                }
                o = self.slots[os].rb().prev;
                i += 1.0;
            }
        }
        self.trails[ti].last_tan = nt;
    }

    /// GetSpawnPerUnitAmount (142c73e30): (Count, the answer).
    fn spawn_per_unit_amount(&mut self, ti: usize, dt: f64, sys: &SysState, rng: &mut dyn RandomSource) -> (f64, bool) {
        let t = self.t.clone();
        let m = t.spu.as_ref().unwrap();
        let v = sample1(&m.value, dt, &sys.params, rng) / m.unit;
        let mut count = 0.0;
        let mut moving = false;
        if v >= 0.0 {
            let s = &mut self.trails[ti];
            let mut dist = hypot3(s.cur[0] - s.last[0], s.cur[1] - s.last[1], s.cur[2] - s.last[2]);
            if (m.max_frame > 0.0 && dist > m.max_frame) || dist > 1048576.0 {
                dist = 0.0;
                self.travelled = 0.0;
                s.last = s.cur;
            }
            if dist > 0.0 {
                let total = dist + s.rem;
                moving = m.tol * m.unit < dist;
                let f = total * v;
                count = if t.ribbon.as_ref().unwrap().initial && self.particles.is_empty() && f < 1.0 {
                    1.0
                } else {
                    f.floor()
                };
                s.rem = max(total - count * m.unit, 0.0);
            }
        }
        (count, if m.ignore_rate { !moving } else { m.process_rate })
    }

    /// GetParticleLifetimeAndSize (142c734f0): a trail's first particle sets the trail's OneOverMaxLifetime and Size;
    /// every particle takes both, then Size.Y = Size.X, BaseSize = Size and RelativeTime = SpawnTime x
    /// OneOverMaxLifetime.
    fn trail_life_size(
        &mut self,
        ti: usize,
        p: &mut Particle,
        first: bool,
        spawn_time: f64,
        sys: &SysState,
        rng: &mut dyn RandomSource,
    ) {
        if first {
            let mut oo = 1.0;
            let life = match &self.t.lifetime {
                Some(l) => sample1(l, self.time, &sys.params, rng),
                None => 0.0,
            };
            if life > 0.0 && 1.0 / life != 0.0 {
                oo = if life < 0.001 { 999.99994 } else { fround(1.0 / fround(life)) };
            }
            self.trails[ti].ooml = oo;
            self.trails[ti].size = p.size[0];
        }
        let s = &self.trails[ti];
        p.one_over_life = s.ooml;
        p.size[0] = s.size;
        p.size[1] = s.size;
        p.base_size = p.size;
        p.rel_time = fround(fround(spawn_time) * p.one_over_life);
    }

    /// A new particle becomes its trail's START: the old START turns END when alone, else MIDDLE, with the new one
    /// as prev; TiledU from SourceDistanceTraveled with a TilingDistance. Returns the new particle's slot.
    fn add_to_trail(&mut self, ti: usize, p: Particle, start: Option<usize>) -> usize {
        let tiling = self.t.ribbon.as_ref().unwrap().tiling;
        let ps = self.alloc(p);
        match start {
            None => self.trails[ti].dist = 0.0,
            Some(st) => {
                let o = self.slots[st].rb_mut();
                o.ty = if o.next.is_some() { R_MIDDLE } else { R_END };
                o.prev = Some(ps);
                self.slots[ps].rb_mut().next = Some(st);
                if tiling.abs() > 1e-8 {
                    let (a, b) = (self.slots[ps].loc, self.slots[st].loc);
                    self.trails[ti].dist += hypot3(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
                    self.slots[ps].rb_mut().tiled_u = self.trails[ti].dist / tiling;
                }
            }
        }
        self.particles.push(ps);
        ps
    }

    /// The trail's START: the first in index order.
    fn trail_start(&self, ti: usize) -> Option<usize> {
        self.particles.iter().copied().find(|&s| {
            let r = self.slots[s].rb();
            r.ty == R_START && r.trail == ti
        })
    }

    /// Kills N of a trail's oldest (142c76a70): from its END towards its START, RelativeTime = 1.1, then
    /// KillParticles.
    fn kill_trail(&mut self, ti: usize, n: f64, sys: &SysState) {
        let Some(mut p) = self.particles.iter().copied().find(|&s| {
            let r = self.slots[s].rb();
            r.ty == R_END && r.trail == ti
        }) else {
            return;
        };
        let mut i = 0.0;
        while i < n {
            self.slots[p].rel_time = 1.1;
            i += 1.0;
            let Some(pr) = self.slots[p].rb().prev else { continue };
            p = pr;
            let r = self.slots[pr].rb_mut();
            if r.ty == R_START {
                break;
            }
            if r.ty == R_DEAD {
                r.triangles = 0.0;
                r.interp_count = 1.0;
            }
        }
        if n > 0.0 {
            self.kill_ribbon(sys);
        }
    }

    /// The ribbon KillParticles' new END of a chain: a DEADTRAIL stays one with TriangleCount 0 and
    /// RenderingInterpCount 1, a START stays, anything else becomes the END.
    fn end_at(&mut self, q: usize) {
        let r = self.slots[q].rb_mut();
        if r.ty == R_DEAD {
            r.triangles = 0.0;
            r.interp_count = 1.0;
        } else if r.ty != R_START {
            r.ty = R_END;
        }
        r.next = None;
    }

    /// The ribbon KillParticles (142c76bb0): a particle past RelativeTime 1, or one whose lifetime is shorter than
    /// the World TimeSeconds since the last Tick, leaves its chain; a dying MIDDLE force-kills every older one.
    fn kill_ribbon(&mut self, sys: &SysState) {
        let now = sys.world_time;
        let mut forced = false;
        let mut i = self.particles.len();
        while i > 0 {
            i -= 1;
            let s = self.particles[i];
            let p = &self.slots[s];
            if !(p.rel_time > 1.0 || (now != 0.0 && 1.0 / p.one_over_life < now - self.last_tick)) {
                continue;
            }
            let (ty, prev, next) = (p.rb().ty, p.rb().prev, p.rb().next);
            if ty == R_START || ty == R_DEAD {
                if let Some(n) = next {
                    self.slots[n].rb_mut().ty = ty;
                }
            } else if ty == R_END {
                if let Some(pr) = prev {
                    self.end_at(pr);
                }
            } else if ty == R_MIDDLE {
                if let Some(pr) = prev {
                    self.end_at(pr);
                }
                let mut q = next;
                while let Some(qs) = q {
                    self.slots[qs].rb_mut().ty = R_FORCE;
                    forced = true;
                    q = self.slots[qs].rb().next;
                }
            }
            if ty != R_FORCE {
                if let Some(n) = next {
                    self.slots[n].rb_mut().prev = None;
                }
                if let Some(pr) = prev {
                    self.slots[pr].rb_mut().next = None;
                }
            }
            let r = self.slots[s].rb_mut();
            r.prev = None;
            r.next = None;
            self.remove_at(i);
        }
        if !forced {
            return;
        }
        let mut i = self.particles.len();
        while i > 0 {
            i -= 1;
            let s = self.particles[i];
            let r = self.slots[s].rb_mut();
            if r.ty != R_FORCE {
                continue;
            }
            r.prev = None;
            r.next = None;
            self.remove_at(i);
        }
    }

    /// Tick_RecalculateTangents (142c81430): along each trail's live chain from its START, 142c81660 sets Tangent =
    /// (A - B) / |A.SpawnTime - B.SpawnTime| x D / SpawnedTessellationPoints (a zero gap counting as 0.0032).
    fn recalc_tangents(&mut self) {
        for ti in 0..self.trails.len() {
            let Some(st) = self.trail_start(ti) else { continue };
            let Some(mut cur) = self.slots[st].rb().next else { continue };
            let mut prev = st;
            let d = self.slots[st].rb().spawn_time - self.slots[cur].rb().spawn_time;
            self.recalc_tangent(st, cur, d, st);
            while let Some(next) = self.slots[cur].rb().next {
                let d = self.slots[cur].rb().spawn_time - self.slots[next].rb().spawn_time;
                self.recalc_tangent(prev, next, d, cur);
                prev = cur;
                cur = next;
            }
            let d = self.slots[prev].rb().spawn_time - self.slots[cur].rb().spawn_time;
            self.recalc_tangent(prev, cur, d, cur);
        }
    }

    fn recalc_tangent(&mut self, a: usize, b: usize, d: f64, out: usize) {
        let gap = self.slots[a].rb().spawn_time - self.slots[b].rb().spawn_time;
        let k = 1.0 / if gap != 0.0 { gap.abs() } else { 0.0032 };
        let (al, bl) = (self.slots[a].loc, self.slots[b].loc);
        let r = self.slots[out].rb_mut();
        let n = r.points;
        for c in 0..3 {
            r.tangent[c] = (k * (al[c] - bl[c]) * d) / n;
        }
    }

    /// OnDeactivateSystem: the ribbon's (142c782d0) turns every START into a DEADTRAIL with bDeadTrailsOnDeactivate.
    pub fn on_deactivate(&mut self) {
        if !self.t.ribbon.as_ref().is_some_and(|r| r.dead_on_deactivate) {
            return;
        }
        for &s in &self.particles {
            let r = self.slots[s].rb_mut();
            if r.ty == R_START {
                r.ty = R_DEAD;
            }
        }
    }

    /// UpdateBoundingBox with bUpdateBox: the box of this tick's particle locations in world space. Sprite
    /// (142c2ad30): each location padded by the larger of |Size x Scale3D| per axis (or the orbit payload's |Offset|)
    /// plus the larger of |Size.X x PivotX| and |Size.Y x PivotY|. Mesh (142c2b4c0): the mesh's extent x Size x
    /// Scale3D. Ribbon (142c82160): from the component location (world) or the origin (local), each location +- Size
    /// x Scale3D, then a local box through ComponentToWorld.
    fn update_box(&mut self, sys: &SysState) {
        let t = &self.t;
        let sc = sys.scale;
        if t.kind == Kind::Ribbon {
            let base = if t.local { ZERO3 } else { sys.loc };
            let mut b = Box3 { min: base, max: base };
            for &s in &self.particles {
                let p = &self.slots[s];
                for c in 0..3 {
                    let k = p.size[c] * sc[c];
                    b.min[c] = min(min(b.min[c], p.loc[c] - k), p.loc[c] + k);
                    b.max[c] = max(max(b.max[c], p.loc[c] - k), p.loc[c] + k);
                }
            }
            self.bbox = Some(if t.local { sys.transform_box(&b) } else { b });
            return;
        }
        let h = 1048576.0;
        let mut b = Box3 { min: [h; 3], max: [-h; 3] };
        let mesh = t.kind == Kind::Mesh;
        for &s in &self.particles {
            let p = &self.slots[s];
            let pos = sys.to_world(t.local, p.loc);
            let mut pad = [0.0; 3];
            if mesh {
                for c in 0..3 {
                    pad[c] = t.mesh_extent[c] * p.size[c] * sc[c];
                }
            } else {
                let mut r = if t.orbit_scale.is_some() {
                    max(max(p.orbit[0].abs(), p.orbit[1].abs()), p.orbit[2].abs())
                } else {
                    max(max((p.size[0] * sc[0]).abs(), (p.size[1] * sc[1]).abs()), (p.size[2] * sc[2]).abs())
                };
                r += max(max((p.size[0] * t.pivot[0]).abs(), (p.size[1] * t.pivot[1]).abs()), 0.0);
                pad = [r; 3];
            }
            for c in 0..3 {
                b.min[c] = min(b.min[c], pos[c] - pad[c]);
                b.max[c] = max(b.max[c], pos[c] + pad[c]);
            }
        }
        self.bbox = Some(b);
    }

    /// UpdateOrbitData (142c2c5d0) with one Orbit module: ChainMode Scale zeroes the payload; Add / Link: Rotation
    /// += RotationRate x dt; with any |Rotation| above 1e-4 the Offset turns by FRotationMatrix(Rotation x 360).
    fn update_orbit_data(&mut self, dt: f64) {
        let scale = self.t.orbit_scale == Some(true);
        for &s in &self.particles {
            let p = &mut self.slots[s];
            if scale {
                p.orb_rot = ZERO3;
                p.orbit = ZERO3;
                continue;
            }
            for c in 0..3 {
                p.orb_rot[c] += p.orb_rate[c] * dt;
            }
            let r = p.orb_rot;
            if r[0].abs() <= 1e-4 && r[1].abs() <= 1e-4 && r[2].abs() <= 1e-4 {
                continue;
            }
            let k = 2.0 * std::f64::consts::PI;
            let (cp, sp, cy, sy, cr, sr) =
                (cos(r[1] * k), sin(r[1] * k), cos(r[2] * k), sin(r[2] * k), cos(r[0] * k), sin(r[0] * k));
            let [x, y, z] = p.orbit;
            p.orbit[0] = x * (cp * cy) + y * (sr * sp * cy - cr * sy) + z * -(cr * sp * cy + sr * sy);
            p.orbit[1] = x * (cp * sy) + y * (sr * sp * sy + cr * cy) + z * (cy * sr - cr * sp * sy);
            p.orbit[2] = x * sp + y * (-sr * cp) + z * (cr * cp);
        }
    }

    // ---------- spawning ----------

    /// Spawn (142c222e0): the SpawnPerUnit rate plus Rate x RateScale at EmitterTime, clamped at 0; the bursts whose
    /// Time has come; the rate particles first, then the burst ones.
    fn spawn_rate_and_bursts(&mut self, mut dt: f64, sys: &mut SysState, others: Others, rng: &mut dyn RandomSource) {
        let t = self.t.clone();
        let mut rate = 0.0;
        let mut process = true;
        if let Some(spu) = &t.spu {
            let (r, p) = self.spawn_per_unit(dt, sys, rng);
            rate += max(r, 0.0);
            process = p;
            let _ = spu;
        }
        if process {
            let a = sample1(&t.rate, self.time, &sys.params, rng);
            let b = sample1(&t.rate_scale, self.time, &sys.params, rng);
            rate = max(a * b + rate, 0.0);
        }
        let mut burst = 0.0;
        if t.spu.as_ref().is_none_or(|s| s.process_bursts) {
            for (i, b) in t.bursts.iter().enumerate() {
                if self.fired[i] || b.time > self.time {
                    continue;
                }
                if dt < 1e-5 {
                    dt = 1e-5;
                }
                let mut n = b.count;
                if b.count_low > -1.0 {
                    let range = b.count - b.count_low + 1.0;
                    n = b.count_low + if range < 1.0 { 0.0 } else { (rng.next_f64() * range).trunc() };
                }
                burst += (sample1(&t.burst_scale, self.time, &sys.params, rng) * n).ceil();
                self.fired[i] = true;
            }
        }
        if !(rate > 0.0) && !(burst > 0.0) {
            return;
        }
        let nl = rate * dt + self.leftover;
        let n = nl.floor();
        let inc = if rate > 0.0 { 1.0 / rate } else { 0.0 };
        let start = inc * self.leftover + dt - inc;
        self.leftover = nl - n;
        let b_inc = if t.legacy_spawn && burst > 0.0 { 1.0 / burst } else { 0.0 };
        self.spawn_batch(sys, others, rng, n, start, inc);
        self.spawn_batch(sys, others, rng, burst, b_inc * dt, b_inc);
    }

    /// SpawnParticles (142c06f60): Interp runs down from 1 by 1 / Count; without legacy spawning SpawnTime runs down
    /// from StartTime by Increment after each particle kept, with it SpawnTime stays and Interp steps first.
    fn spawn_batch(
        &mut self,
        sys: &mut SysState,
        others: Others,
        rng: &mut dyn RandomSource,
        count: f64,
        start_time: f64,
        inc: f64,
    ) {
        let legacy = self.t.legacy_spawn;
        let interp_inc = if inc > 0.0 && count > 0.0 { 1.0 / count } else { 0.0 };
        let mut spawn_time = start_time;
        let mut interp = 1.0;
        let mut i = 0.0;
        while i < count {
            i += 1.0;
            if legacy {
                interp -= interp_inc;
            }
            if !self.spawn_one(sys, others, rng, spawn_time, None, interp, None, true) {
                continue;
            }
            if !legacy {
                spawn_time -= inc;
                interp -= interp_inc;
            }
        }
    }

    /// SpawnPerUnit GetSpawnAmount (142c4ff70): (Rate, the answer).
    fn spawn_per_unit(&mut self, dt: f64, sys: &SysState, rng: &mut dyn RandomSource) -> (f64, bool) {
        let t = self.t.clone();
        let m = t.spu.as_ref().unwrap();
        let ppu = sample1(&m.value, self.time, &sys.params, rng) / m.unit;
        let mut moved = false;
        let mut rate = 0.0;
        if ppu >= 0.0 {
            let (l, o, k) = (sys.loc, sys.old_loc, m.keep);
            let mut dist = hypot3((l[0] - o[0]) * k[0], (l[1] - o[1]) * k[1], (l[2] - o[2]) * k[2]);
            if m.max_frame > 0.0 && dist > m.max_frame {
                dist = 0.0;
                self.travelled = 0.0;
            }
            if dist > 0.0 {
                if dist > m.tol * m.unit {
                    moved = true;
                }
                let n = ((dist + self.travelled) * ppu).floor();
                rate = if dt > 0.0 { n / dt } else { 0.0 };
                self.travelled = max(0.0, dist + self.travelled - n * m.unit);
            }
        }
        (rate, if m.ignore_rate { !moved } else { m.process_rate })
    }

    /// One particle: PreSpawn, the module Spawns, PostSpawn; one already past RelativeTime 1 is dropped. With
    /// `payload` (not ForceSpawn) HandleParticleSpawned (142c51160) counts it and reports each EPET_Spawn entry.
    /// Returns whether the particle was kept.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_one(
        &mut self,
        sys: &mut SysState,
        others: Others,
        rng: &mut dyn RandomSource,
        spawn_time: f64,
        at: Option<V3>,
        interp: f64,
        v0: Option<V3>,
        payload: bool,
    ) -> bool {
        let p = self.make_particle(sys, others, rng, spawn_time, at, interp, v0);
        if p.rel_time > 1.0 {
            return false;
        }
        let (loc, vel) = (p.loc, p.vel);
        let s = self.alloc(p);
        self.particles.push(s);
        if payload && !self.t.spawn_events.is_empty() {
            self.spawn_tracking += 1.0;
            let n = self.spawn_tracking;
            for ev in &self.t.spawn_events {
                if ev.freq == 0.0 || n % ev.freq == 0.0 {
                    sys.push_event(&ev.name, EventType::Spawn, loc, vel, self.time);
                }
            }
        }
        true
    }

    /// The EmitterToSimulation origin: the emitter origin for a local emitter; the component location plus the
    /// rotated, unscaled origin for a world one.
    fn sim_origin(&self, sys: &SysState) -> V3 {
        let o = self.t.origin;
        if self.t.local {
            return o;
        }
        add3(sys.loc, sys.sim_vector(o))
    }

    /// EmitterToSimulation.TransformVector.
    fn to_sim(&self, sys: &SysState, v: V3) -> V3 {
        if self.t.local { v } else { sys.sim_vector(v) }
    }

    /// PreSpawn, the module Spawns and PostSpawn (with its ParticleCounter stamp) of one particle.
    #[allow(clippy::too_many_arguments)]
    fn make_particle(
        &mut self,
        sys: &SysState,
        others: Others,
        rng: &mut dyn RandomSource,
        spawn_time: f64,
        at: Option<V3>,
        interp: f64,
        v0: Option<V3>,
    ) -> Particle {
        let t = self.t.clone();
        let mut p = Particle::fresh();
        p.uid = self.next_uid;
        self.next_uid += 1;
        p.loc = at.unwrap_or_else(|| self.sim_origin(sys));
        self.spawn_time = spawn_time;
        p.old = p.loc;
        if let Some(v) = v0 {
            p.vel = v;
            p.base_vel = v;
        }
        for m in &t.mods {
            self.spawn_module(&t, m, &mut p, sys, others, rng);
        }
        if !t.local && interp != 0.0 {
            let (o, l) = (sys.old_loc, sys.loc);
            if pow(o[0] - l[0], 2.0) + pow(o[1] - l[1], 2.0) + pow(o[2] - l[2], 2.0) > 1.0 {
                for c in 0..3 {
                    p.loc[c] += interp * (o[c] - l[c]);
                }
            }
        }
        p.old = p.loc;
        for c in 0..3 {
            p.loc[c] += spawn_time * p.vel[c];
        }
        p.order = self.counter & 0x1ff_ffff;
        self.counter = self.counter.wrapping_add(1);
        if t.mesh_align != 0 {
            align_mesh(t.mesh_align, &mut p);
        }
        p
    }

    fn source<'a>(&self, others: Others<'a>, name: &str) -> Source<'a> {
        if let Some(e) = others.before.iter().find(|e| e.t.name == name) {
            return Source::Other(e);
        }
        if self.t.name == name {
            return Source::This;
        }
        match others.after.iter().find(|e| e.t.name == name) {
            Some(e) => Source::Other(e),
            None => Source::None,
        }
    }

    /// A module's Spawn.
    fn spawn_module(
        &mut self,
        t: &EmitterTemplate,
        m: &Module,
        p: &mut Particle,
        sys: &SysState,
        others: Others,
        rng: &mut dyn RandomSource,
    ) {
        let et = self.time;
        let pr = &sys.params;
        match m {
            Module::Lifetime { lifetime } => {
                // UParticleModuleLifetime::Spawn 142c23000, in float32.
                let life32 = fround(sample1(lifetime, et, pr, rng));
                p.one_over_life = if life32 > 0.0 { fround(1.0 / life32) } else { 0.0 };
                p.rel_time = fround(fround(self.spawn_time) * p.one_over_life);
            }
            Module::Size { start } => {
                // SpawnEx 142c5ce60: Size += StartSize; BaseSize += StartSize with the Required UVFlippingMode.
                let s = sample(start, et, pr, rng);
                let mut f = s;
                let fl = t.flip;
                if fl == Flip::FlipUv || (fl == Flip::RandomFlipUv && rng.next_f64() > 0.5) {
                    f = f.map(|x| -x);
                } else if fl == Flip::FlipU || (fl == Flip::RandomFlipU && rng.next_f64() > 0.5) {
                    f[0] = -f[0];
                } else if fl == Flip::FlipV || (fl == Flip::RandomFlipV && rng.next_f64() > 0.5) {
                    f[1] = -f[1];
                } else if fl == Flip::RandomFlipUvIndependent {
                    if rng.next_f64() > 0.5 {
                        f[0] = -f[0];
                    }
                    if rng.next_f64() > 0.5 {
                        f[1] = -f[1];
                    }
                }
                for c in 0..3 {
                    p.size[c] += s[c];
                    p.base_size[c] += f[c];
                }
            }
            Module::Color { color, alpha } => {
                // Spawn 142c59470: Color = BaseColor = (StartColor, StartAlpha) at EmitterTime, alpha unclamped.
                let c = sample(color, et, pr, rng);
                let a = sample1(alpha, et, pr, rng);
                p.color = [c[0], c[1], c[2], a];
                p.base_color = p.color;
            }
            Module::ColorOverLife { color, alpha } => {
                // Spawn 142c59520: Color = BaseColor = the curves at RelativeTime.
                let c = sample(color, p.rel_time, pr, rng);
                let a = sample1(alpha, p.rel_time, pr, rng);
                p.color = [c[0], c[1], c[2], a];
                p.base_color = p.color;
            }
            Module::Rotation { start } => {
                p.rot += sample1(start, et, pr, rng) * std::f64::consts::PI * 2.0;
            }
            Module::RotationRate { start } => {
                let r = sample1(start, et, pr, rng) * std::f64::consts::PI * 2.0;
                p.rot_rate += r;
                p.base_rot_rate += r;
            }
            Module::Velocity { start, radial, world } => {
                // SpawnEx 142c5d100: StartVelocity into the simulation space; radial from the EmitterToSimulation
                // origin.
                let v0 = sample(start, et, pr, rng);
                let v = if t.local {
                    if *world { sys.inverse_transform_vector(v0) } else { v0 }
                } else if *world {
                    v0
                } else {
                    sys.sim_vector(v0)
                };
                let rad = sample1(radial, et, pr, rng);
                let eo = self.sim_origin(sys);
                let from = [p.loc[0] - eo[0], p.loc[1] - eo[1], p.loc[2] - eo[2]];
                let len = hypot3(from[0], from[1], from[2]);
                for c in 0..3 {
                    let d = v[c] + if len > 1e-8 { (from[c] / len) * rad } else { 0.0 };
                    p.vel[c] += d;
                    p.base_vel[c] += d;
                }
            }
            Module::Location { start } => {
                // SpawnEx 142c5b6a0: Location += EmitterToSimulation.TransformVector(StartLocation).
                let v = self.to_sim(sys, sample(start, et, pr, rng));
                for c in 0..3 {
                    p.loc[c] += v[c];
                }
            }
            Module::LocationSphere { pos, neg, radius, surface, start, velocity, velocity_scale } => {
                // Spawn 142c5c040: DetermineUnitDirection (142c3df00) per enabled axis; SurfaceOnly normalises it;
                // offset = dir x StartRadius, each axis clamped to |normalised dir| x StartRadius.
                let mut u = [0.0; 3];
                for c in 0..3 {
                    let r = rng.next_f64();
                    u[c] = if pos[c] && neg[c] {
                        r * 2.0 - 1.0
                    } else if pos[c] {
                        r
                    } else if neg[c] {
                        -r
                    } else {
                        0.0
                    };
                }
                let rad = sample1(radius, et, pr, rng);
                let l2 = u[0] * u[0] + u[1] * u[1] + u[2] * u[2];
                let mut n = u;
                if l2 > 1e-8 {
                    for c in 0..3 {
                        n[c] = u[c] / l2.sqrt();
                    }
                }
                let d = if *surface { n } else { u };
                let mut off = [0.0; 3];
                for c in 0..3 {
                    if !pos[c] && !neg[c] {
                        continue;
                    }
                    let mx = n[c].abs() * rad;
                    off[c] = min(max(d[c] * rad, -mx), mx);
                }
                let st = sample(start, et, pr, rng);
                let loc = self.to_sim(sys, add3(off, st));
                for c in 0..3 {
                    p.loc[c] += loc[c];
                }
                if *velocity {
                    let s = sample1(velocity_scale, et, pr, rng);
                    let v = self.to_sim(sys, off);
                    for c in 0..3 {
                        p.vel[c] += v[c] * s;
                        p.base_vel[c] += v[c] * s;
                    }
                }
            }
            Module::LocationCylinder {
                r0,
                r1,
                h,
                pos,
                neg,
                start,
                radius,
                height,
                surface,
                velocity,
                radial_velocity,
                velocity_scale,
            } => {
                // Spawn 142b5b940: a unit direction redrawn until its radial part lies inside the radius (51 draws),
                // height in +-StartHeight / 2, SurfaceOnly normalising the radial part off the caps, clamped.
                let (r0, r1, h) = (*r0, *r1, *h);
                let st = sample(start, et, pr, rng);
                let rad = sample1(radius, et, pr, rng);
                let half = sample1(height, et, pr, rng) / 2.0;
                let mut u = cylinder_dir(pos, neg, rng);
                let mut tries = 0;
                while hypot2(u[r0] * rad, u[r1] * rad) > rad {
                    if tries >= 50 {
                        let l = hypot2(u[r0], u[r1]);
                        let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
                        u[r0] /= l;
                        u[r1] /= l;
                        break;
                    }
                    u = cylinder_dir(pos, neg, rng);
                    tries += 1;
                }
                let l2 = hypot2(u[r0], u[r1]);
                let n20 = if l2 > 1e-8 { u[r0] / l2 } else { 0.0 };
                let n21 = if l2 > 1e-8 { u[r1] / l2 } else { 0.0 };
                let mut off = [0.0; 3];
                off[h] = u[h] * half;
                let cap = *surface && (off[h].abs() - half).abs() < 1e-4;
                off[r0] = (if *surface && !cap { n20 } else { u[r0] }) * rad;
                off[r1] = (if *surface && !cap { n21 } else { u[r1] }) * rad;
                off[r0] = clamp_abs(off[r0], n20.abs() * rad);
                off[r1] = clamp_abs(off[r1], n21.abs() * rad);
                off[h] = clamp_abs(off[h], half);
                let loc = self.to_sim(sys, add3(off, st));
                for c in 0..3 {
                    p.loc[c] += loc[c];
                }
                if *velocity {
                    let mut v = off;
                    if *radial_velocity {
                        v[h] = 0.0;
                    }
                    let k = sample1(velocity_scale, et, pr, rng);
                    for x in &mut v {
                        *x *= k;
                    }
                    let w = self.to_sim(sys, v);
                    for c in 0..3 {
                        p.vel[c] += w[c];
                        p.base_vel[c] += w[c];
                    }
                }
            }
            Module::CameraOffset { offset, .. } => {
                // Spawn 142c59220: CameraOffset at RelativeTime, x the largest |Scale3D| axis for a world emitter.
                let k =
                    if t.local { 1.0 } else { max(max(sys.scale[0].abs(), sys.scale[1].abs()), sys.scale[2].abs()) };
                p.cam_offset = sample1(offset, p.rel_time, pr, rng) * k;
                p.cam_base = p.cam_offset;
            }
            Module::SubUV { .. } => {
                // Spawn 142c237e0: nothing with None; the random modes pick; the linear ones as the update.
                match t.interp {
                    SubUvMethod::None => {}
                    SubUvMethod::Random | SubUvMethod::RandomBlend => p.sub_image = random_image(t, p, rng),
                    _ => update(t, m, et, p, 0.0, sys, rng, true),
                }
            }
            Module::SubUvMovie { .. } => {
                // Spawn 142c23890 (vtable 1442c14e0): nothing with InterpolationMethod None; the random modes call the
                // module's DetermineImageIndex with SpawnTime as the DeltaTime.
                // Unverified: the linear modes (the 4.26 source from memory; the exe's linear branch not read): the
                // payload Time starts at StartingFrame (class default 1) clamped to [0, SubImages - 1] when above 1,
                // at trunc(rand x (SubImages - 1)) when 0, else at 0; then DetermineImageIndex with SpawnTime.
                if t.interp != SubUvMethod::None {
                if !matches!(t.interp, SubUvMethod::Random | SubUvMethod::RandomBlend)
                    && let Module::SubUvMovie { starting_frame: start, .. } = m
                {
                    let n = t.sub_h * t.sub_v;
                    if *start > 1.0 {
                        p.sub_time = min(max(*start, 0.0), n - 1.0);
                    } else if *start == 0.0 {
                        p.sub_time = (rng.next_f64() * (n - 1.0)).trunc();
                    }
                }
                p.sub_image = movie_image(t, m, et, p, self.spawn_time, sys, rng);
                }
            }
            Module::LocationDirect { location, offset, .. } => {
                // Spawn 142c59ad0 (vtable 1442c5fd0): Location = Location(RelativeTime), through ComponentToWorld
                // (TransformPosition, with scale) unless the emitter is local; the payload = LocationOffset(EmitterTime),
                // added to Location.
                let v = sample(location, p.rel_time, pr, rng);
                let l = if t.local { v } else { sys.to_world(true, v) };
                let o = sample(offset, et, pr, rng);
                p.direct = o;
                for c in 0..3 {
                    p.loc[c] = l[c] + o[c];
                }
            }
            Module::ParameterDynamic { .. } => {
                // PreSpawn zeroes the payload; EDPV_AutoSet leaves it.
                p.dyn_param = [0.0; 4];
                update(t, m, et, p, 0.0, sys, rng, true);
            }
            Module::ColorScaleOverLife { .. }
            | Module::SizeMultiplyLife { .. }
            | Module::RotationRateMultiplyLife { .. } => {
                update(t, m, et, p, 0.0, sys, rng, true);
            }
            Module::VelocityOverLifetime { absolute, vel } => {
                if *absolute {
                    let v = self.to_sim(sys, sample(vel, p.rel_time, pr, rng));
                    p.vel = v;
                    p.base_vel = v;
                }
            }
            Module::LocationEmitter {
                name,
                sequential,
                inherit_velocity,
                velocity_scale,
                inherit_rotation,
                rotation_scale,
            } => {
                // Spawn 142c59ca0: the first emitter instance named EmitterName; its particle picked at random or in
                // order; Location set to it across the spaces.
                let src = self.source(others, name);
                let (n, src_local, src_mesh) = match &src {
                    Source::None => return,
                    Source::This => (self.particles.len(), t.local, t.kind == Kind::Mesh),
                    Source::Other(e) => (e.particles.len(), e.t.local, e.t.kind == Kind::Mesh),
                };
                let mut i = 0.0;
                if *sequential {
                    self.last_selected += 1.0;
                    i = self.last_selected;
                    if i >= n as f64 {
                        i = 0.0;
                        self.last_selected = 0.0;
                    }
                } else if n > 0 {
                    i = (rng.next_f64() * n as f64).trunc();
                }
                let i = i as usize;
                if i >= n {
                    return;
                }
                let read = |q: &Particle| SourceParticle {
                    loc: q.loc,
                    vel: q.vel,
                    rot: q.rot,
                    mesh_rot: q.mesh_rot,
                    rel_time: q.rel_time,
                };
                let sp = match &src {
                    Source::This => read(self.particle(i)),
                    Source::Other(e) => read(e.particle(i)),
                    Source::None => unreachable!(),
                };
                if sp.rel_time == 0.0 && sp.loc[0] == 0.0 && sp.loc[1] == 0.0 && sp.loc[2] == 0.0 {
                    p.loc = if t.local { ZERO3 } else { sys.loc };
                } else if src_local == t.local {
                    p.loc = sp.loc;
                } else if src_local {
                    p.loc = sys.to_world(true, sp.loc);
                } else {
                    let (r, sc) = (&sys.rot, sys.scale);
                    let d = [sp.loc[0] - sys.loc[0], sp.loc[1] - sys.loc[1], sp.loc[2] - sys.loc[2]];
                    for k in 0..3 {
                        p.loc[k] = if sc[k].abs() <= 1e-8 {
                            0.0
                        } else {
                            (d[0] * r[3 * k] + d[1] * r[3 * k + 1] + d[2] * r[3 * k + 2]) / sc[k]
                        };
                    }
                }
                if *inherit_velocity {
                    let k = *velocity_scale;
                    for c in 0..3 {
                        p.base_vel[c] += k * sp.vel[c];
                        p.vel[c] += k * sp.vel[c];
                    }
                }
                if *inherit_rotation {
                    p.rot += sp.rot * *rotation_scale;
                    if t.kind == Kind::Mesh && src_mesh {
                        for c in 0..3 {
                            p.mesh_rot[c] += sp.mesh_rot[c];
                        }
                    }
                }
            }
            Module::Orbit { offset, rotation, rate } => {
                // Spawn 142c5af40, per option with bProcessDuringSpawn, at RelativeTime or EmitterTime.
                if offset.spawn {
                    let v = sample(&offset.amount, if offset.emitter_time { et } else { p.rel_time }, pr, rng);
                    for c in 0..3 {
                        p.orb_prev[c] = p.orbit[c];
                        p.orb_base[c] += v[c];
                        p.orbit[c] += v[c];
                    }
                }
                if rotation.spawn {
                    let v = sample(&rotation.amount, if rotation.emitter_time { et } else { p.rel_time }, pr, rng);
                    for c in 0..3 {
                        p.orb_rot[c] += v[c];
                    }
                }
                if rate.spawn {
                    let v = sample(&rate.amount, if rate.emitter_time { et } else { p.rel_time }, pr, rng);
                    for c in 0..3 {
                        p.orb_base_rate[c] += v[c];
                        p.orb_rate[c] += v[c];
                    }
                }
            }
            Module::TypeDataMesh { rpy } => {
                // FParticleMeshEmitterInstance::PostSpawn (142c19990): InitialOrientation = RollPitchYawRange at
                // SpawnTime.
                p.mesh_init = sample(rpy, self.spawn_time, pr, rng);
            }
            Module::MeshRotation { start, inherit_parent } => {
                // StartRotation in turns; bInheritParent adds the component rotation's Euler (in turns).
                let mut r = sample(start, et, pr, rng);
                if *inherit_parent {
                    let e = rows_to_euler(&sys.rot);
                    for c in 0..3 {
                        r[c] += e[c] / 360.0;
                    }
                }
                for c in 0..3 {
                    p.mesh_rot[c] += r[c] * 360.0;
                }
            }
            Module::MeshRotationRate { start } => {
                let r = sample(start, et, pr, rng);
                for c in 0..3 {
                    p.mesh_rate[c] += r[c] * 360.0;
                    p.mesh_rate_base[c] += r[c] * 360.0;
                }
            }
            Module::Acceleration { accel, owner_scale, world } => {
                // Spawn 142c228a0: UsedAcceleration = Acceleration(EmitterTime); Velocity and BaseVelocity += it x
                // SpawnTime (through the inverse transform for bAlwaysInWorldSpace in a local emitter).
                let mut a = sample(accel, et, pr, rng);
                if *owner_scale {
                    for c in 0..3 {
                        a[c] *= sys.scale[c];
                    }
                }
                p.accel = a;
                let v = if t.local && *world { sys.inverse_transform_vector(a) } else { a };
                for c in 0..3 {
                    p.vel[c] += v[c] * self.spawn_time;
                    p.base_vel[c] += v[c] * self.spawn_time;
                }
            }
            Module::VelocityCone { angle, velocity, direction, world } => {
                // SpawnEx 142c5d490: Angle and Velocity at EmitterTime, then one random fraction; the cone rotator
                // Pitch = trunc(Angle x 182.03784), Yaw = trunc(frac x 65533.625) degrees, D = its matrix's row 2
                // (-sinP cosY, -sinP sinY, cosP), set on the axes from Direction (default and zero: +Z). A world
                // emitter without bInWorldSpace through ComponentToWorld (scale and rotation, not
                // EmitterToSimulation), a local one with bInWorldSpace through its inverse; into Velocity and
                // BaseVelocity. bApplyOwnerScale (never set) would scale it.
                let a = sample1(angle, et, pr, rng);
                let speed = sample1(velocity, et, pr, rng);
                let pitch = ((a * 182.03784).trunc() * std::f64::consts::PI) / 180.0;
                let yaw = ((rng.next_f64() * 65533.625).trunc() * std::f64::consts::PI) / 180.0;
                let sp = sin(pitch);
                let (d0, d1, d2) = (-sp * cos(yaw), -sp * sin(yaw), cos(pitch));
                let f = *direction;
                let (mut r, mut u);
                if !(f[0] == 0.0 && f[1] == 0.0 && f[2].abs() == 1.0) {
                    r = cross3(UNIT_Z, f);
                    u = cross3(f, r);
                } else {
                    u = cross3(f, UNIT_X);
                    r = cross3(u, f);
                }
                unit3(&mut r);
                unit3(&mut u);
                let mut v0 = [0.0; 3];
                for c in 0..3 {
                    v0[c] = (d0 * r[c] + d1 * u[c] + d2 * f[c]) * speed;
                }
                let v = if t.local {
                    if *world { sys.inverse_transform_vector(v0) } else { v0 }
                } else if *world {
                    v0
                } else {
                    sys.transform_vector(v0)
                };
                for c in 0..3 {
                    p.vel[c] += v[c];
                    p.base_vel[c] += v[c];
                }
            }
            Module::Collision { damping, damping_rotation, max, delay, .. } => {
                // UParticleModuleCollision::Spawn 142c59340, every distribution at EmitterTime: UsedDampingFactor,
                // UsedDampingFactorRotation, UsedCollisions = RoundToInt(MaxCollisions) (floor(x + 0.5)), Delay =
                // DelayAmount; SpawnTime < Delay sets DelayCollisions (and clears CollisionHasOccurred).
                p.coll_damp = sample(damping, et, pr, rng);
                p.coll_damp_rot = sample(damping_rotation, et, pr, rng);
                p.coll = (sample1(max, et, pr, rng) + 0.5).floor();
                p.coll_delay = delay.as_ref().map_or(0.0, |d| sample1(d, et, pr, rng));
                if self.spawn_time < p.coll_delay {
                    p.flags = (p.flags | PF_DELAY_COLLISIONS) & !PF_COLLISION_HAS_OCCURRED;
                }
            }
            Module::AccelerationConstant { accel, world } => {
                // Unverified: UParticleModuleAccelerationConstant::Spawn (the 4.26 source from memory, not read in the
                // exe): Velocity and BaseVelocity += Acceleration (a plain vector, no distribution and no owner scale)
                // x SpawnTime, through ComponentToWorld.InverseTransformVector for bAlwaysInWorldSpace in a local
                // emitter, else as it is (EmitterToSimulation of a local emitter is the identity here: no emitter
                // origin or rotation is ported).
                let a = if t.local && *world { sys.inverse_transform_vector(*accel) } else { *accel };
                for c in 0..3 {
                    p.vel[c] += a[c] * self.spawn_time;
                    p.base_vel[c] += a[c] * self.spawn_time;
                }
            }
            Module::MeshRotationRateMultiplyLife { mult } => {
                // Unverified: UParticleModuleMeshRotationRateMultiplyLife::Spawn (the 4.26 source from memory, not
                // read in the exe): the mesh rotation payload's RotationRate x LifeMultiplier at RelativeTime, per
                // axis (RotationRateBase kept). The Update does the same every tick.
                let v = sample(mult, p.rel_time, pr, rng);
                for c in 0..3 {
                    p.mesh_rate[c] *= v[c];
                }
            }
            // (RotationOverLifetime's Spawn is the empty stub; SizeScale has none; SizeScaleBySpeed's Spawn +0x270 is
            // a stub, vtable 1442cacd8)
            Module::RotationOverLifetime { .. } | Module::SizeScale { .. } | Module::SizeScaleBySpeed { .. } => {}
            Module::AttractorPoint { .. }
            | Module::SourceMovement { .. }
            | Module::AccelerationDrag { .. }
            // (UParticleModuleAccelerationOverLifetime has no Spawn of its own)
            | Module::AccelerationOverLifetime { .. }
            // (UParticleModuleMeshRotationRateOverLife has no Spawn of its own: see its Update)
            | Module::MeshRotationRateOverLife { .. }
            | Module::None => {}
        }
    }

    /// Each particle's update modules in the template's order, particle by particle.
    fn update_all(&mut self, dt: f64, sys: &mut SysState, rng: &mut dyn RandomSource) {
        let t = self.t.clone();
        if t.upd_mods.is_empty() {
            return;
        }
        let time = self.time;
        for &s in &self.particles {
            let p = &mut self.slots[s];
            for &mi in &t.upd_mods {
                let m = &t.mods[mi];
                if let Module::Collision { done, reach, .. } = m {
                    collide(&t, *done, *reach, p, dt, sys, &mut self.collision_tracking, time);
                } else {
                    update(&t, m, time, p, dt, sys, rng, false);
                }
            }
        }
    }
}

/// A beam end's tangent: the X axis `x` (Direct, Emitter), else the Tangent at RelativeTime, through TransformVector
/// unless Distribution or absolute.
fn beam_tangent(e: &BeamEnd, rel: f64, x: V3, sys: &SysState, rng: &mut dyn RandomSource) -> V3 {
    if e.tan_method == 0 || e.tan_method == 3 {
        return x;
    }
    let v = sample(&e.tangent, rel, &sys.params, rng);
    if e.tan_method == 2 || e.absolute { v } else { sys.transform_vector(v) }
}

/// A BeamModifier slot on an end's point (in place) and strength (the answer).
fn beam_modify(
    m: Option<&BeamMod>,
    pt: &mut V3,
    strength: f64,
    et: f64,
    pp: &Params,
    rng: &mut dyn RandomSource,
) -> f64 {
    let Some(m) = m else { return strength };
    if m.flags & 1 != 0 {
        let v = sample(&m.pos, et, pp, rng);
        for c in 0..3 {
            pt[c] = if m.flags & 2 != 0 { pt[c] * v[c] } else { pt[c] + v[c] };
        }
    }
    if m.flags & 0x10 != 0 {
        let s = sample1(&m.str, et, pp, rng);
        return if m.flags & 0x20 != 0 { strength * s } else { strength + s };
    }
    strength
}

/// A beam end's point: the user array's entry `i` ([0] past its end), the component, the actor, else the point
/// distribution at EmitterTime (through ComponentToWorld unless absolute).
fn beam_resolve(
    e: &BeamEnd,
    user: Option<&Vec<V3>>,
    i: usize,
    et: f64,
    sys: &SysState,
    rng: &mut dyn RandomSource,
) -> V3 {
    let at = match e.method {
        1 => user.filter(|u| !u.is_empty()).map(|u| *u.get(i).unwrap_or(&u[0])),
        2 => Some(sys.loc),
        4 if e.name != "None" => sys.beam_actors.iter().find(|(n, _)| *n == e.name).and_then(|(_, at)| *at),
        _ => None,
    };
    if let Some(at) = at {
        return at;
    }
    let v = sample(&e.point, et, &sys.params, rng);
    if e.absolute { v } else { sys.to_world(true, v) }
}

/// The TypeData Update's InterpolatedPoints k = 1 .. `n` at t = k / n on FMath::CubicInterp(source, source tangent x
/// source strength, target, target tangent x target strength), a small tangent taken as `x`.
fn interp_points(bm: &mut BeamPayload, n: usize, x: V3) {
    let st = if small_tangent(bm.src_tan) { x } else { bm.src_tan };
    let tt = if small_tangent(bm.tgt_tan) { x } else { bm.tgt_tan };
    let inv = 1.0 / n as f64;
    for k in 1..=n {
        let a = k as f64 * inv;
        let a2 = a * a;
        let a3 = a2 * a;
        let (h00, h10, h11, h01) = (2.0 * a3 - 3.0 * a2 + 1.0, a3 - 2.0 * a2 + a, a3 - a2, 3.0 * a2 - 2.0 * a3);
        for c in 0..3 {
            bm.pts[k - 1][c] =
                fround(h00 * bm.src[c] + h10 * st[c] * bm.src_str + h11 * tt[c] * bm.tgt_str + h01 * bm.tgt[c]);
        }
    }
}

/// Every component of the tangent within 1e-4.
fn small_tangent(v: V3) -> bool {
    v[0].abs() <= 1e-4 && v[1].abs() <= 1e-4 && v[2].abs() <= 1e-4
}

/// A new ribbon payload (START, unlinked).
#[allow(clippy::too_many_arguments)]
fn new_payload(
    trail: usize,
    spawn_time: f64,
    spawn_delta: f64,
    points: f64,
    tangent: V3,
    up: V3,
    interp: bool,
    moved: bool,
) -> RibbonPayload {
    RibbonPayload {
        ty: R_START,
        prev: None,
        next: None,
        trail,
        spawn_time,
        spawn_delta,
        tiled_u: 0.0,
        points,
        triangles: 0.0,
        interp_count: 0.0,
        interp,
        moved,
        tangent,
        up,
    }
}

fn clamp_abs(v: f64, mx: f64) -> f64 {
    min(max(v, -mx), mx)
}

/// The cylinder's unit direction draw: one random per axis, X Y Z.
fn cylinder_dir(pos: &[bool; 3], neg: &[bool; 3], rng: &mut dyn RandomSource) -> V3 {
    let mut out = [0.0; 3];
    for c in 0..3 {
        let r = rng.next_f64();
        out[c] = if pos[c] && neg[c] {
            r * 2.0 - 1.0
        } else if pos[c] {
            r
        } else if neg[c] {
            -r
        } else {
            0.0
        };
    }
    out
}

/// A mesh emitter with PSA_Velocity (1) or PSA_AwayFromCenter (2) (mesh PostSpawn 142c19990, mesh Tick 142c266a0):
/// the alignment Euler of FQuat::FindBetweenNormals((1, 0, 0), Dir) (14135d040, Rotator 14136e550), Dir the
/// Velocity or the Location, normalised above a squared length of 1e-8.
fn align_mesh(mode: u8, p: &mut Particle) {
    let d = if mode == 2 { p.loc } else { p.vel };
    let (mut x, mut y, mut z) = (d[0], d[1], d[2]);
    let l2 = x * x + y * y + z * z;
    if l2 > 1e-8 {
        let k = 1.0 / l2.sqrt();
        x *= k;
        y *= k;
        z *= k;
    }
    let (mut qy, mut qz, mut qw) = (-z, y, 1.0 + x);
    if qw < 1e-6 {
        (qy, qz, qw) = (0.0, 1.0, 0.0);
    }
    let n = 1.0 / (qy * qy + qz * qz + qw * qw).sqrt();
    qy *= n;
    qz *= n;
    qw *= n;
    let r = 180.0 / std::f64::consts::PI;
    let st = -qw * qy;
    let yaw = atan2(2.0 * qw * qz, 1.0 - 2.0 * (qy * qy + qz * qz)) * r;
    let e = &mut p.mesh_align;
    if !(-0.4999995..=0.4999995).contains(&st) {
        e[0] = if st < 0.0 { -yaw } else { yaw };
        e[1] = if st < 0.0 { -90.0 } else { 90.0 };
    } else {
        e[0] = atan2(-2.0 * qy * qz, 1.0 - 2.0 * qy * qy) * r;
        e[1] = asin(2.0 * st) * r;
    }
    e[2] = yaw;
}

/// UParticleModuleSubUV::DetermineImageIndex (142c0d4f0), the random modes: the particle keeps its image unless
/// RandomImageTime is 0, its RelativeTime has moved past it since the last pick, or there was no pick; a pick is
/// trunc(rand x SubImages).
fn random_image(t: &EmitterTemplate, p: &mut Particle, rng: &mut dyn RandomSource) -> f64 {
    let rt = t.random_time.unwrap_or(0.0);
    if rt != 0.0 && !(p.rel_time - p.sub_time > rt) && p.sub_time != 0.0 {
        return p.sub_image;
    }
    p.sub_time = p.rel_time;
    let n = t.sub_h * t.sub_v;
    if n > 0.0 { (rng.next_f64() * n).trunc() } else { 0.0 }
}

/// A module's Update on one particle (and the spawn half of the ones whose Spawn runs it).
#[allow(clippy::too_many_arguments)]
fn update(
    t: &EmitterTemplate,
    m: &Module,
    time: f64,
    p: &mut Particle,
    dt: f64,
    sys: &SysState,
    rng: &mut dyn RandomSource,
    spawning: bool,
) {
    let pr = &sys.params;
    match m {
        Module::Orbit { offset, rotation, rate } => {
            // Update 142c61120, per option with bProcessDuringUpdate.
            if offset.update {
                let v = sample(&offset.amount, if offset.emitter_time { time } else { p.rel_time }, pr, rng);
                for c in 0..3 {
                    p.orb_prev[c] = p.orbit[c];
                    p.orbit[c] += v[c];
                }
            }
            if rotation.update {
                let v = sample(&rotation.amount, if rotation.emitter_time { time } else { p.rel_time }, pr, rng);
                for c in 0..3 {
                    p.orb_rot[c] += v[c];
                }
            }
            if rate.update {
                let v = sample(&rate.amount, if rate.emitter_time { time } else { p.rel_time }, pr, rng);
                for c in 0..3 {
                    p.orb_rate[c] += v[c];
                }
            }
        }
        Module::ColorOverLife { color, alpha } => {
            // Update 142c5fb10: Color (not BaseColor) = the curves at RelativeTime.
            let c = sample(color, p.rel_time, pr, rng);
            p.color[3] = sample1(alpha, p.rel_time, pr, rng);
            p.color[0] = c[0];
            p.color[1] = c[1];
            p.color[2] = c[2];
        }
        Module::ColorScaleOverLife { emitter_time, color, alpha } => {
            let tt = if *emitter_time { time } else { p.rel_time };
            let c = sample(color, tt, pr, rng);
            let a = sample1(alpha, tt, pr, rng);
            p.color[0] *= c[0];
            p.color[1] *= c[1];
            p.color[2] *= c[2];
            p.color[3] *= a;
        }
        Module::SizeMultiplyLife { mult, axes } => {
            let s = sample(mult, p.rel_time, pr, rng);
            for c in 0..3 {
                if axes[c] {
                    p.size[c] *= s[c];
                }
            }
        }
        Module::SubUV { index } => {
            // Update 142c2aac0: skips None, the random modes while RandomImageChanges is 0 and a particle past
            // RelativeTime 1. Linear: trunc(SubImageIndex(RelativeTime)); the others unrounded.
            if t.interp == SubUvMethod::None || p.rel_time > 1.0 {
                return;
            }
            if matches!(t.interp, SubUvMethod::Random | SubUvMethod::RandomBlend) {
                if t.random_changes != 0.0 && t.random_time.is_some() {
                    p.sub_image = random_image(t, p, rng);
                }
                return;
            }
            let i = sample1(index, p.rel_time, pr, rng);
            p.sub_image = if t.interp == SubUvMethod::Linear { i.trunc() } else { i };
        }
        Module::SubUvMovie { .. } => {
            // SubUV's Update (142c2aac0, inherited): the same skips, then the module's own DetermineImageIndex.
            if t.interp == SubUvMethod::None || p.rel_time > 1.0 {
                return;
            }
            if matches!(t.interp, SubUvMethod::Random | SubUvMethod::RandomBlend) && t.random_changes == 0.0 {
                return;
            }
            p.sub_image = movie_image(t, m, time, p, dt, sys, rng);
        }
        Module::LocationDirect { location, scale, .. } => {
            // Update 142c60580: the target = Location(RelativeTime) (through ComponentToWorld unless local) + the
            // payload; Diff = target - Location; with S = ScaleFactor(RelativeTime).X: Velocity = Diff x S / DeltaTime
            // (0 when DeltaTime is not above 0) and Location += Diff x (1 - S). The tick's integration then moves the
            // particle by Velocity x DeltaTime, onto the target.
            let v = sample(location, p.rel_time, pr, rng);
            let n = if t.local { v } else { sys.to_world(true, v) };
            let sc = sample1(scale, p.rel_time, pr, rng);
            let inv = if dt > 0.0 { 1.0 / dt } else { 0.0 };
            for c in 0..3 {
                let d = n[c] + p.direct[c] - p.loc[c];
                p.vel[c] = d * sc * inv;
                p.loc[c] += d * (1.0 - sc);
            }
        }
        // Unverified: UParticleModuleMeshRotationRateOverLife::Update (4.26 from memory, not read in the exe): v =
        // RotRate at RelativeTime; bScaleRotRate multiplies the mesh rotation payload's RotationRate by v, else adds
        // v x 360 (turns to degrees, as MeshRotationRate's Spawn). The rate is taken as reset to RotationRateBase each
        // tick, as the sprite RotationRate is.
        Module::MeshRotationRateOverLife { rate, scale } => {
            let v = sample(rate, p.rel_time, pr, rng);
            for c in 0..3 {
                if *scale {
                    p.mesh_rate[c] *= v[c];
                } else {
                    p.mesh_rate[c] += v[c] * 360.0;
                }
            }
        }
        Module::ParameterDynamic { params } => {
            // Spawn / update (142c5cb30, 142c61360).
            for (i, d) in params.iter().enumerate() {
                if !spawning && d.spawn_only {
                    continue;
                }
                if d.method == DynMethod::AutoSet {
                    continue;
                }
                let mut v = if d.sampled {
                    sample1(&d.value, if d.emitter_time { time } else { p.rel_time }, pr, rng)
                } else {
                    1.0
                };
                match d.method {
                    DynMethod::VelocityX => v *= p.vel[0],
                    DynMethod::VelocityY => v *= p.vel[1],
                    DynMethod::VelocityZ => v *= p.vel[2],
                    DynMethod::VelocityMag => v *= hypot3(p.vel[0], p.vel[1], p.vel[2]),
                    _ => {}
                }
                p.dyn_param[i] = v;
            }
        }
        Module::CameraOffset { offset, spawn_only } => {
            // Update 142c5e300 (DirectSet): Offset = CameraOffset(RelativeTime), unscaled, unless bSpawnTimeOnly.
            if !*spawn_only {
                p.cam_offset = sample1(offset, p.rel_time, pr, rng);
            }
        }
        Module::Acceleration { world, .. } => {
            // Update 142c27850: Velocity and BaseVelocity += the payload x DeltaTime.
            let a = if t.local && *world { sys.inverse_transform_vector(p.accel) } else { p.accel };
            for c in 0..3 {
                p.vel[c] += a[c] * dt;
                p.base_vel[c] += a[c] * dt;
            }
        }
        Module::AccelerationOverLifetime { accel, world } => {
            // UParticleModuleAccelerationOverLifetime::Update: a = AccelOverLife(RelativeTime), through this tick's
            // ComponentToWorld.InverseTransformVector for bAlwaysInWorldSpace in a local emitter; Velocity and
            // BaseVelocity += a x DeltaTime.
            // Unverified: 4.26's Update, not read in the exe.
            let s = sample(accel, p.rel_time, pr, rng);
            let a = if t.local && *world { sys.inverse_transform_vector(s) } else { s };
            for c in 0..3 {
                p.vel[c] += a[c] * dt;
                p.base_vel[c] += a[c] * dt;
            }
        }
        Module::AccelerationDrag { drag } => {
            let k = sample1(drag, p.rel_time, pr, rng);
            for c in 0..3 {
                let d = -p.vel[c] * k * dt;
                p.vel[c] += d;
                p.base_vel[c] += d;
            }
        }
        Module::RotationRateMultiplyLife { mult } => {
            p.rot_rate *= sample1(mult, p.rel_time, pr, rng);
        }
        Module::SizeScale { scale } => {
            // Unverified: UParticleModuleSizeScale::Update from the stock 4.26 source, not read in the exe: Size =
            // BaseSize x SizeScale(RelativeTime), replacing what the modules before it set.
            let s = sample(scale, p.rel_time, pr, rng);
            for c in 0..3 {
                p.size[c] = p.base_size[c] * s[c];
            }
        }
        Module::SizeScaleBySpeed { speed, max: mx } => {
            // Update 142c62b20 (ctor 142c34a30, vtable 1442cacd8): speed = |Velocity| (not BaseVelocity), s =
            // min(max(SpeedScale x speed, 1.0), MaxScale) per axis (the 1.0 lower bound at 143a4f22c, the max first),
            // Size = (|BaseSize.X| x sx, |BaseSize.Y| x sy, |BaseSize.Z|): it overwrites what an earlier module did to
            // Size. The ctor writes MaxScale (1, 1) and leaves SpeedScale at the zeroed CDO's (0, 0).
            let v = p.vel;
            let sp = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let sx = min(max(speed[0] * sp, 1.0), mx[0]);
            let sy = min(max(speed[1] * sp, 1.0), mx[1]);
            let b = p.base_size;
            p.size = [b[0].abs() * sx, b[1].abs() * sy, b[2].abs()];
        }
        Module::RotationOverLifetime { over, scale } => {
            // UParticleModuleRotationOverLifetime::Update (142c2a8f0; Spawn is the empty stub): every tick v =
            // RotationOverLife at RelativeTime; with Scale (+0x60 bit 0, true from the ctor 142c061b0) the particle's
            // current Rotation x (v x 6.2831855, the float at 143ba2b30), else Rotation + v x 6.2831855. Per tick,
            // not per second: the result follows the frame rate, and a scaled Rotation of 0 stays 0. Kept in float32
            // as the build's field is.
            let v = fround(fround(sample1(over, p.rel_time, pr, rng)) * TWO_PI_F);
            p.rot = if *scale { fround(fround(p.rot) * v) } else { fround(fround(p.rot) + v) };
        }
        Module::MeshRotationRateMultiplyLife { mult } => {
            // (the Spawn's rule, on the rate ResetParticleParameters put back to RotationRateBase)
            let v = sample(mult, p.rel_time, pr, rng);
            for c in 0..3 {
                p.mesh_rate[c] *= v[c];
            }
        }
        Module::AccelerationConstant { accel, world } => {
            // Unverified: UParticleModuleAccelerationConstant::Update (the 4.26 source from memory): Velocity and
            // BaseVelocity += the Spawn's vector x DeltaTime, through this tick's transform.
            let a = if t.local && *world { sys.inverse_transform_vector(*accel) } else { *accel };
            for c in 0..3 {
                p.vel[c] += a[c] * dt;
                p.base_vel[c] += a[c] * dt;
            }
        }
        Module::AttractorPoint { position, world_position, range, by_distance, strength, neg, pos, base } => {
            // UParticleModuleAttractorPoint::Update (142c29560).
            let a = sample(position, time, pr, rng);
            let xf = !t.local && !*world_position;
            let mut at = a;
            if xf {
                at = add3(sys.transform_vector(a), sys.loc);
            }
            let sc = if xf { hypot3(sys.scale[0], sys.scale[1], sys.scale[2]) } else { 3f64.sqrt() };
            let rg = sample1(range, time, pr, rng) * sc;
            let mut d = [at[0] - p.loc[0], at[1] - p.loc[1], at[2] - p.loc[2]];
            let sq = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            let dist = sq.sqrt();
            if dist > rg {
                return;
            }
            let mut str = if *by_distance {
                if rg != 0.0 { sample1(strength, (rg - dist) / rg, pr, rng) } else { 0.0 }
            } else {
                sample1(strength, time, pr, rng)
            };
            if xf {
                str *= hypot3(sys.scale[0], sys.scale[1], sys.scale[2]);
            }
            if sq > 1e-8 {
                for x in &mut d {
                    *x /= dist;
                }
            }
            if str < 0.0 {
                str = -str;
                for x in &mut d {
                    *x = -*x;
                }
            }
            for c in 0..3 {
                let lo = if neg[c] { -1.0 } else { 0.0 };
                let hi = if pos[c] { 1.0 } else { 0.0 };
                let v = min(max(d[c], lo), hi) * str * dt;
                p.vel[c] += v;
                if *base {
                    p.base_vel[c] += v;
                }
            }
        }
        Module::VelocityOverLifetime { absolute, vel } => {
            if *absolute {
                let s = sample(vel, p.rel_time, pr, rng);
                p.vel = if t.local { s } else { sys.sim_vector(s) };
            } else {
                let v = sample(vel, p.rel_time, pr, rng);
                for c in 0..3 {
                    p.vel[c] *= v[c];
                }
            }
        }
        _ => {}
    }
}

/// RotationOverLifetime's turns to radians (the float at 143ba2b30).
const TWO_PI_F: f64 = std::f32::consts::TAU as f64;

fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Normalised in place when its length is above 1e-8.
fn unit3(a: &mut V3) {
    let l = hypot3(a[0], a[1], a[2]);
    if l > 1e-8 {
        for x in a.iter_mut() {
            *x /= l;
        }
    }
}

/// particle_source's work on the source emitter `src` (the ribbon itself or another), the trails taken out of the
/// ribbon. A held particle is a copy refreshed from its slot while the slot holds it: GetParticleDirect reads the slot
/// whether or not the particle lives, so a killed one stays where it died; a new particle in its slot is not followed.
#[allow(clippy::too_many_arguments)]
fn held_source(
    src: &EmitterInstance,
    ts: &TrailSource,
    trails: &mut [Trail],
    sel: &mut f64,
    ti: usize,
    sys: &SysState,
    rng: &mut dyn RandomSource,
) -> V3 {
    let n = src.particles.len();
    if n == 0 {
        return sys.loc;
    }
    if let Some(h) = trails[ti].held.as_mut()
        && let Some(p) = src.slots.get(h.slot)
        && p.uid == h.uid
    {
        h.loc = p.loc;
        h.rel_time = p.rel_time;
    }
    if trails[ti].held.is_some_and(|h| h.rel_time > 1.0) {
        trails[ti].held = None;
    }
    if trails[ti].held.is_none() {
        let i = if ts.random {
            (rng.next_f64() * n as f64).trunc()
        } else {
            *sel += 1.0;
            if *sel >= n as f64 {
                *sel = 0.0;
            }
            loop {
                let uid = src.slots[src.particles[*sel as usize]].uid;
                let taken = trails.iter().enumerate().any(|(k, s)| k != ti && s.held.is_some_and(|h| h.uid == uid));
                if !taken {
                    break;
                }
                *sel += 1.0;
                if *sel >= n as f64 {
                    *sel = -1.0;
                    break;
                }
            }
            *sel
        };
        trails[ti].held = (i >= 0.0).then(|| {
            let slot = src.particles[i as usize];
            let p = &src.slots[slot];
            Held { slot, uid: p.uid, loc: p.loc, rel_time: p.rel_time }
        });
    }
    let s = &mut trails[ti];
    match s.held {
        None => {
            s.src_time = 0.0;
            sys.loc
        }
        Some(h) => {
            s.src_time = h.rel_time;
            if src.t.local { add3(h.loc, sys.loc) } else { h.loc }
        }
    }
}

/// A world point into a local emitter's simulation space (InverseTransformPosition); a world emitter's unchanged.
fn from_world(local: bool, sys: &SysState, w: V3) -> V3 {
    if !local {
        return w;
    }
    sys.inverse_transform_vector([w[0] - sys.loc[0], w[1] - sys.loc[1], w[2] - sys.loc[2]])
}

/// UParticleModuleCollision::Update (142c5e3e0) in the module's place in the update list, per particle:
/// - skipped with Freeze, IgnoreCollisions, FreezeTranslation or FreezeRotation; with DelayCollisions while
///   RelativeTime < Delay, then the flag goes. Neither RelativeTime past 1 nor UsedCollisions is tested;
/// - Location = Location + Velocity x DeltaTime (UpdateBoundingBox moves it after the modules) and OldLocation, both
///   through ComponentToWorld (with scale) in a local emitter; Direction = their difference normalised; Size =
///   Particle.Size x the component's Scale3D; End = Location + Direction x Size / DirScalar (3.5, the class default);
/// - Extent: a mesh emitter's mesh bounds BoxExtent x Size (bCollisionsConsiderPartilceSize: Unverified, taken as the
///   type data's 4.26 class default true, no emitter here sets it), else 0: a box sweep in place of the line trace;
/// - the distance gate (MaxCollisionDistance below 2097152, class default 1000): a particle whose End is not within
///   MaxCollisionDistance / LODFactor of the view (LODFactor: the view's FOV / 90, PC +0x2e8 at 142b59fe5; the view of
///   the last drawn frame, SysState.collision_view) gets IgnoreCollisions for life. Batcave's camera (arm 6000 at
///   least, FOV 26) puts every PS_Droplets drop out of reach, so no drop collides, as in the build;
/// - the trace from OldLocation to End. What it meets is the stage's horizontal surfaces from above (SysState.surfaces:
///   the floor and the solids stood on), not the drawn geometry, the fighters or the actors (the build traces the
///   world's collision channels): Normal (0, 0, 1), the hit where the line (a sweep: the box's bottom, its footprint
///   against the surface's rect) meets the plane, Hit.Location the line's point (the box centre), Hit.Time its fraction;
/// - a hit: every normal here is vertical (bOnlyVerticalNormalsDecrementCount counts it) and no surface is a Pawn or a
///   trigger, so UsedCollisions--. Below 1: Location = Hit.Location (back through the inverse transform in a local
///   emitter), then EPCC_Kill (RelativeTime past 1: KillParticles takes it next tick) or the option's flags. Else the
///   bounce: BaseVelocity mirrored by the normal x UsedDampingFactor, BaseRotationRate x UsedDampingFactorRotation.X,
///   the mesh rotation payload's RotationRateBase x UsedDampingFactorRotation, Velocity = 0 and Location += Direction
///   mirrored x |Location - OldLocation| x UsedDampingFactor x (1 - Hit.Time) (a local emitter's in world space
///   through ComponentToWorld and back). Then the EventGenerator's EPET_Collision entries report the hit's location
///   (Frequency as the spawn entries; the particle's velocity, the EmitterTime), a killed particle's too, and the
///   particle gets CollisionHasOccurred.
///
/// Unverified: the local-space bounce's transforms follow the 4.26 source (the exe's branch "does the same through
/// ComponentToWorld", not read step by step); bApplyPhysics' impulse has nothing to push here.
/// Deviation: the 0.1 s LastRenderTime gate (bCollideOnlyIfVisible) and the box test once in 31 updates (more than 7
/// live, bounds grown by the reach x 1.075: it fails only when every particle would) are not ported; nor the kill
/// removing the particle at once inside the Update (here at the next KillParticles: drawn one more frame at the hit),
/// nor a zero-length move's trace (the exe traces to Location with a zero Direction; here no trace).
#[allow(clippy::too_many_arguments)]
fn collide(
    t: &EmitterTemplate,
    done: Completion,
    reach: f64,
    p: &mut Particle,
    dt: f64,
    sys: &mut SysState,
    tracking: &mut f64,
    time: f64,
) {
    if p.flags & PF_COLLISION_IGNORE_CHECK != 0 {
        return;
    }
    if p.flags & PF_DELAY_COLLISIONS != 0 {
        if p.rel_time < p.coll_delay {
            return;
        }
        p.flags &= !PF_DELAY_COLLISIONS;
    }
    let Some(surfaces) = sys.surfaces.clone() else { return };
    let local = t.local;
    let l = sys.to_world(local, [p.loc[0] + p.vel[0] * dt, p.loc[1] + p.vel[1] * dt, p.loc[2] + p.vel[2] * dt]);
    let o = sys.to_world(local, p.old);
    let mut d = [l[0] - o[0], l[1] - o[1], l[2] - o[2]];
    let len = hypot3(d[0], d[1], d[2]);
    if len < 1e-8 {
        return;
    }
    for x in &mut d {
        *x /= len;
    }
    let (sx, sy, sz) = (p.size[0] * sys.scale[0], p.size[1] * sys.scale[1], p.size[2] * sys.scale[2]);
    let (ex, ey, ez) = (l[0] + (d[0] * sx) / 3.5, l[1] + (d[1] * sy) / 3.5, l[2] + (d[2] * sz) / 3.5);
    if reach < 2097152.0 {
        let near = sys.collision_view.is_some_and(|v| {
            (pow(ex - v.loc[0], 2.0) + pow(ey - v.loc[1], 2.0) + pow(ez - v.loc[2], 2.0)) * v.lod * v.lod
                < reach * reach
        });
        if !near {
            p.flags |= PF_IGNORE_COLLISIONS;
            return;
        }
    }
    if !(ez < o[2]) {
        return;
    }
    let (bx, by, bz) = if t.kind == Kind::Mesh {
        let me = t.mesh_extent;
        ((me[0] * sx).abs(), (me[1] * sy).abs(), (me[2] * sz).abs())
    } else {
        (0.0, 0.0, 0.0)
    };
    let (mut best, mut hx, mut hy, mut hz) = (2.0, 0.0, 0.0, 0.0);
    for s in surfaces.iter() {
        let z = s.z + bz;
        if !(o[2] >= z && ez < z) {
            continue;
        }
        let tt = (o[2] - z) / (o[2] - ez);
        if tt >= best {
            continue;
        }
        let x = o[0] + (ex - o[0]) * tt;
        let y = o[1] + (ey - o[1]) * tt;
        if let Some(r) = s.rect
            && (x + bx < r[0] || x - bx > r[1] || y + by < r[2] || y - by > r[3])
        {
            continue;
        }
        best = tt;
        hx = x;
        hy = y;
        hz = z;
    }
    if best > 1.0 {
        return;
    }
    p.coll -= 1.0;
    if p.coll < 1.0 {
        p.loc = from_world(local, sys, [hx, hy, hz]);
        match done {
            Completion::Kill => p.rel_time = 1.1,
            Completion::Flags(f) => p.flags |= f,
        }
    } else {
        let (df, dr) = (p.coll_damp, p.coll_damp_rot);
        let mut bv = if local { sys.transform_vector(p.base_vel) } else { p.base_vel };
        bv[2] = -bv[2];
        for c in 0..3 {
            bv[c] *= df[c];
        }
        p.base_vel = if local { sys.inverse_transform_vector(bv) } else { bv };
        p.base_rot_rate *= dr[0];
        for c in 0..3 {
            p.mesh_rate_base[c] *= dr[c];
        }
        p.vel = ZERO3;
        let k = len * (1.0 - best);
        let mut w = sys.to_world(local, p.loc);
        w[0] += d[0] * k * df[0];
        w[1] += d[1] * k * df[1];
        w[2] -= d[2] * k * df[2];
        p.loc = from_world(local, sys, w);
    }
    if !t.collision_events.is_empty() {
        *tracking += 1.0;
        let n = *tracking;
        for ev in &t.collision_events {
            if ev.freq == 0.0 || n % ev.freq == 0.0 {
                sys.push_event(&ev.name, EventType::Collision, [hx, hy, hz], p.vel, time);
            }
        }
    }
    p.flags |= PF_COLLISION_HAS_OCCURRED;
}

/// UParticleModuleSubUVMovie::DetermineImageIndex (142c0d6c0): FrameRate at EmitterTime with bUseEmitterTime
/// (+0x70 bit 0), else at RelativeTime; the payload Time += DeltaTime; the image is Time x FrameRate, truncated
/// unless the method is Linear_Blend. No wrap (the fills wrap past the last cell).
fn movie_image(
    t: &EmitterTemplate,
    m: &Module,
    time: f64,
    p: &mut Particle,
    dt: f64,
    sys: &SysState,
    rng: &mut dyn RandomSource,
) -> f64 {
    let Module::SubUvMovie { frame_rate, emitter_time, .. } = m else { return p.sub_image };
    let rate = sample1(frame_rate, if *emitter_time { time } else { p.rel_time }, &sys.params, rng);
    p.sub_time += dt;
    let i = p.sub_time * rate;
    if t.interp == SubUvMethod::LinearBlend { i } else { i.trunc() }
}

/// SourceMovement's FinalUpdate test (142c0f270 step 2): only a particle with RelativeTime > 2 x DeltaTime x
/// OneOverMaxLifetime moves with the component.
pub fn source_movement_aged(rel_time: f64, one_over_life: f64, dt32: f64) -> bool {
    rel_time > fround(fround(2.0 * dt32) * one_over_life)
}
